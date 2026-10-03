//! LLM-basierter Faktenextraktor (Konzept 6.4, Meilenstein 9).
//!
//! Bindet die Chat-Engine (via [`GrammarChatCall`]) an den
//! [`pa_memory::extractor::FactExtractor`]-Trait. Der Extraktor formuliert
//! einen strikten JSON-Prompt und erzwingt die Antwortstruktur mit einer
//! GBNF-Grammatik — so kommt keine erfundene Zeile durch. Duplikat- und
//! Widerspruchserkennung passiert weiterhin in `pa_memory::MemoryStore`
//! (nutzt `superseded_by`), diese Bridge liefert nur die Kandidaten.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use pa_memory::extractor::{ExtractedFact, FactExtractor, FactExtractorError, TriggerReason};
use pa_types::{
    chat::{Message, MessageRole, MessageStatus},
    memory::FactCategory,
};
use serde::Deserialize;

use crate::agent_engine::GrammarChatCall;

/// Prompt und Grammatik werden intern als `const` gehalten, damit der KV-Cache
/// des llama-server konsistent bleibt (Konzept 5.3, stabiler Präfix).
const SYSTEM_PROMPT: &str = "Du bist ein Faktenextraktor. Antworte AUSSCHLIESSLICH als JSON: {\"facts\":[{\"text\":\"…\",\"category\":\"preference|project|person|skill|constraint|other\",\"confidence\":0.0}]}. Wenn nichts Merkenswertes drinsteht, antworte {\"facts\":[]}. Keine Erklärungen, keine Prosa, kein Codeblock.";

const FACTS_GBNF: &str = r#"
root       ::= "{\"facts\":" facts "}"
facts      ::= "[]" | "[" fact ("," fact)* "]"
fact       ::= "{"
                 "\"text\":" string ","
                 "\"category\":" category ","
                 "\"confidence\":" number
               "}"
category   ::= "\"preference\"" | "\"project\"" | "\"person\"" | "\"skill\"" | "\"constraint\"" | "\"other\""
string     ::= "\"" schar* "\""
schar      ::= [^"\\] | "\\" ["\\/bfnrt] | "\\u" [0-9a-fA-F]{4}
number     ::= ("0" | [1-9] [0-9]*) ("." [0-9]+)?
"#;

/// LLM-Faktenextraktor über die Chat-Engine.
///
/// Der `cancel`-Handle wird geteilt, damit die UI (z. B. Session-Ende oder
/// Nutzer-Abbruch) einen langen Extraktionslauf sauber stoppen kann; in
/// diesem Fall gilt das Ergebnis als leer, keine halbe Fakten-Liste wird
/// zurückgeliefert.
pub struct LlmFactExtractor<E: GrammarChatCall> {
    engine: Arc<Mutex<E>>,
    cancel: Arc<AtomicBool>,
}

