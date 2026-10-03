//! Klammer zwischen Konversation, Präfix, Kontextbudget und Inferenz.
//!
//! Der Orchestrator kapselt die Reihenfolge, in der Schritt 3 den CLI-Chat
//! ausgeführt hat, und macht sie testbar:
//!
//! 1. Servergesundheit prüfen (und Restart transparent melden).
//! 2. Nutzernachricht persistieren.
//! 3. Assistenten-Platzhalter im Status `Streaming` persistieren.
//! 4. Vollständigen Verlauf aus dem Vault laden.
//! 5. Prompt priorisiert kürzen ([`crate::budget::build_prompt`]).
//! 6. Streaming aufrufen und Deltas an die UI durchreichen.
//! 7. Endzustand atomar in dieselbe Assistentenzeile schreiben – auch bei
//!    Abbruch oder Transportfehler, damit kein sichtbarer Teiltext verloren geht.

use std::{error::Error, fmt};

use pa_types::chat::{MessageRole, MessageStatus, StreamErrorDto, StreamOutcomeDto};

use crate::{
    budget::{build_prompt, BudgetPolicy, HeuristicTokenEstimator, TokenEstimator},
    conversation::Conversations,
    engine::{ChatEngine, EngineReady},
    prompt::PromptPrefix,
    CoreError,
};

/// Beschreibt einen einzelnen Chatturn samt vom Aufrufer erzeugter IDs und Zeit.
///
/// Die IDs kommen bewusst von außen, damit CLI und spätere Tauri-UI ihre eigene
/// Zufallsquelle nutzen und der Orchestrator keine globalen Nebenwirkungen hat.
#[derive(Debug, Clone)]
pub struct TurnRequest<'a> {
    pub conversation_id: &'a str,
    pub user_message_id: &'a str,
    pub assistant_message_id: &'a str,
    pub user_input: &'a str,
    pub now_unix_ms: i64,
}

/// Rückmeldung eines erfolgreich abgeschlossenen Turns – einschließlich Abbruch.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnOutcome {
    pub user_message_id: String,
    pub assistant_message_id: String,
    pub outcome: StreamOutcomeDto,
    pub engine_status: EngineReady,
    pub dropped_older_turns: usize,
}

/// Fehler eines einzelnen Turns; im Streaming-Fall wurde der Teiltext schon
/// persistiert, damit die Persistenzgrenze nicht überschritten wird.
///
/// `Stream` boxt den Detailbericht bewusst, damit `Result<TurnOutcome, TurnError>`
/// nicht durch die große Variante aufgebläht wird (Clippy `result_large_err`).
#[derive(Debug)]
pub enum TurnError {
    /// Deterministischer Fehler außerhalb der Server-Kommunikation.
    Core(CoreError),
    /// Transport-/Protokollfehler; `partial_text` wurde bereits als
    /// `aborted` unter `assistant_message_id` in den Vault geschrieben.
    Stream(Box<StreamFailure>),
}

/// Detailbericht eines Streaming-Fehlers samt persistierter Nachricht.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamFailure {
    pub source: StreamErrorDto,
    pub assistant_message_id: String,
}

impl fmt::Display for TurnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => write!(formatter, "Core-Fehler: {error}"),
            Self::Stream(failure) => {
                write!(formatter, "Streamfehler: {}", failure.source.message)
            }
        }
    }
}

impl Error for TurnError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Core(error) => Some(error),
            Self::Stream(_) => None,
        }
    }
}

impl From<CoreError> for TurnError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}

/// Bündelt Callbacks, damit die Signatur des Turn-Aufrufs kurz bleibt.
///
/// Die drei Referenzen werden gezielt auf denselben äußeren Lebenszeitparameter
/// gelegt, damit der Orchestrator sie beim Streaming-Aufruf disjunkt
/// reborrowen kann.
pub struct TurnCallbacks<'a> {
    /// Wird periodisch geprüft (auch während der Prompt-/Header-Wartezeit).
    /// `false` bricht den Stream ab, ohne den bereits sichtbaren Teiltext zu verwerfen.
    pub should_continue: &'a mut dyn FnMut() -> bool,
    /// Wird für jedes Serverdelta aufgerufen; die UI ist für die Anzeige verantwortlich.
    pub on_delta: &'a mut dyn FnMut(&str),
    /// Wird einmal vor der Persistierung der Nutzernachricht aufgerufen, sobald
    /// bekannt ist, ob der Inferenzserver neu gestartet werden musste.
    pub on_engine_ready: &'a mut dyn FnMut(EngineReady),
}

