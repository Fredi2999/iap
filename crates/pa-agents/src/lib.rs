//! Agent-Mode: Eskalationsleiter L0–L3 gemäß Konzept Kapitel 7.
//!
//! `pa-agents` kapselt die vier Rollen (Proposer, Critic, Verifier,
//! Synthesizer) als reine Systemprompt-Blöcke — es wird immer nur die
//! **einzige** geladene Modellinstanz verwendet, niemals eine zweite
//! (Konzept 7.1). Der Router schlägt eine Stufe vor, der Frühabbruch
//! nach dem Critic spart die restlichen Rollen, und ein Persistierungs-
//! Trait hält den Zwischenstand fest, damit ein Lauf nach einem Absturz
//! fortgesetzt werden kann.
//!
//! Alles Rendering und alle Werkzeugaufrufe laufen weiterhin über
//! `pa-core::engine::ChatEngine` bzw. `pa-tools::ToolRegistry` — diese
//! Crate zieht kein neues Modell und keinen zweiten Server hoch.

pub mod grammar;
pub mod persist;
pub mod roles;
pub mod router;
pub mod runner;
pub mod schema;
pub mod severity;

use thiserror::Error;

pub use grammar::{CRITIC_GBNF, VERIFIER_GBNF};
pub use persist::{AgentRun, AgentRunStore, InMemoryAgentRunStore};
pub use roles::{Role, RolePrompt, RoleTranscript};
pub use router::{RouterHeuristic, RouterSuggestion, TimeEstimate};
pub use runner::{
    AgentBudgets, EngineCall, EscalationRunner, RunCallbacks, RunOutcome, RunProgress, Stage,
    StageResult,
};
pub use schema::{
    CriticFinding, CriticReport, Severity, VerifierClaim, VerifierReport, VerifierStatus,
};

/// Bündelt Fehler aus Persistenz, Engine-Aufruf und Rollen-Parsing.
#[derive(Debug, Error)]
pub enum AgentError {
    /// Persistenzfehler beim Speichern eines Zwischenstands.
    #[error("Persistenz: {0}")]
    Persistence(String),
    /// Der externe Engine-Callback lieferte einen Fehler.
    #[error("Engine: {0}")]
    Engine(String),
    /// Der Critic-/Verifier-Text passte nicht zur GBNF-Grammatik.
    #[error("strukturierte Ausgabe konnte nicht geparst werden: {0}")]
    Parse(String),
    /// Nutzerabbruch – bereits erzeugte Rollenergebnisse sollen erhalten bleiben.
    #[error("Lauf wurde vom Nutzer abgebrochen")]
    Aborted,
    /// Konfigurationsfehler (z. B. ungültige Stage).
    #[error("ungültige Agenten-Konfiguration: {0}")]
    InvalidConfiguration(String),
}
