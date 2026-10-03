//! Kernfassade des Memory-Systems.
//!
//! `MemoryStore` verbindet Vault-Persistenz, Embedder und Extraktor. Er
//! hält (wie `SharedConversations`) nur eine `Arc<Mutex<HotVault>>`-
//! Referenz und nimmt die Sperre pro Operation – der 5-Minuten-Rücksync
//! greift zwischen zwei Operationen weiterhin.

use std::sync::{Arc, Mutex};

use pa_types::memory::{Fact, FactCategory, ItemType, MemoryHit};
use pa_vault::{hot_copy::HotVault, memory as vault_memory};
use rand_core::{OsRng, RngCore};

use crate::{
    embedder::Embedder,
    extractor::{ExtractedFact, FactExtractor, TriggerReason},
    vector::{assemble_hits, cosine_similarity, reciprocal_rank_fusion, top_k_cosine},
    MemoryError,
};

/// Ein Fakt, wie er dem `MemoryStore` zur Ingestion übergeben wird.
///
/// `id` ist optional; ohne ID vergibt der Store eine 128-bit Zufalls-ID
/// mit Präfix, damit sie visuell von Konversations-IDs unterscheidbar bleibt.
#[derive(Debug, Clone)]
pub struct IngestFact {
    pub id: Option<String>,
    pub text: String,
    pub category: FactCategory,
    pub confidence: f32,
    pub source_message_id: Option<String>,
    pub valid_from_unix_ms: i64,
    pub user_verified: bool,
}

impl From<ExtractedFact> for IngestFact {
    fn from(fact: ExtractedFact) -> Self {
        Self {
            id: None,
            text: fact.text,
            category: fact.category,
            confidence: fact.confidence,
            source_message_id: fact.source_message_id,
            valid_from_unix_ms: 0,
            user_verified: false,
        }
    }
}

/// Steuert eine einzelne Retrieval-Anfrage; folgt Konzept 6.3.
#[derive(Debug, Clone, Copy)]
pub struct RetrievalOptions {
    /// Anzahl der BM25-Kandidaten, die in die RRF einfließen.
    pub bm25_candidates: u32,
    /// Anzahl der Vektor-Kandidaten, die in die RRF einfließen.
    pub vector_candidates: usize,
    /// Endgültige Ergebniszahl (Konzept: 3–6 Chunks, hart auf Budget begrenzt).
    pub final_hits: usize,
    /// Recency-Boost-Alter in ms; Fakten, die innerhalb dieser Spanne
    /// zuletzt angefragt wurden, bekommen einen Score-Aufschlag.
    pub recency_window_ms: i64,
    /// Additive Bonus-Score für angepinnte (`user_verified`) Fakten.
    pub pin_bonus: f64,
    /// Ähnlichkeitsschwelle für die Duplikat-/Widerspruchserkennung
    /// (Konzept: > 0,9).
    pub duplicate_similarity: f32,
}

impl Default for RetrievalOptions {
    fn default() -> Self {
        Self {
            bm25_candidates: 32,
            vector_candidates: 32,
            final_hits: 6,
            recency_window_ms: 7 * 24 * 3600 * 1000,
            pin_bonus: 0.05,
            duplicate_similarity: 0.9,
        }
    }
}

/// Fassade, die Vault, Embedder und Extraktor bündelt.
///
/// Der `Embedder` liegt als `Box<dyn>`, damit die Tauri-Session einen
/// beliebigen (Hashing-, Loopback-, künftig andere) Embedder in demselben
/// Feld halten kann, ohne Generics durch den `AppState` schleifen zu müssen.
pub struct MemoryStore {
    vault: Arc<Mutex<HotVault>>,
    embedder: Box<dyn Embedder + Send + Sync>,
    options: RetrievalOptions,
}

