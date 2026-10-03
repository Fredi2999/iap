//! IPC für Projekte (Feature 5) und den Chat mit eigenen Dokumenten (Feature 1).
//!
//! Außerdem der Baustein, der pro Frage Projekt-Grundanweisung und passende
//! Dokument-Abschnitte für den Prompt bestimmt; beide Chat-Pfade (mit und ohne
//! Werkzeuge) nutzen ihn, damit sich Antworten gleich verhalten.

use std::collections::{BTreeSet, HashMap};

use pa_memory::embedder::{Embedder, HashingEmbedder};
use pa_types::{
    ipc::{DocumentInfo, MessageSource, Project},
    memory::{Chunk, ItemType},
};
use pa_vault::{library, memory as vault_memory, repository::VaultRepository};
use tauri::State;

use crate::{documents, now_unix_ms, random_id, require_session, AppError, AppResult, AppState};

/// Höchstens so viele Quellen je Antwort; mehr verwirrt kleine Modelle.
const MAX_SOURCES: usize = 6;
/// Länge des Auszugs, der unter der Quellenmarke angezeigt wird.
const EXCERPT_CHARS: usize = 280;

fn sync(state: &AppState) -> AppResult<()> {
    let session = require_session(state)?;
    let mut vault = session.vault_runtime.lock()?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

// ------------------------------------------------------------------
// Projekte
// ------------------------------------------------------------------

/// Alle Projekte für die Gruppierung der Unterhaltungen.
#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> AppResult<Vec<Project>> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(library::list_projects(vault.repository().connection())?)
}

/// Legt ein Projekt an (`id` leer) oder ändert Name und Grundanweisung.
#[tauri::command]
pub fn save_project(
    state: State<'_, AppState>,
    id: Option<String>,
    name: String,
    system_prompt: String,
) -> AppResult<Project> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::Invalid(
            "Der Projektname muss 1 bis 80 Zeichen haben.".to_owned(),
        ));
    }
    if system_prompt.chars().count() > 4_000 {
        return Err(AppError::Invalid(
            "Die Grundanweisung darf höchstens 4000 Zeichen haben.".to_owned(),
        ));
    }
    let now = now_unix_ms();
    let session = require_session(&state)?;
    let project = {
        let mut vault = session.vault_runtime.lock()?;
        let connection = vault.repository_mut().connection_mut();
        let created = match id.as_deref() {
            Some(existing) => library::get_project(connection, existing)?
                .map(|project| project.created_at_unix_ms)
                .ok_or_else(|| AppError::Invalid("Projekt nicht gefunden".to_owned()))?,
            None => now,
        };
        let project = Project {
            id: id.unwrap_or_else(|| random_id("project")),
            name,
            system_prompt: system_prompt.trim().to_owned(),
            created_at_unix_ms: created,
            updated_at_unix_ms: now,
        };
        library::upsert_project(connection, &project)?;
        project
    };
    sync(&state)?;
    Ok(project)
}

/// Löscht ein Projekt; seine Unterhaltungen bleiben ohne Projekt erhalten.
#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let session = require_session(&state)?;
    {
        let mut vault = session.vault_runtime.lock()?;
        library::delete_project(vault.repository_mut().connection_mut(), &id)?;
    }
    sync(&state)
}

/// Ordnet eine Unterhaltung einem Projekt zu; `None` löst die Zuordnung.
#[tauri::command]
pub fn assign_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
    project_id: Option<String>,
) -> AppResult<()> {
    let session = require_session(&state)?;
    {
        let mut vault = session.vault_runtime.lock()?;
        library::assign_conversation(
            vault.repository_mut().connection_mut(),
            &conversation_id,
            project_id.as_deref(),
        )?;
    }
    sync(&state)
}

// ------------------------------------------------------------------
// Dokumente
// ------------------------------------------------------------------

/// Übernimmt ein Dokument, das die Oberfläche als Rohbytes schickt.
///
/// Die Datei hat der Nutzer im Auswahldialog des Systems selbst gewählt; die
/// WebView liest sie und schickt nur den Inhalt. IAP öffnet dabei keinen
/// Pfad auf dem Host. Der Dateiname kommt URL-kodiert im Header `x-file-name`.
/// Läuft außerhalb des UI-Threads, weil Extraktion und Indexierung dauern können.
#[tauri::command(async)]
pub fn import_document(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> AppResult<DocumentInfo> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(AppError::Invalid(
            "Erwartet werden die Rohdaten der Datei.".to_owned(),
        ));
    };
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .map(percent_decode)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| AppError::Invalid("Dateiname fehlt".to_owned()))?;
    let name: String = name.chars().filter(|c| !c.is_control()).take(160).collect();
    let kind = documents::DocumentKind::from_file_name(&name)
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    let text = documents::extract_text(kind, bytes)
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    let chunks = documents::chunk_text(&text, documents::CHUNK_CHARS);

    let document = DocumentInfo {
        id: random_id("document"),
        name,
        kind: kind.as_str().to_owned(),
        size_bytes: bytes.len() as u64,
        chunk_count: u32::try_from(chunks.len()).unwrap_or(u32::MAX),
        created_at_unix_ms: now_unix_ms(),
    };
    let embedder = HashingEmbedder;
    let session = require_session(&state)?;
    {
        let mut vault = session.vault_runtime.lock()?;
        let connection = vault.repository_mut().connection_mut();
        for (ordinal, text) in chunks.iter().enumerate() {
            let chunk = Chunk {
                id: format!("{}#{ordinal}", document.id),
                doc_id: document.id.clone(),
                ordinal: ordinal as i64,
                text: text.clone(),
                heading_path: None,
                tokens: (text.chars().count() / 4) as i64,
            };
            vault_memory::upsert_chunk(connection, &chunk)?;
            vault_memory::upsert_fts(connection, ItemType::Chunk, &chunk.id, &chunk.text)?;
            if let Ok(vector) = embedder.embed(&chunk.text) {
                vault_memory::upsert_embedding(connection, ItemType::Chunk, &chunk.id, &vector)?;
            }
        }
        library::insert_document(connection, &document)?;
    }
    sync(&state)?;
    Ok(document)
}

