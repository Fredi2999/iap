//! Gedächtnis: Graph, Bearbeiten und Lernen aus einer Unterhaltung.
//!
//! Alles läuft lokal. Das Lernen ist eine Modellanfrage und läuft deshalb wie jede andere
//! lange Arbeit in der Job-Warteschlange (sichtbar, abbrechbar), und zwar nur auf ausdrücklichen
//! Wunsch des Nutzers: Auf einem 8-GB-Rechner soll nicht nebenbei ein Modell rechnen.

use std::sync::atomic::Ordering;

use pa_launcher::llm_extractor_bridge::LlmFactExtractor;
use pa_memory::{
    extractor::{FactExtractor, TriggerReason},
    graph::build_graph,
    IngestFact,
};
use pa_types::{
    avatar::JobKind,
    chat::MessageRole,
    memory::{Fact, FactCategory, MemoryGraph},
};
use serde::Deserialize;
use tauri::{AppHandle, Manager, State};

use crate::{now_unix_ms, require_session, session_memory, AppError, AppResult, AppState};

/// Längster Fakttext in Zeichen.
const MAX_FACT_CHARS: usize = 4_000;
/// So viele der letzten Nachrichten gehen in die Extraktion.
const SNIPPET_MESSAGES: usize = 12;
/// So viele Zeichen je Nachricht gehen in die Extraktion.
const SNIPPET_CHARS_PER_MESSAGE: usize = 700;
/// Höchstzahl neuer Fakten pro Lauf; mehr wären ein Zeichen für einen entgleisten Extraktor.
const MAX_LEARNED: usize = 8;

fn invalid(text: impl Into<String>) -> AppError {
    AppError::Invalid(text.into())
}

fn sync_vault(state: &AppState) {
    if let Ok(session) = require_session(state) {
        if let Ok(mut vault) = session.vault_runtime.lock() {
            let mut no_fault = pa_vault::hot_copy::NoFault;
            let _ = vault.sync(&mut no_fault);
        }
    }
}

/// Gedächtnis als Graph. Mit `include_superseded` erscheinen auch abgelöste Fakten.
#[tauri::command]
pub fn memory_graph(
    state: State<'_, AppState>,
    include_superseded: bool,
) -> AppResult<MemoryGraph> {
    let memory = session_memory(&state)?;
    let store = memory
        .lock()
        .map_err(|_| AppError::Internal("Memory-Mutex vergiftet".to_owned()))?;
    let facts = store
        .all_facts()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(build_graph(&facts, include_superseded))
}

/// Anfrage zum Ändern eines bestehenden Fakts.
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryUpdateRequest {
    pub id: String,
    pub text: String,
    pub category: FactCategory,
    pub user_verified: bool,
}

/// Ändert Text, Kategorie und Bestätigung eines bestehenden Fakts.
#[tauri::command]
pub fn update_fact(state: State<'_, AppState>, request: MemoryUpdateRequest) -> AppResult<Fact> {
    let text = request.text.trim();
    if text.is_empty() {
        return Err(invalid("Der Fakt darf nicht leer sein."));
    }
    if text.chars().count() > MAX_FACT_CHARS {
        return Err(invalid(format!(
            "Der Fakt ist zu lang (höchstens {MAX_FACT_CHARS} Zeichen)."
        )));
    }
    let memory = session_memory(&state)?;
    let fact = {
        let mut store = memory
            .lock()
            .map_err(|_| AppError::Internal("Memory-Mutex vergiftet".to_owned()))?;
        store
            .update_fact(&request.id, text, request.category, request.user_verified)
            .map_err(|error| match error {
                pa_memory::MemoryError::NotFound(_) => invalid("Diesen Fakt gibt es nicht mehr."),
                other => AppError::Internal(other.to_string()),
            })?
    };
    sync_vault(&state);
    Ok(fact)
}