impl MemoryStore {
    /// Bindet den Store an eine bereits geöffnete `HotVault`-Referenz und
    /// einen konkreten Embedder.
    pub fn new<E: Embedder + Send + Sync + 'static>(
        vault: Arc<Mutex<HotVault>>,
        embedder: E,
    ) -> Self {
        Self {
            vault,
            embedder: Box::new(embedder),
            options: RetrievalOptions::default(),
        }
    }

    /// Wie [`Self::new`], aber mit bereits geboxtem Embedder — bequem für
    /// Aufrufer, die die Wahl anhand Laufzeitzustand treffen (z. B.
    /// Hashing- vs. Loopback-Embedder in der Tauri-App).
    pub fn from_boxed(
        vault: Arc<Mutex<HotVault>>,
        embedder: Box<dyn Embedder + Send + Sync>,
    ) -> Self {
        Self {
            vault,
            embedder,
            options: RetrievalOptions::default(),
        }
    }

    /// Überschreibt die Retrieval-Optionen (Tests, Nutzerpräferenz).
    pub fn with_options(mut self, options: RetrievalOptions) -> Self {
        self.options = options;
        self
    }

    /// Nimmt einen extern erkannten Fakt auf.
    ///
    /// Wenn ein bestehender Fakt mit Cosinus-Ähnlichkeit >
    /// [`RetrievalOptions::duplicate_similarity`] existiert, wird der
    /// neue Fakt als Nachfolger geführt und der alte per
    /// `superseded_by` verlinkt (Konzept 6.4). Ist die Ähnlichkeit
    /// ≥ 0,99, wird der bestehende Fakt einfach aktualisiert.
    pub fn ingest(&mut self, fact: IngestFact) -> Result<String, MemoryError> {
        let id = fact.id.clone().unwrap_or_else(|| new_id("fact"));
        let embedding = self.embedder.embed(&fact.text)?;

        // Ein erneutes Speichern unter derselben ID ist eine Änderung, kein Duplikat: Der Fakt darf
        // sich nicht selbst als Vorgänger finden (er würde sich sonst löschen).
        let existing_similar = self
            .find_similar_fact(&embedding)?
            .filter(|(existing_id, _)| existing_id != &id);
        let vault_fact = Fact {
            id: id.clone(),
            text: fact.text,
            category: fact.category,
            confidence: fact.confidence,
            source_message_id: fact.source_message_id,
            valid_from_unix_ms: fact.valid_from_unix_ms,
            valid_until_unix_ms: None,
            superseded_by: None,
            user_verified: fact.user_verified,
            access_count: 0,
            last_accessed_unix_ms: None,
        };

        let mut vault = self.lock()?;
        vault_memory::upsert_fact(vault.repository_mut().connection_mut(), &vault_fact)?;
        vault_memory::upsert_embedding(
            vault.repository_mut().connection_mut(),
            ItemType::Fact,
            &vault_fact.id,
            &embedding,
        )?;
        vault_memory::upsert_fts(
            vault.repository_mut().connection_mut(),
            ItemType::Fact,
            &vault_fact.id,
            &vault_fact.text,
        )?;

        if let Some((existing_id, similarity)) = existing_similar {
            if similarity >= 0.99 {
                // Effektiv derselbe Fakt: den alten löschen, damit wir keine
                // Doppel-Fakten mit fast identischem Text tragen.
                vault_memory::delete_fact(vault.repository_mut().connection_mut(), &existing_id)?;
                vault_memory::delete_embedding(
                    vault.repository_mut().connection_mut(),
                    &existing_id,
                )?;
            } else {
                vault_memory::mark_superseded(
                    vault.repository_mut().connection_mut(),
                    &existing_id,
                    &vault_fact.id,
                    vault_fact.valid_from_unix_ms,
                )?;
            }
        }
        Ok(id)
    }

    /// Führt eine Extraktor-Runde durch und legt alle erkannten Fakten ab.
    ///
    /// Rückgabe: Liste der neu aufgenommenen Fakt-IDs (kann leer sein).
    pub fn ingest_from_extraction(
        &mut self,
        extractor: &dyn FactExtractor,
        conversation_snippet: &str,
        trigger: TriggerReason,
    ) -> Result<Vec<String>, MemoryError> {
        let extracted = extractor.extract(conversation_snippet, trigger)?;
        let mut new_ids = Vec::new();
        for candidate in extracted {
            let id = self.ingest(candidate.into())?;
            new_ids.push(id);
        }
        Ok(new_ids)
    }

    /// Hybride Suche gemäß Konzept 6.3.
    ///
    /// Sucht parallel BM25 und Vektor, kombiniert per RRF, wendet
    /// Recency- und Pin-Boost an und liefert höchstens
    /// [`RetrievalOptions::final_hits`] Ergebnisse.
    pub fn retrieve(&self, query: &str) -> Result<Vec<MemoryHit>, MemoryError> {
        let query_embedding = self.embedder.embed(query)?;
        let vault = self.lock()?;
        let connection = vault.repository().connection();

        let bm25 = vault_memory::search_bm25(connection, query, self.options.bm25_candidates)?
            .into_iter()
            .map(|(item_type, id, _score)| (item_type, id))
            .collect::<Vec<_>>();

        let fact_vectors = vault_memory::load_embeddings_by_type(connection, ItemType::Fact)?;
        let chunk_vectors = vault_memory::load_embeddings_by_type(connection, ItemType::Chunk)?;
        let mut vector_hits = top_k_cosine(
            &query_embedding,
            &fact_vectors,
            self.options.vector_candidates,
            ItemType::Fact,
        );
        vector_hits.extend(top_k_cosine(
            &query_embedding,
            &chunk_vectors,
            self.options.vector_candidates,
            ItemType::Chunk,
        ));
        // Nach Cosinus-Score global sortieren, damit die RRF-Rangreihenfolge
        // aussagekräftig ist.
        vector_hits.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        vector_hits.truncate(self.options.vector_candidates);
        let vector_ranked: Vec<(ItemType, String)> = vector_hits
            .into_iter()
            .map(|(item_type, id, _score)| (item_type, id))
            .collect();

        let fused = reciprocal_rank_fusion(&[bm25, vector_ranked], 60, self.options.final_hits * 4);

        let boosted = self.apply_recency_and_pin_boosts(fused, connection);

        // Preview aus der aktuellen Vault-Sicht bauen (Fakt-Text bzw.
        // Chunk-Text, gekürzt auf ~280 Zeichen).
        let hits = assemble_hits(boosted, |item_type, item_id| {
            preview_for(item_type, item_id, connection)
        });
        Ok(hits.into_iter().take(self.options.final_hits).collect())
    }

    /// Nutzerkontrolle: löscht einen Fakt vollständig aus Vault und Indexen.
    pub fn forget(&mut self, fact_id: &str) -> Result<(), MemoryError> {
        let mut vault = self.lock()?;
        vault_memory::delete_fact(vault.repository_mut().connection_mut(), fact_id)?;
        vault_memory::delete_embedding(vault.repository_mut().connection_mut(), fact_id)?;
        Ok(())
    }

    /// Alle Fakten inklusive abgelöster (für den Graphen).
    pub fn all_facts(&self) -> Result<Vec<Fact>, MemoryError> {
        let vault = self.lock()?;
        vault_memory::list_all_facts(vault.repository().connection()).map_err(MemoryError::from)
    }

    /// Ändert Text, Kategorie und Bestätigung eines bestehenden Fakts, ohne Duplikatprüfung.
    ///
    /// Warum getrennt von [`ingest`](Self::ingest): Eine Bearbeitung behält ID, Alter, Quelle und
    /// Zugriffszähler und darf weder einen anderen Fakt ablösen noch gelöscht werden.
    ///
    /// # Errors
    /// Wenn es den Fakt nicht gibt oder die Persistenz fehlschlägt.
    pub fn update_fact(
        &mut self,
        fact_id: &str,
        text: &str,
        category: FactCategory,
        user_verified: bool,
    ) -> Result<Fact, MemoryError> {
        let embedding = self.embedder.embed(text)?;
        let mut vault = self.lock()?;
        let mut fact = vault_memory::get_fact(vault.repository().connection(), fact_id)?
            .ok_or_else(|| MemoryError::NotFound(fact_id.to_owned()))?;
        fact.text = text.to_owned();
        fact.category = category;
        fact.user_verified = user_verified;
        if user_verified {
            fact.confidence = 1.0;
        }
        vault_memory::upsert_fact(vault.repository_mut().connection_mut(), &fact)?;
        vault_memory::upsert_embedding(
            vault.repository_mut().connection_mut(),
            ItemType::Fact,
            &fact.id,
            &embedding,
        )?;
        vault_memory::upsert_fts(
            vault.repository_mut().connection_mut(),
            ItemType::Fact,
            &fact.id,
            &fact.text,
        )?;
        Ok(fact)
    }

    /// Alle nicht abgelösten Fakten, wie sie der UI-Bereich „Memory" zeigt.
    pub fn active_facts(&self) -> Result<Vec<Fact>, MemoryError> {
        let vault = self.lock()?;
        vault_memory::list_active_facts(vault.repository().connection()).map_err(MemoryError::from)
    }

    fn find_similar_fact(&self, embedding: &[f32]) -> Result<Option<(String, f32)>, MemoryError> {
        let vault = self.lock()?;
        let vectors =
            vault_memory::load_embeddings_by_type(vault.repository().connection(), ItemType::Fact)?;
        drop(vault);
        let mut best: Option<(String, f32)> = None;
        for (id, other) in vectors {
            let similarity = cosine_similarity(embedding, &other);
            if similarity >= self.options.duplicate_similarity {
                match best.as_ref() {
                    Some((_, current)) if *current >= similarity => {}
                    _ => best = Some((id, similarity)),
                }
            }
        }
        Ok(best)
    }

    fn apply_recency_and_pin_boosts(
        &self,
        fused: Vec<(ItemType, String, f64)>,
        connection: &rusqlite::Connection,
    ) -> Vec<(ItemType, String, f64)> {
        let now = current_unix_ms();
        let mut boosted: Vec<(ItemType, String, f64)> = fused
            .into_iter()
            .map(|(item_type, item_id, mut score)| {
                if item_type == ItemType::Fact {
                    if let Ok(Some(fact)) = vault_memory::get_fact(connection, &item_id) {
                        if fact.user_verified {
                            score += self.options.pin_bonus;
                        }
                        if let Some(last) = fact.last_accessed_unix_ms {
                            let age = now.saturating_sub(last);
                            if age >= 0 && age <= self.options.recency_window_ms {
                                score += 0.02;
                            }
                        }
                    }
                }
                (item_type, item_id, score)
            })
            .collect();
        boosted.sort_by(|a, b| {
            b.2.partial_cmp(&a.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.cmp(&b.1))
        });
        boosted
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HotVault>, MemoryError> {
        self.vault.lock().map_err(|_| MemoryError::Poisoned)
    }
}