/// Alle übernommenen Dokumente.
#[tauri::command]
pub fn list_documents(state: State<'_, AppState>) -> AppResult<Vec<DocumentInfo>> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(library::list_documents(vault.repository().connection())?)
}

/// Entfernt ein Dokument mit allen Abschnitten aus dem Tresor.
#[tauri::command]
pub fn delete_document(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let session = require_session(&state)?;
    {
        let mut vault = session.vault_runtime.lock()?;
        library::delete_document(vault.repository_mut().connection_mut(), &id)?;
    }
    sync(&state)
}

/// Hängt ein Dokument an eine Unterhaltung oder nimmt es wieder ab.
#[tauri::command]
pub fn set_document_attached(
    state: State<'_, AppState>,
    conversation_id: String,
    document_id: String,
    attached: bool,
) -> AppResult<()> {
    let session = require_session(&state)?;
    {
        let mut vault = session.vault_runtime.lock()?;
        let connection = vault.repository_mut().connection_mut();
        if attached {
            library::attach_document(connection, &conversation_id, &document_id)?;
        } else {
            library::detach_document(connection, &conversation_id, &document_id)?;
        }
    }
    sync(&state)
}

/// Dokumente, die an einer Unterhaltung hängen.
#[tauri::command]
pub fn conversation_documents(
    state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<Vec<DocumentInfo>> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    let connection = vault.repository().connection();
    let ids: BTreeSet<String> = library::conversation_document_ids(connection, &conversation_id)?
        .into_iter()
        .collect();
    Ok(library::list_documents(connection)?
        .into_iter()
        .filter(|document| ids.contains(&document.id))
        .collect())
}

/// Gespeicherte Quellen aller Antworten einer Unterhaltung (für die Quellen-Chips).
#[tauri::command]
pub fn conversation_sources(
    state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<Vec<MessageSource>> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(library::conversation_sources(
        vault.repository().connection(),
        &conversation_id,
    )?)
}

/// Dekodiert `%XX`-Folgen (UTF-8), wie sie `encodeURIComponent` erzeugt.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let hex = |byte: u8| (byte as char).to_digit(16);
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                output.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

// ------------------------------------------------------------------
// Kontext je Frage
// ------------------------------------------------------------------

/// Was eine einzelne Frage zusätzlich zum normalen Prompt bekommt.
#[derive(Debug, Default)]
pub struct TurnContext {
    /// Grundanweisung des Projekts der Unterhaltung.
    pub project_prompt: Option<String>,
    /// Nummerierte Quellen aus angehängten Dokumenten.
    pub sources: Vec<MessageSource>,
}

impl TurnContext {
    /// Anweisung plus Quellentext für den Prompt; `None`, wenn keine Quellen da sind.
    pub fn sources_prompt(&self, texts: &HashMap<u32, String>) -> Option<String> {
        if self.sources.is_empty() {
            return None;
        }
        let mut prompt = String::from(
            "Quellen aus angehängten Dokumenten. Beantworte die Frage vorrangig mit diesen Quellen \
             und belege jede Aussage mit der Nummer der Quelle in eckigen Klammern, zum Beispiel [1]. \
             Steht die Antwort nicht in den Quellen, sag das offen.",
        );
        for source in &self.sources {
            let text = texts
                .get(&source.number)
                .map(String::as_str)
                .unwrap_or(&source.excerpt);
            prompt.push_str(&format!(
                "\n\n[{}] {}, Abschnitt {}:\n{}",
                source.number,
                source.document_name,
                source.chunk_ordinal + 1,
                text
            ));
        }
        Some(prompt)
    }
}

