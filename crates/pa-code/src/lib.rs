//! Code-Bereich (Konzept 9): Snapshots, Diff-Erzeugung, gestufte
//! Ausführungsvorbereitung.
//!
//! Das Frontend (CodeMirror 6, Projektbaum, Diff-Ansicht mit
//! Hunk-Annahme) folgt in einer eigenen Session. `pa-code` liefert die
//! reine Rust-Basis:
//!
//! - `snapshot`: Copy-on-Write per Hardlink, wo möglich; Fallback auf
//!   normale Kopie. Vor jedem schreibenden Agentenlauf angelegt.
//! - `diff`: unified diff und Hunk-Struktur für die Diff-Ansicht.
//! - `execution`: Stufe A (Analyse) durch reines Parsen; Stufe B (WASM)
//!   nur strukturell vorbereitet — konkrete Sandbox folgt später;
//!   Stufe C explizit deaktiviert (Konzept 9.3 „Stufe C nur mit
//!   Dev-Pack", Auftrag: „Stufe C nur vorbereiten, nicht aktivieren").

pub mod agent;
pub mod diff;
pub mod execution;
pub mod git;
pub mod patch;
pub mod snapshot;

use thiserror::Error;

pub use diff::{apply_hunks_to_string, DiffHunk, HunkLine, LineOp, UnifiedDiff};
pub use execution::{
    stage_b_prerequisites, ExecutionRequest, ExecutionResult, ExecutionStage, ExecutionStatus,
    StageAvailability, StageBPrerequisite,
};
pub use snapshot::{Snapshot, SnapshotKind, SnapshotManager};

/// Bündelt Fehler von Snapshot, Diff und Execution.
#[derive(Debug, Error)]
pub enum CodeError {
    #[error("Snapshot: {0}")]
    Snapshot(String),
    #[error("Diff: {0}")]
    Diff(String),
    #[error("Execution: {0}")]
    Execution(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
