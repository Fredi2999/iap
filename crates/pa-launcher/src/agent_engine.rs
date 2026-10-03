//! Brücke zwischen [`pa_agents::EscalationRunner`] und
//! [`crate::inference_backend::LlamaServerEngine`].
//!
//! Der [`EscalationRunner`] kennt Inferenz nur über den `EngineCall`-Trait
//! (Konzept 7.1: „ein Modell, mehrere Rollen — nicht mehrere Prozesse").
//! Diese Datei liefert die konkrete Implementierung, die eine `RolePrompt`
//! in einen Chat-Turn übersetzt, alle Deltas zu einem Text sammelt, die
//! Dauer echt misst und `usage`-Tokens durchreicht.
//!
//! Grammatik-Argument aus `EngineCall::call` wird an
//! [`LlamaServerEngine::stream_chat_with_grammar`] weitergereicht — das ist
//! die strukturelle Umsetzung von Konzept 7.2 („strukturierter Output per
//! GBNF-Grammar") auf Serverebene.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

use pa_agents::{AgentError, EngineCall, RolePrompt, StageResult};
use pa_types::chat::{Message, MessageRole, MessageStatus, StreamErrorDto, StreamOutcomeDto};

use crate::inference_backend::LlamaServerEngine;

/// Kleinerer Trait als [`pa_core::engine::ChatEngine`], der zusätzlich das
/// GBNF-Feld akzeptiert. Bewusst als **eigener** Trait: der `ChatEngine`-Trait
/// bleibt kompatibel zur bestehenden Streaming-Werkzeugschleife
/// (Phase 2), Agenten-spezifisches Sampling ist ein Zusatzweg.
pub trait GrammarChatCall: Send {
    fn stream_chat_with_grammar(
        &self,
        messages: &[Message],
        grammar: Option<&str>,
        should_continue: &mut dyn FnMut() -> bool,
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> Result<StreamOutcomeDto, StreamErrorDto>;
}

impl GrammarChatCall for LlamaServerEngine {
    fn stream_chat_with_grammar(
        &self,
        messages: &[Message],
        grammar: Option<&str>,
        should_continue: &mut dyn FnMut() -> bool,
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> Result<StreamOutcomeDto, StreamErrorDto> {
        LlamaServerEngine::stream_chat_with_grammar(
            self,
            messages,
            grammar,
            should_continue,
            on_delta,
        )
    }
}

/// Adapter, den der [`EscalationRunner`] als [`EngineCall`] verwendet.
///
/// Über `E: GrammarChatCall` bleibt der Adapter testbar mit einer Mock-Engine
/// — in Produktion wird immer `Arc<Mutex<LlamaServerEngine>>` eingesetzt
/// (siehe Typalias [`LlamaEngineCallAdapter`]).
pub struct EngineCallAdapter<E: GrammarChatCall> {
    engine: Arc<Mutex<E>>,
    cancel: Arc<AtomicBool>,
}

/// Standardkonstruktion für die Produktions-Engine.
pub type LlamaEngineCallAdapter = EngineCallAdapter<LlamaServerEngine>;

impl<E: GrammarChatCall> EngineCallAdapter<E> {
    /// Bindet den Adapter an eine geteilte Engine.
    ///
    /// Das `cancel`-Signal darf zwischen Adapter und UI (Tauri) geteilt
    /// werden: setzt es der Aufrufer auf `true`, bricht der aktuelle
    /// Rollen-Turn ab und der Runner liefert das bisher beste Ergebnis
    /// (Konzept 7.3).
    pub fn new(engine: Arc<Mutex<E>>, cancel: Arc<AtomicBool>) -> Self {
        Self { engine, cancel }
    }

    /// Bequeme Konstruktion mit eigenem, nicht geteiltem Abbruchsignal.
    /// Der Aufrufer bekommt das Signal via [`Self::cancel_handle`] zurück,
    /// falls er es später betätigen möchte.
    pub fn with_local_cancel(engine: Arc<Mutex<E>>) -> Self {
        Self::new(engine, Arc::new(AtomicBool::new(false)))
    }