/// Verdichtet die letzten Nachrichten zu einem Text für den Extraktor.
fn snippet_of(messages: &[pa_types::chat::Message]) -> String {
    let start = messages.len().saturating_sub(SNIPPET_MESSAGES);
    messages[start..]
        .iter()
        .filter(|m| matches!(m.role, MessageRole::User | MessageRole::Assistant))
        .filter(|m| !m.content.trim().is_empty())
        .map(|m| {
            let who = if m.role == MessageRole::User {
                "Nutzer"
            } else {
                "IAP"
            };
            let text: String = m.content.chars().take(SNIPPET_CHARS_PER_MESSAGE).collect();
            format!("{who}: {}", text.trim())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn learn_blocking(app: &AppHandle, conversation_id: &str) -> AppResult<Vec<Fact>> {
    let state = app.state::<AppState>();
    let session = require_session(&state)?;
    let (snippet, source_message_id) = {
        let vault = session.vault_runtime.lock()?;
        let messages = vault.repository().messages(conversation_id)?;
        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::User)
            .map(|m| m.id.clone());
        (snippet_of(&messages), last_user)
    };
    if snippet.trim().is_empty() {
        return Err(invalid(
            "In dieser Unterhaltung gibt es noch nichts zu lernen.",
        ));
    }

    let mut job =
        state
            .flow
            .jobs
            .submit(JobKind::Agent, "Gedächtnis: aus Unterhaltung lernen", true);
    let job_cancel = job.cancel_flag();
    let mut slot = job.acquire().map_err(|_| invalid("Abgebrochen."))?;
    let extractor = LlmFactExtractor::with_cancel(
        std::sync::Arc::clone(&session.engine),
        std::sync::Arc::clone(&job_cancel),
    );
    let candidates = match extractor.extract(&snippet, TriggerReason::ExplicitMemorize) {
        Ok(candidates) => candidates,
        Err(error) => {
            if job_cancel.load(Ordering::SeqCst) {
                slot.cancelled();
                return Err(invalid("Abgebrochen."));
            }
            slot.fail();
            return Err(invalid(format!(
                "Das Modell konnte keine Fakten erkennen: {error}"
            )));
        }
    };
    drop(slot);

    let memory = session_memory(&state)?;
    let mut learned = Vec::new();
    {
        let mut store = memory
            .lock()
            .map_err(|_| AppError::Internal("Memory-Mutex vergiftet".to_owned()))?;
        let now = now_unix_ms();
        for candidate in candidates.into_iter().take(MAX_LEARNED) {
            let text = candidate.text.trim();
            if text.is_empty() || text.chars().count() > MAX_FACT_CHARS {
                continue;
            }
            // Automatisch gelernt: nie „bestätigt", mit der Quelle der Unterhaltung.
            let id = store
                .ingest(IngestFact {
                    id: None,
                    text: text.to_owned(),
                    category: candidate.category,
                    confidence: candidate.confidence.clamp(0.0, 0.95),
                    source_message_id: source_message_id.clone(),
                    valid_from_unix_ms: now,
                    user_verified: false,
                })
                .map_err(|error| AppError::Internal(error.to_string()))?;
            learned.push(id);
        }
        let active = store
            .active_facts()
            .map_err(|error| AppError::Internal(error.to_string()))?;
        learned = active
            .into_iter()
            .filter(|fact| learned.contains(&fact.id))
            .map(|fact| fact.id)
            .collect();
    }
    let facts = {
        let store = memory
            .lock()
            .map_err(|_| AppError::Internal("Memory-Mutex vergiftet".to_owned()))?;
        store
            .active_facts()
            .map_err(|error| AppError::Internal(error.to_string()))?
            .into_iter()
            .filter(|fact| learned.contains(&fact.id))
            .collect::<Vec<_>>()
    };
    sync_vault(&state);
    Ok(facts)
}

/// Lässt das lokale Modell aus einer Unterhaltung merkenswerte Fakten ableiten. Läuft in der
/// Job-Warteschlange und lässt sich über `code_assist_cancel`-ähnlich per `learn_cancel` abbrechen.
#[tauri::command]
pub async fn learn_from_conversation(
    app: AppHandle,
    conversation_id: String,
) -> AppResult<Vec<Fact>> {
    tauri::async_runtime::spawn_blocking(move || learn_blocking(&app, &conversation_id))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

/// Bricht laufendes Lernen ab.
#[tauri::command]
pub fn learn_cancel(state: State<'_, AppState>) {
    // Nur Lernläufe: dieselbe Jobart nutzt auch der Code-Assistent.
    let ids: Vec<String> = state
        .flow
        .jobs
        .snapshot()
        .into_iter()
        .filter(|job| job.kind == JobKind::Agent && job.label.starts_with("Gedächtnis:"))
        .map(|job| job.id)
        .collect();
    for id in ids {
        state.flow.jobs.cancel(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_types::chat::{Message, MessageStatus};

    fn msg(id: &str, role: MessageRole, text: &str) -> Message {
        Message {
            id: id.to_owned(),
            conversation_id: "c".to_owned(),
            position: 0,
            role,
            content: text.to_owned(),
            status: MessageStatus::Complete,
            created_at_unix_ms: 0,
        }
    }

    #[test]
    fn the_snippet_keeps_only_the_last_user_and_assistant_messages_and_clips_them() {
        let mut messages = vec![msg("0", MessageRole::System, "geheime Systemanweisung")];
        for i in 0..20 {
            messages.push(msg(
                &format!("u{i}"),
                MessageRole::User,
                &format!("Frage {i}"),
            ));
            messages.push(msg(
                &format!("a{i}"),
                MessageRole::Assistant,
                &"x".repeat(2_000),
            ));
        }
        let snippet = snippet_of(&messages);
        assert!(!snippet.contains("geheime Systemanweisung"));
        assert!(snippet.contains("Nutzer: Frage 19"));
        assert!(
            !snippet.contains("Frage 0\n"),
            "alte Nachrichten fallen heraus"
        );
        let longest = snippet
            .lines()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
        assert!(longest <= SNIPPET_CHARS_PER_MESSAGE + "IAP: ".len());
    }

    #[test]
    fn an_empty_conversation_gives_an_empty_snippet() {
        assert!(snippet_of(&[]).is_empty());
        assert!(snippet_of(&[msg("1", MessageRole::User, "   ")]).is_empty());
    }
}