fn preview_for(item_type: ItemType, item_id: &str, connection: &rusqlite::Connection) -> String {
    match item_type {
        ItemType::Fact => vault_memory::get_fact(connection, item_id)
            .ok()
            .flatten()
            .map(|fact| shorten(&fact.text, 280))
            .unwrap_or_default(),
        ItemType::Chunk => connection
            .query_row("SELECT text FROM chunks WHERE id = ?1", [item_id], |row| {
                row.get::<_, String>(0)
            })
            .ok()
            .map(|text| shorten(&text, 280))
            .unwrap_or_default(),
    }
}

fn shorten(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(max_chars).collect();
    truncated.push('…');
    truncated
}

fn new_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let mut hex = String::with_capacity(prefix.len() + 1 + 32);
    hex.push_str(prefix);
    hex.push('-');
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn current_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedder::HashingEmbedder;
    use crate::extractor::RegexMemorizeExtractor;
    use pa_vault::{
        hot_copy::HotVault,
        key::derive_key,
        meta::{Argon2Parameters, VaultMeta},
    };

    fn fast_meta(salt: [u8; 16]) -> VaultMeta {
        VaultMeta::new(
            salt,
            Argon2Parameters {
                memory_kib: 8 * 1024,
                iterations: 1,
                parallelism: 1,
            },
        )
    }

    fn open_store(dir: &std::path::Path) -> MemoryStore {
        let portable = dir.join("vault.db");
        let hot = dir.join("hot");
        let meta = fast_meta([5; 16]);
        let key = derive_key("memory", &meta).unwrap();
        let vault = HotVault::start(&portable, &hot, key).unwrap();
        let vault = Arc::new(Mutex::new(vault));
        MemoryStore::new(vault, HashingEmbedder).with_options(RetrievalOptions {
            duplicate_similarity: 0.5,
            ..RetrievalOptions::default()
        })
    }

    #[test]
    fn ingest_stores_fact_with_embedding_and_fts_entry() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = open_store(temp.path());
        let id = store
            .ingest(IngestFact {
                id: None,
                text: "Nutzer arbeitet mit Rust und Svelte".to_owned(),
                category: FactCategory::Skill,
                confidence: 0.9,
                source_message_id: None,
                valid_from_unix_ms: 1,
                user_verified: false,
            })
            .unwrap();

        let active = store.active_facts().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id);
    }

    #[test]
    fn retrieve_finds_ingested_fact_via_bm25_or_vector() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = open_store(temp.path());
        let _ = store
            .ingest(IngestFact {
                id: None,
                text: "Nutzer arbeitet mit Rust und Svelte".to_owned(),
                category: FactCategory::Skill,
                confidence: 0.9,
                source_message_id: None,
                valid_from_unix_ms: 1,
                user_verified: false,
            })
            .unwrap();
        let hits = store.retrieve("Rust").unwrap();
        assert!(!hits.is_empty(), "erwartet mindestens einen Treffer");
        assert!(hits[0].preview.contains("Rust"));
    }

    #[test]
    fn ingesting_a_similar_fact_supersedes_the_old_one() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = open_store(temp.path()).with_options(RetrievalOptions {
            duplicate_similarity: 0.5,
            ..RetrievalOptions::default()
        });
        let first = store
            .ingest(IngestFact {
                id: None,
                text: "Nutzer mag Kaffee".to_owned(),
                category: FactCategory::Preference,
                confidence: 0.7,
                source_message_id: None,
                valid_from_unix_ms: 1,
                user_verified: false,
            })
            .unwrap();
        let second = store
            .ingest(IngestFact {
                id: None,
                text: "Nutzer mag Kaffee".to_owned(), // exakt gleich → ähnlicher Vektor
                category: FactCategory::Preference,
                confidence: 0.9,
                source_message_id: None,
                valid_from_unix_ms: 2,
                user_verified: true,
            })
            .unwrap();
        assert_ne!(first, second);
        let active = store.active_facts().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, second);
    }

    #[test]
    fn ingest_from_extraction_persists_new_facts() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = open_store(temp.path());
        let extractor = RegexMemorizeExtractor;
        let snippet = "merk dir: Der Server heißt gemma-e2b\nmerk dir: Kaffee ist gut";
        let new_ids = store
            .ingest_from_extraction(&extractor, snippet, TriggerReason::ExplicitMemorize)
            .unwrap();
        assert_eq!(new_ids.len(), 2);
        assert_eq!(store.active_facts().unwrap().len(), 2);
    }

    #[test]
    fn forget_removes_fact_completely() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = open_store(temp.path());
        let id = store
            .ingest(IngestFact {
                id: None,
                text: "wird gelöscht".to_owned(),
                category: FactCategory::Other,
                confidence: 0.5,
                source_message_id: None,
                valid_from_unix_ms: 1,
                user_verified: false,
            })
            .unwrap();
        store.forget(&id).unwrap();
        assert!(store.active_facts().unwrap().is_empty());
    }

    #[test]
    fn saving_a_fact_again_under_its_own_id_does_not_delete_it() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = open_store(temp.path());
        let ingest = |store: &mut MemoryStore, id: Option<String>, text: &str| {
            store
                .ingest(IngestFact {
                    id,
                    text: text.to_owned(),
                    category: FactCategory::Preference,
                    confidence: 1.0,
                    source_message_id: None,
                    valid_from_unix_ms: 1,
                    user_verified: true,
                })
                .unwrap()
        };
        let id = ingest(&mut store, None, "Nutzer mag Kaffee");
        // Dieselbe ID, identischer Text: früher fand der Fakt sich selbst und löschte sich.
        let again = ingest(&mut store, Some(id.clone()), "Nutzer mag Kaffee");
        assert_eq!(again, id);
        let facts = store.active_facts().unwrap();
        assert_eq!(facts.len(), 1, "der Fakt muss erhalten bleiben");
        assert_eq!(facts[0].id, id);
    }

    #[test]
    fn updating_a_fact_keeps_id_age_and_index_and_changes_text_and_category() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = open_store(temp.path());
        let id = store
            .ingest(IngestFact {
                id: None,
                text: "Nutzer mag Kaffee".to_owned(),
                category: FactCategory::Preference,
                confidence: 0.7,
                source_message_id: Some("msg-1".to_owned()),
                valid_from_unix_ms: 42,
                user_verified: false,
            })
            .unwrap();
        let updated = store
            .update_fact(
                &id,
                "Nutzer trinkt Tee\nund keinen Kaffee",
                FactCategory::Person,
                true,
            )
            .unwrap();
        assert_eq!(updated.id, id);
        assert_eq!(updated.valid_from_unix_ms, 42, "Alter bleibt");
        assert_eq!(
            updated.source_message_id.as_deref(),
            Some("msg-1"),
            "Quelle bleibt"
        );
        assert!(updated.user_verified && updated.confidence >= 1.0);
        assert_eq!(store.active_facts().unwrap().len(), 1);
        // Der Suchindex kennt den neuen Text, nicht den alten.
        let hits = store.retrieve("Tee").unwrap();
        assert!(hits.iter().any(|hit| hit.preview.contains("Tee")));
        assert!(matches!(
            store.update_fact("gibt-es-nicht", "x", FactCategory::Other, false),
            Err(MemoryError::NotFound(_))
        ));
    }

    #[test]
    fn all_facts_includes_superseded_ones() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = open_store(temp.path());
        let mk = |text: &str| IngestFact {
            id: None,
            text: text.to_owned(),
            category: FactCategory::Preference,
            confidence: 0.7,
            source_message_id: None,
            valid_from_unix_ms: 1,
            user_verified: false,
        };
        let first = store.ingest(mk("Nutzer mag Kaffee")).unwrap();
        let _second = store.ingest(mk("Nutzer mag Kaffee mit Milch")).unwrap();
        let all = store.all_facts().unwrap();
        assert!(all.iter().any(|fact| fact.id == first));
        assert!(all.len() >= store.active_facts().unwrap().len());
    }
}