    /// Gibt einen Klon des Abbruch-Signals zurück (z. B. für UI-Buttons).
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }
}

impl<E: GrammarChatCall> EngineCall for EngineCallAdapter<E> {
    fn call(
        &mut self,
        prompt: &RolePrompt,
        grammar: Option<&str>,
    ) -> Result<StageResult, AgentError> {
        // Rendern der Rollen-Prompts als Message-Verlauf. Die Rolle des
        // System-Blocks bleibt System, damit llama-server den Cache-
        // Präfix wiederverwenden kann (Konzept 5.3). Der User-Block
        // trägt die dynamischen Turn-Inhalte.
        let messages = role_prompt_to_messages(prompt);

        let cancel = Arc::clone(&self.cancel);
        let mut should_continue = move || !cancel.load(Ordering::SeqCst);
        let mut collected = String::new();
        let mut on_delta = |delta: &str| -> bool {
            collected.push_str(delta);
            true
        };

        let started = Instant::now();
        let engine = self
            .engine
            .lock()
            .map_err(|_| AgentError::Engine("Engine-Mutex vergiftet".to_owned()))?;
        let outcome_result = engine.stream_chat_with_grammar(
            &messages,
            grammar,
            &mut should_continue,
            &mut on_delta,
        );
        // Sperre so früh wie möglich freigeben, damit die UI/andere Threads
        // weiterarbeiten können, bevor wir das Ergebnis auswerten.
        drop(engine);
        let elapsed = started.elapsed();
        let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);

        match outcome_result {
            Ok(outcome) => {
                if outcome.aborted {
                    return Err(AgentError::Aborted);
                }
                // `collected` und `outcome.text` sollten identisch sein;
                // im Zweifel bevorzugen wir den Wert der Engine, weil er
                // auch bei zwischengeschalteten Protokollfehlern konsistent
                // mit `outcome.aborted` ist.
                Ok(StageResult {
                    raw_output: outcome.text,
                    elapsed_ms,
                    prompt_tokens: outcome.prompt_tokens,
                    completion_tokens: outcome.completion_tokens,
                })
            }
            Err(error) => Err(AgentError::Engine(format!(
                "{} (partial={} Zeichen)",
                error.message,
                error.partial_text.chars().count()
            ))),
        }
    }
}

/// Rendert eine [`RolePrompt`] in einen Zwei-Message-Verlauf (`System` +
/// `User`), den der llama-server-Chat-Endpunkt versteht.
///
/// Die synthetischen IDs sind rein technischer Natur; llama-server ignoriert
/// sie, das persistierbare Vault-Modell braucht sie nicht.
pub fn role_prompt_to_messages(prompt: &RolePrompt) -> Vec<Message> {
    vec![
        synthetic_message("role-system", 0, MessageRole::System, &prompt.system),
        synthetic_message("role-user", 1, MessageRole::User, &prompt.user),
    ]
}

fn synthetic_message(tag: &str, position: i64, role: MessageRole, content: &str) -> Message {
    Message {
        id: format!("agent-{tag}"),
        conversation_id: String::new(),
        position,
        role,
        content: content.to_owned(),
        status: MessageStatus::Complete,
        created_at_unix_ms: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_agents::{Role, RolePrompt};

    struct EchoEngine;

    impl GrammarChatCall for EchoEngine {
        fn stream_chat_with_grammar(
            &self,
            messages: &[Message],
            grammar: Option<&str>,
            _should_continue: &mut dyn FnMut() -> bool,
            _on_delta: &mut dyn FnMut(&str) -> bool,
        ) -> Result<StreamOutcomeDto, StreamErrorDto> {
            let last_user = messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, MessageRole::User))
                .map(|m| m.content.clone())
                .unwrap_or_default();
            let text = match grammar {
                Some(g) => format!("[gbnf:{}b] {last_user}", g.len()),
                None => last_user,
            };
            Ok(StreamOutcomeDto {
                text,
                aborted: false,
                timings: Default::default(),
                prompt_tokens: Some(7),
                completion_tokens: Some(3),
            })
        }
    }

    #[test]
    fn role_prompt_becomes_system_plus_user() {
        let prompt = RolePrompt::new(Role::Proposer, "S", "U");
        let messages = role_prompt_to_messages(&prompt);
        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0].role, MessageRole::System));
        assert!(matches!(messages[1].role, MessageRole::User));
        assert_eq!(messages[0].content, "S");
        assert_eq!(messages[1].content, "U");
    }

    #[test]
    fn adapter_returns_stage_result_with_tokens() {
        let engine = Arc::new(Mutex::new(EchoEngine));
        let mut adapter = EngineCallAdapter::with_local_cancel(engine);
        let prompt = RolePrompt::new(Role::Proposer, "S", "Hallo");
        let result = adapter.call(&prompt, None).unwrap();
        assert_eq!(result.raw_output, "Hallo");
        assert_eq!(result.prompt_tokens, Some(7));
        assert_eq!(result.completion_tokens, Some(3));
    }

    #[test]
    fn adapter_passes_grammar_through() {
        let engine = Arc::new(Mutex::new(EchoEngine));
        let mut adapter = EngineCallAdapter::with_local_cancel(engine);
        let prompt = RolePrompt::new(Role::Critic, "S", "Kritik");
        let result = adapter.call(&prompt, Some("root ::= .")).unwrap();
        assert!(result.raw_output.starts_with("[gbnf:"));
    }
}
