use std::{io, path::PathBuf};

use thiserror::Error;

/// Redigiert Geheimnisse und trennt Prozess-, Protokoll- und Modellfehler.
#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("lokaler Inferenz-I/O-Fehler: {0}")]
    Io(#[from] io::Error),
    #[error("ungültiger SSE-Stream: {0}")]
    InvalidSse(String),
    #[error("ungültige lokale HTTP-Antwort: {0}")]
    Http(String),
    #[error("llama-server `{path}` konnte nicht gestartet werden: {reason}")]
    Process { path: PathBuf, reason: String },
    #[error("llama-server wurde unerwartet beendet")]
    UnexpectedExit,
    #[error("Modelladapterfehler: {0}")]
    Adapter(String),
}