/// Führt die Turn-Reihenfolge deterministisch aus und ist damit ohne realen
/// Server testbar (Engine, Konversationsspeicher und Estimator sind Traits).
pub struct ChatOrchestrator {
    prefix: PromptPrefix,
    policy: BudgetPolicy,
    estimator: Box<dyn TokenEstimator + Send + Sync>,
}

impl ChatOrchestrator {
    /// Nutzt den heuristischen Token-Schätzer und die im MVP genutzten Reservate.
    pub fn new(prefix: PromptPrefix, policy: BudgetPolicy) -> Self {
        Self {
            prefix,
            policy,
            estimator: Box::new(HeuristicTokenEstimator),
        }
    }

    /// Ersetzt den Token-Schätzer, z. B. für Tests oder einen echten Tokenizer.
    pub fn with_estimator(mut self, estimator: Box<dyn TokenEstimator + Send + Sync>) -> Self {
        self.estimator = estimator;
        self
    }

    /// Ersetzt den Präfix, ohne den Orchestrator neu zu bauen.
    pub fn set_prefix(&mut self, prefix: PromptPrefix) {
        self.prefix = prefix;
    }

    /// Ersetzt das Budget, ohne den Orchestrator neu zu bauen.
    pub fn set_policy(&mut self, policy: BudgetPolicy) {
        self.policy = policy;
    }

    /// Liest die aktuelle Präfixstruktur (für UI-Anzeigen und Tests).
    pub fn prefix(&self) -> &PromptPrefix {
        &self.prefix
    }

    /// Liest die aktuelle Budgetkonfiguration.
    pub fn policy(&self) -> BudgetPolicy {
        self.policy
    }

    /// Führt einen einzelnen Chatturn aus.
    pub fn run_turn(
        &self,
        conversations: &mut dyn Conversations,
        engine: &mut dyn ChatEngine,
        request: TurnRequest<'_>,
        callbacks: &mut TurnCallbacks<'_>,
    ) -> Result<TurnOutcome, TurnError> {
        let engine_status = engine.ensure_ready()?;
        (callbacks.on_engine_ready)(engine_status);

        conversations.append(
            request.user_message_id,
            request.conversation_id,
            MessageRole::User,
            request.user_input,
            MessageStatus::Complete,
            request.now_unix_ms,
        )?;
        conversations.append(
            request.assistant_message_id,
            request.conversation_id,
            MessageRole::Assistant,
            "",
            MessageStatus::Streaming,
            request.now_unix_ms,
        )?;

        let history = conversations.messages(request.conversation_id)?;
        // Die Streaming-Zeile darf nie Teil des zu sendenden Prompts sein; sie
        // ist ein Platzhalter, den wir gleich mit dem finalen Text füllen.
        let history_without_placeholder: Vec<_> = history
            .into_iter()
            .filter(|message| message.id != request.assistant_message_id)
            .collect();

        let budgeted = build_prompt(
            &self.prefix,
            &history_without_placeholder,
            self.policy,
            self.estimator.as_ref(),
        )?;

        // Zwei Felder von `callbacks` werden gleichzeitig mutabel reborrowed;
        // die Destrukturierung macht das für den Borrow-Checker sichtbar.
        let TurnCallbacks {
            should_continue,
            on_delta,
            ..
        } = &mut *callbacks;
        let stream_result = engine.stream_chat(
            &budgeted.messages,
            &mut **should_continue,
            &mut |delta: &str| {
                (**on_delta)(delta);
                true
            },
        );

        match stream_result {
            Ok(outcome) => {
                let status = if outcome.aborted {
                    MessageStatus::Aborted
                } else {
                    MessageStatus::Complete
                };
                conversations.finish(request.assistant_message_id, &outcome.text, status)?;
                Ok(TurnOutcome {
                    user_message_id: request.user_message_id.to_owned(),
                    assistant_message_id: request.assistant_message_id.to_owned(),
                    outcome,
                    engine_status,
                    dropped_older_turns: budgeted.dropped_older_turns,
                })
            }
            Err(error) => {
                // Auch bei Transportfehlern wird der bereits sichtbare Teiltext
                // atomar in derselben Assistentenzeile persistiert.
                conversations.finish(
                    request.assistant_message_id,
                    &error.partial_text,
                    MessageStatus::Aborted,
                )?;
                Err(TurnError::Stream(Box::new(StreamFailure {
                    source: error,
                    assistant_message_id: request.assistant_message_id.to_owned(),
                })))
            }
        }
    }
}
