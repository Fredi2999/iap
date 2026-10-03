//! Git-Baustein für Agent Flow: Lesen mit `gix`, Schreiben über das begrenzte
//! MinGit-Paket (Konzept 10.5).

pub mod cli;
pub mod flow;
pub mod repo;

use thiserror::Error;

/// Fehler der Git-Schicht.
#[derive(Debug, Error)]
pub enum GitError {
    /// Kein (brauchbares) Git-Repository.
    #[error("{0}")]
    Repo(String),
    /// Die Policy hat den Aufruf verweigert.
    #[error("Nicht erlaubt: {0}")]
    Policy(String),
    /// Prozessfehler (Start, Ausgabe).
    #[error("Git-Prozess: {0}")]
    Process(String),
    /// Git meldete einen Fehler.
    #[error("Git `{subcommand}` schlug fehl (Code {code}): {stderr}")]
    Failed {
        subcommand: String,
        code: i32,
        stderr: String,
    },
    /// Zeitgrenze überschritten.
    #[error("Git `{0}` hat die Zeitgrenze überschritten")]
    Timeout(String),
    /// Vom Nutzer abgebrochen.
    #[error("Abgebrochen")]
    Cancelled,
    /// Datei-Ein-/Ausgabe.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// Ein Änderungsvorschlag ist nicht anwendbar.
    #[error("{0}")]
    Patch(#[from] crate::patch::PatchError),
}