/// Bestimmt Projekt-Grundanweisung und Quellen für eine Frage.
///
/// Quellen: BM25 über die Abschnitte der angehängten Dokumente; findet die
/// Suche nichts, gelten die ersten Abschnitte (oft Einleitung oder Überblick).
/// Höchstens ein Viertel des Kontexts geht an Quellen (Spezifikation C1).
pub fn turn_context(
    repository: &VaultRepository,
    conversation_id: &str,
    question: &str,
    context_tokens: u32,
) -> (TurnContext, HashMap<u32, String>) {
    let connection = repository.connection();
    let mut context = TurnContext {
        project_prompt: library::conversation_project(connection, conversation_id)
            .ok()
            .flatten()
            .map(|project| project.system_prompt)
            .filter(|prompt| !prompt.trim().is_empty()),
        sources: Vec::new(),
    };
    let mut texts = HashMap::new();
    let Ok(document_ids) = library::conversation_document_ids(connection, conversation_id) else {
        return (context, texts);
    };
    if document_ids.is_empty() {
        return (context, texts);
    }
    let names: HashMap<String, String> = library::list_documents(connection)
        .unwrap_or_default()
        .into_iter()
        .map(|document| (document.id, document.name))
        .collect();
    let mut chunks: HashMap<String, Chunk> = HashMap::new();
    for document_id in &document_ids {
        for chunk in vault_memory::list_chunks_by_doc(connection, document_id).unwrap_or_default() {
            chunks.insert(chunk.id.clone(), chunk);
        }
    }

    let mut ranked: Vec<String> = Vec::new();
    if let Some(query) = fts_query(question) {
        for (item_type, item_id, _score) in
            vault_memory::search_bm25(connection, &query, 200).unwrap_or_default()
        {
            if item_type == ItemType::Chunk
                && chunks.contains_key(&item_id)
                && !ranked.contains(&item_id)
            {
                ranked.push(item_id);
            }
        }
    }
    if ranked.is_empty() {
        let mut first: Vec<&Chunk> = chunks.values().collect();
        first.sort_by_key(|chunk| (chunk.doc_id.clone(), chunk.ordinal));
        ranked = first.into_iter().map(|chunk| chunk.id.clone()).collect();
    }

    let budget_chars = context_tokens as usize; // ein Viertel der Token, je Token etwa 4 Zeichen
    let mut used = 0usize;
    for id in ranked {
        let Some(chunk) = chunks.get(&id) else {
            continue;
        };
        let length = chunk.text.chars().count();
        if context.sources.len() >= MAX_SOURCES
            || (used + length > budget_chars && !context.sources.is_empty())
        {
            break;
        }
        used += length;
        let number = context.sources.len() as u32 + 1;
        texts.insert(number, chunk.text.clone());
        context.sources.push(MessageSource {
            message_id: String::new(),
            number,
            document_id: chunk.doc_id.clone(),
            document_name: names.get(&chunk.doc_id).cloned().unwrap_or_default(),
            chunk_ordinal: u32::try_from(chunk.ordinal).unwrap_or(0),
            excerpt: chunk.text.chars().take(EXCERPT_CHARS).collect(),
        });
    }
    (context, texts)
}

/// Baut aus einer Frage eine sichere FTS5-Abfrage: nur Wörter, jedes in
/// Anführungszeichen, mit ODER verknüpft. So lösen Sonderzeichen wie `C++?`
/// keinen Syntaxfehler aus.
fn fts_query(question: &str) -> Option<String> {
    let terms: Vec<String> = question
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| term.chars().count() >= 2)
        .take(16)
        .map(|term| format!("\"{}\"", term.to_lowercase()))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

/// Speichert die Quellen einer Antwort, damit die Chips nach einem Neustart
/// bleiben, und liefert sie mit gesetzter Nachrichten-ID für die Oberfläche.
pub fn store_sources(
    state: &AppState,
    assistant_id: &str,
    sources: &[MessageSource],
) -> AppResult<Vec<MessageSource>> {
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let stored: Vec<MessageSource> = sources
        .iter()
        .cloned()
        .map(|mut source| {
            source.message_id = assistant_id.to_owned();
            source
        })
        .collect();
    let session = require_session(state)?;
    let mut vault = session.vault_runtime.lock()?;
    library::save_message_sources(
        vault.repository_mut().connection_mut(),
        assistant_id,
        &stored,
    )?;
    Ok(stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_keeps_only_words() {
        assert_eq!(
            fts_query("Was kostet C++?").as_deref(),
            Some("\"was\" OR \"kostet\"")
        );
        assert_eq!(fts_query("?! +"), None);
    }

    #[test]
    fn percent_decoding_restores_umlauts() {
        assert_eq!(percent_decode("Pr%C3%BCfung%20A.pdf"), "Prüfung A.pdf");
        assert_eq!(percent_decode("ohne%"), "ohne%");
        assert_eq!(percent_decode("%ü%zz"), "%ü%zz");
    }

    #[test]
    fn sources_prompt_numbers_every_source() {
        let context = TurnContext {
            project_prompt: None,
            sources: vec![MessageSource {
                message_id: String::new(),
                number: 1,
                document_id: "d".into(),
                document_name: "Plan.md".into(),
                chunk_ordinal: 0,
                excerpt: "Kurz".into(),
            }],
        };
        let texts = HashMap::from([(1, "Voller Text".to_owned())]);
        let prompt = context.sources_prompt(&texts).expect("Quellen");
        assert!(prompt.contains("[1] Plan.md, Abschnitt 1:\nVoller Text"));
    }
}
