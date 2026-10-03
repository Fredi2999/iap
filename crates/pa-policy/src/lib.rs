//! Zentrale Sicherheitsschicht nach Konzept Kapitel 10.
//!
//! `pa-policy` ist die einzige Stelle, an der Fähigkeits-, Pfad- und
//! Modusprüfungen leben. Jeder Werkzeugaufruf – ob CLI, UI oder späterer
//! Agentenlauf – geht hier durch, wird protokolliert und darf nur mit einer
//! `Decision::Allow` weiterlaufen.

pub mod audit;
pub mod capability;
pub mod command;
pub mod device;
pub mod egress;
pub mod grants;
pub mod host_root;
pub mod mode;
pub mod path;
pub mod process;

use std::{io, path::PathBuf};

use thiserror::Error;

pub use audit::{
    action_to_str, mode_to_str, outcome_to_str, AuditLog, AuditOutcome, AuditRecord, AuditSink,
    AuditStore,
};
pub use capability::{Capability, CapabilityAction, CapabilityRequest, Decision, DerivationSource};
pub use grants::{Grant, GrantScope, GrantStore, GrantSubject};
pub use mode::Mode;
pub use path::{normalize, resolve_absolute_in_scope, safe_join, PathScope};

/// Verwendet einen abstrakten Fehler, damit die Ursache (Pfad, Modus,
/// Fremdinhalt) für Audit und UI klar getrennt bleibt.
#[derive(Debug, Error)]
pub enum PolicyError {
    /// Aktive Verhinderung eines Verzeichnisausbruchs vor der Kanonisierung.
    #[error("Pfad-Traversal blockiert `{path}`: {reason}")]
    PathTraversal { path: PathBuf, reason: String },
    /// Der kanonische Pfad liegt außerhalb der freigegebenen Wurzeln.
    #[error("Pfad `{path}` liegt außerhalb aller freigegebenen Wurzeln")]
    PathOutOfScope { path: PathBuf },
    /// Windows-Reservenamen (CON, NUL, COM1 …) werden nie zugelassen, auch
    /// nicht innerhalb erlaubter Wurzeln.
    #[error("Pfadkomponente `{component}` in `{path}` ist ein Windows-Reservename")]
    ReservedName { path: PathBuf, component: String },
    /// Aktion durch den aktuellen Modus verweigert.
    #[error("Aktion in Modus {mode:?} verweigert: {reason}")]
    ModeDenied { mode: Mode, reason: String },
    /// Aktion erfordert Bestätigung durch den Nutzer.
    #[error("Bestätigung erforderlich: {0}")]
    ConfirmationRequired(String),
    /// Auto-Freigaben gelten nicht für Aktionen, deren Ziel aus gelesenem
    /// Fremdinhalt stammt (siehe Konzept 10.3).
    #[error("Aktion stammt aus Fremdinhalt und benötigt bewusste Bestätigung")]
    DerivedFromUntrusted,
    /// Die Datenherkunftsgrenze (Exa, Aufnahme, Prozess) wurde verletzt.
    #[error("Datengrenze verletzt: {0}")]
    DataBoundary(String),
    /// Persistenzfehler beim Audit-Log oder Grant-Speicher.
    #[error("Persistenzfehler: {0}")]
    Storage(String),
    /// Unerwarteter IO-Fehler beim Auflösen eines Pfades.
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

impl From<rusqlite::Error> for PolicyError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.to_string())
    }
}
