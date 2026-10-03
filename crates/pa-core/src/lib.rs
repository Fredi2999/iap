//! Orchestrierung von Konversationen, Kontextbudget und Streaming außerhalb der
//! Inferenz- und Vault-Schichten. `pa-core` kennt `pa-inference` bewusst nicht
//! direkt, sondern ausschließlich über den [`engine::ChatEngine`]-Trait.

pub mod budget;
pub mod conversation;
pub mod engine;
pub mod jobs;
pub mod orchestrator;
pub mod prompt;
pub mod tool_loop;
pub mod workflow;

use thiserror::Error;

/// Trennt deterministische Core-Fehler (Vault, Budget, Konfiguration) von den
/// bewusst weitergereichten Streaming-Transportfehlern der Engine.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Weiterreichen realer Vault-Fehler statt eigener, verwaschener Klassifikation.
    #[error("Vault-Operation fehlgeschlagen: {0}")]
    Vault(#[from] pa_vault::VaultError),
    /// Wird gemeldet, wenn schon die aktuelle Nutzernachricht plus Präfix das
    /// Kontextfenster überschreitet – kein stiller Verlust von Eingaben.
    #[error(
        "Prompt sprengt Kontext: {needed_tokens} Tokens benötigt, {available_tokens} verfügbar bei Kontextfenster {context_tokens}"
    )]
    PromptExceedsContext {
        needed_tokens: u32,
        available_tokens: u32,
        context_tokens: u32,
    },
    /// Fasst Startfehler der Inferenz zusammen, damit die UI sie mit einem
    /// klaren Text an den Nutzer weitergeben kann.
    #[error("Inferenz nicht bereit: {0}")]
    EngineUnavailable(String),
    /// Fällt bei widersprüchlichen Startparametern hart, statt eine unsinnige
    /// Konfiguration stillschweigend zu benutzen.
    #[error("ungültige Core-Konfiguration: {0}")]
    InvalidConfiguration(String),
    /// Signalisiert einem übergeordneten Aufrufer, dass ein Mutex im
    /// Vault-Runtime vergiftet ist – der Prozess sollte kontrolliert enden,
    /// statt beschädigten Zustand weiter zu benutzen.
    #[error("interner Mutex im Vault-Runtime ist vergiftet")]
    Poisoned,
}
