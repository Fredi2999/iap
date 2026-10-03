//! Integrationstest: EscalationRunner läuft L2 durch den EngineCallAdapter
//! und stellt sicher, dass die GBNF-Grammatik beim Critic gesetzt und beim
//! Proposer/Synthesizer nicht gesetzt ist.

use std::sync::{atomic::AtomicBool, Arc, Mutex};

use pa_agents::{
    grammar, persist::AgentRunStore, AgentBudgets, EngineCall, EscalationRunner,
    InMemoryAgentRunStore, Role, RunCallbacks, RunProgress, Stage,
};
use pa_launcher::agent_engine::{EngineCallAdapter, GrammarChatCall};
use pa_types::chat::{Message, StreamErrorDto, StreamOutcomeDto};

/// Aufzeichnung eines einzelnen Engine-Aufrufs.
#[derive(Debug, Clone)]
struct RecordedCall {
    grammar: Option<String>,
    /// Aus dem letzten User-Block der eingehenden Nachrichten extrahiert,
    /// damit die Rolle im Assertion-Teil identifizierbar ist.
    last_user_text: String,
}

/// Mock, die vorbereitete Antworten liefert und die grammar-Argumente
/// aufzeichnet. Der Mock ist bewusst außerhalb von `pa-agents`, weil er
/// die Adapter-Grenze testet — nicht den Runner.
struct RecordingEngine {
    responses: Mutex<Vec<String>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl RecordingEngine {
    fn new<I: IntoIterator<Item = &'static str>>(responses: I) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(str::to_owned).collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().expect("calls-mutex").clone()
    }
}

impl GrammarChatCall for RecordingEngine {
    fn stream_chat_with_grammar(
        &self,
        messages: &[Message],
        grammar: Option<&str>,
        _should_continue: &mut dyn FnMut() -> bool,
        _on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> Result<StreamOutcomeDto, StreamErrorDto> {
        let last_user_text = messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, pa_types::chat::MessageRole::User))
            .map(|m| m.content.clone())
            .unwrap_or_default();
        self.calls.lock().expect("calls-mutex").push(RecordedCall {
            grammar: grammar.map(str::to_owned),
            last_user_text,
        });
        let mut responses = self.responses.lock().expect("responses-mutex");
        if responses.is_empty() {
            return Ok(StreamOutcomeDto {
                text: "(keine Antwort mehr im Skript)".to_owned(),
                aborted: false,
                timings: Default::default(),
                prompt_tokens: Some(0),
                completion_tokens: Some(0),
            });
        }
        let text = responses.remove(0);
        Ok(StreamOutcomeDto {
            text,
            aborted: false,
            timings: Default::default(),
            prompt_tokens: Some(11),
            completion_tokens: Some(23),
        })
    }
}

#[test]
fn l2_run_through_adapter_passes_grammar_only_to_critic() {
    // Skript-Antworten:
    // 1. Proposer  → freier Text
    // 2. Critic    → JSON mit einem medium-Befund → Synthesizer läuft
    // 3. Synth     → freier Text
    let engine = Arc::new(Mutex::new(RecordingEngine::new([
        "Erster Vorschlag mit Annahmen.",
        r#"{"findings":[{"befund":"unklarer Randfall","schweregrad":"medium","betrifft":"eingabe","vorschlag":"prüfen"}]}"#,
        "Angepasste Endantwort.",
    ])));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut adapter = EngineCallAdapter::new(Arc::clone(&engine), Arc::clone(&cancel));

    let mut store = InMemoryAgentRunStore::new();
    let mut progress: Vec<RunProgress> = Vec::new();
    let mut on_progress = |event: RunProgress| progress.push(event);
    let mut should_continue = || true;
    let mut callbacks = RunCallbacks {
        should_continue: &mut should_continue,
        on_progress: &mut on_progress,
    };
    let runner = EscalationRunner {
        checklist: vec!["Fehlerbehandlung".to_owned(), "Randfälle".to_owned()],
        budgets: AgentBudgets::default(),
    };

    let outcome = runner
        .run(
            "run-l2-adapter",
            Stage::L2Critique,
            "Bewerte den Vorschlag",
            None,
            1,
            &mut adapter,
            &mut store,
            &mut callbacks,
        )
        .expect("L2-Lauf ohne Fehler");

    // 1) Alle drei Rollen liefen durch (Proposer, Critic, Synthesizer).
    assert_eq!(outcome.run.roles.len(), 3);
    assert!(!outcome.early_stopped);
    assert_eq!(outcome.final_answer, "Angepasste Endantwort.");

    let calls = engine.lock().unwrap().calls();
    assert_eq!(calls.len(), 3, "genau drei Engine-Aufrufe erwartet");

    // 2) Grammar-Weitergabe: Proposer=None, Critic=CRITIC_GBNF, Synth=None.
    assert_eq!(
        calls[0].grammar, None,
        "Proposer darf keine Grammatik erhalten"
    );
    assert_eq!(
        calls[1].grammar.as_deref(),
        Some(grammar::CRITIC_GBNF),
        "Critic muss die CRITIC_GBNF-Grammatik erhalten"
    );
    assert_eq!(
        calls[2].grammar, None,
        "Synthesizer darf keine Grammatik erhalten"
    );

    // 3) Prompt-Weitergabe: die Nutzerteile der jeweiligen Rollen müssen im
    //    letzten User-Block landen.
    assert!(
        calls[0].last_user_text.contains("Bewerte den Vorschlag"),
        "Proposer-Prompt muss die Nutzeraufgabe enthalten"
    );
    assert!(
        calls[1].last_user_text.contains("Erster Vorschlag"),
        "Critic-Prompt muss den Proposer-Text zitieren"
    );
    assert!(
        calls[2].last_user_text.contains("Erster Vorschlag"),
        "Synth-Prompt muss den Proposer-Text zitieren"
    );

    // 4) Persistierte StageResults tragen die vom Server gemeldeten Tokens.
    let stored = store.get("run-l2-adapter").unwrap().unwrap();
    for role in &stored.roles {
        assert_eq!(role.prompt_tokens, Some(11));
        assert_eq!(role.completion_tokens, Some(23));
    }
    assert!(matches!(stored.roles[0].role, Role::Proposer));
    assert!(matches!(stored.roles[1].role, Role::Critic));
    assert!(matches!(stored.roles[2].role, Role::Synthesizer));

    // 5) Progress-Events beinhalten am Ende ein `Finished`.
    assert!(matches!(
        progress.last(),
        Some(RunProgress::Finished { .. })
    ));
}

#[test]
fn adapter_reports_aborted_stream_as_agent_aborted() {
    struct AbortingEngine;
    impl GrammarChatCall for AbortingEngine {
        fn stream_chat_with_grammar(
            &self,
            _messages: &[Message],
            _grammar: Option<&str>,
            _should_continue: &mut dyn FnMut() -> bool,
            _on_delta: &mut dyn FnMut(&str) -> bool,
        ) -> Result<StreamOutcomeDto, StreamErrorDto> {
            Ok(StreamOutcomeDto {
                text: "Teiltext".into(),
                aborted: true,
                timings: Default::default(),
                prompt_tokens: None,
                completion_tokens: None,
            })
        }
    }
    let engine = Arc::new(Mutex::new(AbortingEngine));
    let mut adapter = EngineCallAdapter::with_local_cancel(engine);
    let prompt = pa_agents::RolePrompt::new(Role::Proposer, "S", "U");
    let err = adapter.call(&prompt, None).unwrap_err();
    assert!(matches!(err, pa_agents::AgentError::Aborted));
}
