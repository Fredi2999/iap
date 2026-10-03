//! Persistenz-Schicht des Memory-Systems (Schemaversion 3).
//!
//! Die Retrieval-, RRF- und Faktenextraktions-Logik lebt in `pa-memory`;
//! diese Datei bietet nur die dünne, testbare Datenbankschicht. Alle
//! öffentlichen Funktionen akzeptieren eine bereits verschlüsselte
//! `rusqlite::Connection`, damit sie sich sowohl aus `VaultRepository`
//! als auch aus Tests direkt aufrufen lassen.

use pa_types::memory::{Chunk, Fact, FactCategory, ItemType, EMBEDDING_DIM};
use rusqlite::{params, Connection, OptionalExtension};

use crate::VaultError;

/// Übernimmt einen Fakt vollständig (Insert oder Update).
///
/// Ein bestehender Fakt mit derselben `id` wird ersetzt; das ist wichtig,
/// damit `user_verified` und `access_count` aus der UI unter demselben
/// Schlüssel überschrieben werden können.
pub fn upsert_fact(connection: &mut Connection, fact: &Fact) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO facts (
             id, text, category, confidence, source_message_id,
             valid_from_unix_ms, valid_until_unix_ms, superseded_by,
             user_verified, access_count, last_accessed_unix_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET
             text = excluded.text,
             category = excluded.category,
             confidence = excluded.confidence,
             source_message_id = excluded.source_message_id,
             valid_from_unix_ms = excluded.valid_from_unix_ms,
             valid_until_unix_ms = excluded.valid_until_unix_ms,
             superseded_by = excluded.superseded_by,
             user_verified = excluded.user_verified,
             access_count = excluded.access_count,
             last_accessed_unix_ms = excluded.last_accessed_unix_ms",
        params![
            fact.id,
            fact.text,
            fact.category.as_str(),
            fact.confidence as f64,
            fact.source_message_id,
            fact.valid_from_unix_ms,
            fact.valid_until_unix_ms,
            fact.superseded_by,
            fact.user_verified as i64,
            fact.access_count,
            fact.last_accessed_unix_ms,
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Setzt `superseded_by` und `valid_until` auf einen bestehenden Fakt.
///
/// Der ersetzte Fakt bleibt lesbar; die Historie ist damit vollständig
/// (Konzept 6.4).
pub fn mark_superseded(
    connection: &mut Connection,
    fact_id: &str,
    successor_id: &str,
    valid_until_unix_ms: i64,
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE facts SET superseded_by = ?2, valid_until_unix_ms = ?3 WHERE id = ?1",
        params![fact_id, successor_id, valid_until_unix_ms],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Erhöht `access_count` und schreibt `last_accessed_unix_ms`.
///
/// Wird nach erfolgreichem Retrieval aufgerufen, damit die Recency- und
/// Popularity-Boosts in `pa-memory` sinnvolle Werte haben.
pub fn record_fact_access(
    connection: &mut Connection,
    fact_id: &str,
    now_unix_ms: i64,
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE facts
         SET access_count = access_count + 1,
             last_accessed_unix_ms = ?2
         WHERE id = ?1",
        params![fact_id, now_unix_ms],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Lädt einen einzelnen Fakt oder liefert `None`.
pub fn get_fact(connection: &Connection, fact_id: &str) -> Result<Option<Fact>, VaultError> {
    connection
        .query_row(
            "SELECT id, text, category, confidence, source_message_id,
                    valid_from_unix_ms, valid_until_unix_ms, superseded_by,
                    user_verified, access_count, last_accessed_unix_ms
             FROM facts WHERE id = ?1",
            [fact_id],
            row_to_fact,
        )
        .optional()
        .map_err(VaultError::from)
}

/// Liefert alle aktiven Fakten (die noch nicht ersetzt wurden), sortiert
/// nach absteigendem Zugriffszähler und Confidence — die UI kann dies
/// direkt anzeigen.
pub fn list_active_facts(connection: &Connection) -> Result<Vec<Fact>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT id, text, category, confidence, source_message_id,
                valid_from_unix_ms, valid_until_unix_ms, superseded_by,
                user_verified, access_count, last_accessed_unix_ms
         FROM facts
         WHERE superseded_by IS NULL
         ORDER BY user_verified DESC, access_count DESC, confidence DESC, id ASC",
    )?;
    let rows = statement
        .query_map([], row_to_fact)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Liefert alle Fakten, auch die ersetzten (für die Verlaufsansicht im Graphen).
pub fn list_all_facts(connection: &Connection) -> Result<Vec<Fact>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT id, text, category, confidence, source_message_id,
                valid_from_unix_ms, valid_until_unix_ms, superseded_by,
                user_verified, access_count, last_accessed_unix_ms
         FROM facts
         ORDER BY valid_from_unix_ms ASC, id ASC",
    )?;
    let rows = statement
        .query_map([], row_to_fact)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Löscht einen Fakt endgültig — wird nur ausgelöst, wenn der Nutzer den
/// Eintrag im Memory-Bereich der UI aktiv entfernt.
pub fn delete_fact(connection: &mut Connection, fact_id: &str) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM facts WHERE id = ?1", [fact_id])?;
    remove_from_indexes(&transaction, ItemType::Fact.as_str(), fact_id)?;
    transaction.commit()?;
    Ok(())
}

/// Fügt einen Dokumentchunk ein oder aktualisiert ihn.
pub fn upsert_chunk(connection: &mut Connection, chunk: &Chunk) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO chunks (id, doc_id, ordinal, text, heading_path, tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
             doc_id = excluded.doc_id,
             ordinal = excluded.ordinal,
             text = excluded.text,
             heading_path = excluded.heading_path,
             tokens = excluded.tokens",
        params![
            chunk.id,
            chunk.doc_id,
            chunk.ordinal,
            chunk.text,
            chunk.heading_path,
            chunk.tokens,
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Alle Chunks eines Dokuments in Ordinalreihenfolge.
pub fn list_chunks_by_doc(connection: &Connection, doc_id: &str) -> Result<Vec<Chunk>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT id, doc_id, ordinal, text, heading_path, tokens
         FROM chunks WHERE doc_id = ?1 ORDER BY ordinal ASC",
    )?;
    let rows = statement
        .query_map([doc_id], |row| {
            Ok(Chunk {
                id: row.get(0)?,
                doc_id: row.get(1)?,
                ordinal: row.get(2)?,
                text: row.get(3)?,
                heading_path: row.get(4)?,
                tokens: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Schreibt oder aktualisiert das Embedding eines Items.
///
/// Prüft die Dimension gegen [`EMBEDDING_DIM`]; ein Mismatch wird zum
/// harten Fehler, damit ein anders dimensioniertes Modell nicht still
/// falsche Cosinus-Werte liefert.
pub fn upsert_embedding(
    connection: &mut Connection,
    item_type: ItemType,
    item_id: &str,
    embedding: &[f32],
) -> Result<(), VaultError> {
    if embedding.len() != EMBEDDING_DIM {
        return Err(VaultError::Integrity(format!(
            "Embedding hat Dimension {}, erwartet {EMBEDDING_DIM}",
            embedding.len()
        )));
    }
    let blob = embedding_to_blob(embedding);
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO vec_items (item_id, item_type, embedding)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(item_id) DO UPDATE SET
             item_type = excluded.item_type,
             embedding = excluded.embedding",
        params![item_id, item_type.as_str(), blob],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Lädt alle Embeddings eines Typs; wird von `pa-memory` in eine
/// Cosine-Suche gefüttert. Für den erwarteten Umfang (Zehntausende
/// Fakten/Chunks) ist ein voller Table-Scan ausreichend schnell und
/// vermeidet die Extension-Abhängigkeit auf sqlite-vec.
pub fn load_embeddings_by_type(
    connection: &Connection,
    item_type: ItemType,
) -> Result<Vec<(String, Vec<f32>)>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT item_id, embedding FROM vec_items WHERE item_type = ?1 ORDER BY item_id ASC",
    )?;
    let rows = statement
        .query_map([item_type.as_str()], |row| {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut vectors = Vec::with_capacity(rows.len());
    for (id, blob) in rows {
        vectors.push((id, blob_to_embedding(&blob)?));
    }
    Ok(vectors)
}

/// Entfernt das Embedding eines Items; wird beim Löschen eines Faktes
/// oder Chunks aufgerufen.
pub fn delete_embedding(connection: &mut Connection, item_id: &str) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM vec_items WHERE item_id = ?1", [item_id])?;
    transaction.commit()?;
    Ok(())
}

/// Schreibt Text in den FTS5-Index. Existierende Zeilen werden zuerst
/// gelöscht, damit ein Item nicht doppelt indiziert wird.
pub fn upsert_fts(
    connection: &mut Connection,
    item_type: ItemType,
    item_id: &str,
    text: &str,
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    remove_from_indexes(&transaction, item_type.as_str(), item_id)?;
    transaction.execute(
        "INSERT INTO fts_items (text, item_type, item_id) VALUES (?1, ?2, ?3)",
        params![text, item_type.as_str(), item_id],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Führt eine BM25-Suche über FTS5 aus und liefert (item_type, item_id,
/// bm25-score) in aufsteigender Reihenfolge (niedriger = besser).
///
/// `limit` verhindert, dass ein sehr allgemeiner Query den kompletten
/// Log durchlaufen muss.
pub fn search_bm25(
    connection: &Connection,
    query: &str,
    limit: u32,
) -> Result<Vec<(ItemType, String, f64)>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT item_type, item_id, bm25(fts_items) AS score
         FROM fts_items
         WHERE fts_items MATCH ?1
         ORDER BY score ASC
         LIMIT ?2",
    )?;
    let rows = statement
        .query_map(params![query, limit], |row| {
            let item_type: String = row.get(0)?;
            let item_id: String = row.get(1)?;
            let score: f64 = row.get(2)?;
            Ok((item_type, item_id, score))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut typed = Vec::with_capacity(rows.len());
    for (item_type, item_id, score) in rows {
        if let Some(kind) = ItemType::parse_wire(&item_type) {
            typed.push((kind, item_id, score));
        }
    }
    Ok(typed)
}

fn remove_from_indexes(
    transaction: &rusqlite::Transaction<'_>,
    item_type: &str,
    item_id: &str,
) -> Result<(), VaultError> {
    transaction.execute(
        "DELETE FROM fts_items WHERE item_type = ?1 AND item_id = ?2",
        params![item_type, item_id],
    )?;
    Ok(())
}

fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(embedding.len() * 4);
    for value in embedding {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob
}

fn blob_to_embedding(blob: &[u8]) -> Result<Vec<f32>, VaultError> {
    if blob.len() != EMBEDDING_DIM * 4 {
        return Err(VaultError::Integrity(format!(
            "Embedding-BLOB hat {} Byte, erwartet {}",
            blob.len(),
            EMBEDDING_DIM * 4
        )));
    }
    let mut vector = Vec::with_capacity(EMBEDDING_DIM);
    for chunk in blob.chunks_exact(4) {
        let mut bytes = [0_u8; 4];
        bytes.copy_from_slice(chunk);
        vector.push(f32::from_le_bytes(bytes));
    }
    Ok(vector)
}

fn row_to_fact(row: &rusqlite::Row<'_>) -> rusqlite::Result<Fact> {
    let category: String = row.get(2)?;
    let confidence: f64 = row.get(3)?;
    Ok(Fact {
        id: row.get(0)?,
        text: row.get(1)?,
        category: FactCategory::from_str_or_other(&category),
        confidence: confidence as f32,
        source_message_id: row.get(4)?,
        valid_from_unix_ms: row.get(5)?,
        valid_until_unix_ms: row.get(6)?,
        superseded_by: row.get(7)?,
        user_verified: row.get::<_, i64>(8)? != 0,
        access_count: row.get(9)?,
        last_accessed_unix_ms: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_types::memory::EMBEDDING_DIM;

    fn open_migrated() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&connection).unwrap();
        connection
    }

    fn sample_fact(id: &str, text: &str, category: FactCategory) -> Fact {
        Fact {
            id: id.to_owned(),
            text: text.to_owned(),
            category,
            confidence: 0.7,
            source_message_id: None,
            valid_from_unix_ms: 1,
            valid_until_unix_ms: None,
            superseded_by: None,
            user_verified: false,
            access_count: 0,
            last_accessed_unix_ms: None,
        }
    }

    #[test]
    fn upsert_and_load_fact_round_trips_all_fields() {
        let mut connection = open_migrated();
        let mut fact = sample_fact("f-1", "Nutzer nutzt Rust", FactCategory::Skill);
        fact.access_count = 3;
        fact.user_verified = true;
        upsert_fact(&mut connection, &fact).unwrap();
        let loaded = get_fact(&connection, "f-1").unwrap().unwrap();
        assert_eq!(loaded, fact);
    }

    #[test]
    fn mark_superseded_hides_from_active_list_but_keeps_row() {
        let mut connection = open_migrated();
        let f1 = sample_fact("f-1", "alt", FactCategory::Preference);
        let f2 = sample_fact("f-2", "neu", FactCategory::Preference);
        upsert_fact(&mut connection, &f1).unwrap();
        upsert_fact(&mut connection, &f2).unwrap();
        mark_superseded(&mut connection, "f-1", "f-2", 100).unwrap();
        let active = list_active_facts(&connection).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "f-2");
        let historical = get_fact(&connection, "f-1").unwrap().unwrap();
        assert_eq!(historical.superseded_by.as_deref(), Some("f-2"));
    }

    #[test]
    fn embedding_dimension_is_enforced_and_roundtrips_exactly() {
        let mut connection = open_migrated();
        let vector: Vec<f32> = (0..EMBEDDING_DIM).map(|i| i as f32 * 0.01).collect();
        upsert_embedding(&mut connection, ItemType::Fact, "f-1", &vector).unwrap();
        let stored = load_embeddings_by_type(&connection, ItemType::Fact).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].0, "f-1");
        assert_eq!(stored[0].1, vector);

        let wrong = vec![0.0_f32; EMBEDDING_DIM - 1];
        let error = upsert_embedding(&mut connection, ItemType::Fact, "bad", &wrong).unwrap_err();
        assert!(matches!(error, VaultError::Integrity(_)));
    }

    #[test]
    fn bm25_finds_indexed_text_and_ignores_unrelated_terms() {
        let mut connection = open_migrated();
        upsert_fts(
            &mut connection,
            ItemType::Fact,
            "f-1",
            "Nutzer arbeitet mit Rust und Svelte",
        )
        .unwrap();
        upsert_fts(
            &mut connection,
            ItemType::Chunk,
            "c-1",
            "Ein Rezept für Sauerteigbrot ohne Rust",
        )
        .unwrap();

        let hits = search_bm25(&connection, "Rust", 10).unwrap();
        assert!(hits
            .iter()
            .any(|(kind, id, _)| *kind == ItemType::Fact && id == "f-1"));

        let none = search_bm25(&connection, "Sonnenuntergang", 10).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn deleting_a_fact_removes_it_from_the_fts_index() {
        let mut connection = open_migrated();
        upsert_fact(
            &mut connection,
            &sample_fact("f-1", "Nutzer liest Bücher", FactCategory::Preference),
        )
        .unwrap();
        upsert_fts(
            &mut connection,
            ItemType::Fact,
            "f-1",
            "Nutzer liest Bücher",
        )
        .unwrap();
        assert!(!search_bm25(&connection, "Bücher", 10).unwrap().is_empty());
        delete_fact(&mut connection, "f-1").unwrap();
        assert!(search_bm25(&connection, "Bücher", 10).unwrap().is_empty());
    }
}