impl<E: GrammarChatCall> LlmFactExtractor<E> {
    /// Standardkonstruktion mit lokalem, nicht geteiltem Abbruchsignal.
    pub fn new(engine: Arc<Mutex<E>>) -> Self {
        Self {
            engine,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Baukasten für UI-Integration: der Aufrufer kann sein eigenes
    /// Abbruchsignal übergeben (z. B. Chat-Cancel).
    pub fn with_cancel(engine: Arc<Mutex<E>>, cancel: Arc<AtomicBool>) -> Self {
        Self { engine, cancel }
    }

    /// Handle zum späteren Abbruch (z. B. UI-Button).
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    fn build_messages(&self, snippet: &str, trigger: TriggerReason) -> Vec<Message> {
        let reason_line = match trigger {
            TriggerReason::SessionEnd => "Anlass: Session-Ende.",
            TriggerReason::ExplicitMemorize => "Anlass: Nutzer sagte explizit 'merk dir'.",
            TriggerReason::IdleQueue => "Anlass: Leerlauf.",
        };
        vec![
            synthetic_message(0, MessageRole::System, SYSTEM_PROMPT),
            synthetic_message(
                1,
                MessageRole::User,
                &format!("{reason_line}\n\n===DIALOG BEGINN===\n{snippet}\n===DIALOG ENDE==="),
            ),
        ]
    }
}

fn synthetic_message(position: i64, role: MessageRole, content: &str) -> Message {
    Message {
        id: format!("extract-{position}"),
        conversation_id: String::new(),
        position,
        role,
        content: content.to_owned(),
        status: MessageStatus::Complete,
        created_at_unix_ms: 0,
    }
}

#[derive(Deserialize)]
struct FactsPayload {
    facts: Vec<RawFact>,
}

#[derive(Deserialize)]
struct RawFact {
    text: String,
    category: String,
    confidence: f32,
}

fn map_category(raw: &str) -> FactCategory {
    match raw {
        "preference" => FactCategory::Preference,
        "project" => FactCategory::Project,
        "person" => FactCategory::Person,
        "skill" => FactCategory::Skill,
        "constraint" => FactCategory::Constraint,
        _ => FactCategory::Other,
    }
}

impl<E: GrammarChatCall> FactExtractor for LlmFactExtractor<E> {
    fn extract(
        &self,
        conversation_snippet: &str,
        trigger: TriggerReason,
    ) -> Result<Vec<ExtractedFact>, FactExtractorError> {
        let messages = self.build_messages(conversation_snippet, trigger);
        let mut collected = String::new();
        let mut cancel_check = || !self.cancel.load(Ordering::SeqCst);
        let mut delta_sink = |delta: &str| {
            collected.push_str(delta);
            true
        };
        let engine = self
            .engine
            .lock()
            .map_err(|_| FactExtractorError::Other("Engine-Mutex vergiftet".to_owned()))?;
        let outcome = engine
            .stream_chat_with_grammar(
                &messages,
                Some(FACTS_GBNF),
                &mut cancel_check,
                &mut delta_sink,
            )
            .map_err(|error| FactExtractorError::Inference(error.message))?;
        if outcome.aborted {
            return Ok(Vec::new());
        }
        let payload: FactsPayload = serde_json::from_str(collected.trim()).map_err(|error| {
            FactExtractorError::Parse(format!(
                "LLM-Antwort ist kein gültiges Facts-JSON: {error}; Rohantwort: {collected}"
            ))
        })?;
        let mut extracted = Vec::with_capacity(payload.facts.len());
        for raw in payload.facts {
            let trimmed = raw.text.trim();
            if trimmed.is_empty() {
                continue;
            }
            extracted.push(ExtractedFact {
                text: trimmed.to_owned(),
                category: map_category(&raw.category),
                confidence: raw.confidence.clamp(0.0, 1.0),
                source_message_id: None,
            });
        }
        Ok(extracted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_types::chat::ServerTimingsDto;
    use pa_types::chat::{StreamErrorDto, StreamErrorKind, StreamOutcomeDto};

    /// Testfake: liefert eine vordefinierte Antwort als Delta.
    struct FakeEngine {
        response: String,
        aborted: bool,
    }

    impl GrammarChatCall for FakeEngine {
        fn stream_chat_with_grammar(
            &self,
            _messages: &[Message],
            _grammar: Option<&str>,
            _should_continue: &mut dyn FnMut() -> bool,
            on_delta: &mut dyn FnMut(&str) -> bool,
        ) -> Result<StreamOutcomeDto, StreamErrorDto> {
            let _ = on_delta(&self.response);
            Ok(StreamOutcomeDto {
                text: self.response.clone(),
                aborted: self.aborted,
                timings: ServerTimingsDto {
                    prompt_ms: None,
                    prompt_per_second: None,
                    predicted_per_second: None,
                },
                prompt_tokens: None,
                completion_tokens: None,
            })
        }
    }

    fn make(response: &str, aborted: bool) -> LlmFactExtractor<FakeEngine> {
        LlmFactExtractor::new(Arc::new(Mutex::new(FakeEngine {
            response: response.to_owned(),
            aborted,
        })))
    }

    #[test]
    fn parses_multiple_facts_and_maps_categories() {
        let extractor = make(
            r#"{"facts":[{"text":"Ich mag Kaffee","category":"preference","confidence":0.9},{"text":"Arbeite an IAP","category":"project","confidence":0.8}]}"#,
            false,
        );
        let facts = extractor
            .extract(
                "Nutzer: Ich mag Kaffee, arbeite an IAP.",
                TriggerReason::SessionEnd,
            )
            .unwrap();
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].category, FactCategory::Preference);
        assert_eq!(facts[1].category, FactCategory::Project);
    }

    #[test]
    fn empty_list_is_a_valid_response() {
        let extractor = make(r#"{"facts":[]}"#, false);
        let facts = extractor
            .extract("nichts merkenswertes", TriggerReason::IdleQueue)
            .unwrap();
        assert!(facts.is_empty());
    }

    #[test]
    fn abort_returns_empty_result_without_partial_facts() {
        let extractor = make(
            r#"{"facts":[{"text":"Halbfertig","category":"other","confidence":0.5}]}"#,
            true,
        );
        let facts = extractor
            .extract("nutzer sprach", TriggerReason::SessionEnd)
            .unwrap();
        assert!(facts.is_empty(), "abort darf keine halben Fakten liefern");
    }

    #[test]
    fn broken_json_yields_parse_error() {
        let extractor = make("kein JSON hier", false);
        let error = extractor
            .extract("dummy", TriggerReason::SessionEnd)
            .unwrap_err();
        assert!(matches!(error, FactExtractorError::Parse(_)));
    }

    #[test]
    fn transport_error_from_engine_bubbles_up() {
        struct Failing;
        impl GrammarChatCall for Failing {
            fn stream_chat_with_grammar(
                &self,
                _messages: &[Message],
                _grammar: Option<&str>,
                _should_continue: &mut dyn FnMut() -> bool,
                _on_delta: &mut dyn FnMut(&str) -> bool,
            ) -> Result<StreamOutcomeDto, StreamErrorDto> {
                Err(StreamErrorDto {
                    kind: StreamErrorKind::Transport,
                    message: "Server weg".to_owned(),
                    partial_text: String::new(),
                    timings: ServerTimingsDto {
                        prompt_ms: None,
                        prompt_per_second: None,
                        predicted_per_second: None,
                    },
                })
            }
        }
        let extractor = LlmFactExtractor::new(Arc::new(Mutex::new(Failing)));
        let error = extractor
            .extract("dialog", TriggerReason::SessionEnd)
            .unwrap_err();
        assert!(matches!(error, FactExtractorError::Inference(_)));
    }
}
