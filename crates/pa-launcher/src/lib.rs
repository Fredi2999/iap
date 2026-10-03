//! Portabler Start, Integritätsprüfung und Ressourcenplanung.

use std::{io, path::PathBuf};

use thiserror::Error;

pub mod agent_engine;
pub mod bootstrap;
pub mod cache;
pub mod cli;
pub mod embedder_bridge;
pub mod eval;
pub mod gguf;
pub mod hardware;
pub mod host_profile;
pub mod inference_backend;
pub mod llm_extractor_bridge;
pub mod manifest;
pub mod model_config;
pub mod paths;
pub mod resources;
pub mod runtime;
pub mod tool_runtime;
pub mod vault_agent_run;
pub mod vault_audit;

mod atomic_file;

/// Bewahrt die konkrete Startphase, damit Integritätsfehler handlungsfähig bleiben.
#[derive(Debug, Error)]
pub enum LauncherError {
    #[error("{action} `{path}` fehlgeschlagen: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Manifest `{path}` ist kein gültiges JSON: {source}")]
    ManifestJson {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsicherer Paketpfad `{path}`: {reason}")]
    UnsafePath { path: String, reason: &'static str },
    #[error("Manifesteintrag `{path}` hat {actual} Bytes statt {expected}")]
    SizeMismatch {
        path: PathBuf,
        expected: u64,
        actual: u64,
    },
    #[error("SHA-256 für `{path}` stimmt nicht: erwartet {expected}, erhalten {actual}")]
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("ungültiges Manifest: {0}")]
    InvalidManifest(String),
    #[error("Ressourcenplan kann nicht erstellt werden: {0}")]
    ResourcePlan(String),
    #[error("GGUF `{path}` ist ungültig: {reason}")]
    InvalidGguf { path: PathBuf, reason: String },
    #[error("Modellbeschreibung `{path}` ist ungültig: {reason}")]
    ModelDescriptor { path: PathBuf, reason: String },
    #[error("Modellkopie wurde abgebrochen")]
    CopyAborted,
    #[error("Modell-Cache ist ungültig: {0}")]
    Cache(String),
}
