//! Verträge für Agent Flow: mehrere Coding-Kandidaten in eigenen Git-Worktrees.

use serde::{Deserialize, Serialize};

/// Höchstzahl der Kandidaten je Aufgabe.
pub const MAX_CANDIDATES: u32 = 5;
/// Vorgabe auf schwacher Hardware (T0).
pub const T0_DEFAULT_CANDIDATES: u32 = 2;

/// Wo das Projekt liegt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectLocation {
    /// Auf dem USB-Stick.
    Stick,
    /// Ausdrücklich freigegebenes Host-Projekt.
    Host,
}

/// Zustand einer Datei im Arbeitsbaum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    Modified,
    Added,
    Deleted,
    Untracked,
    Ignored,
}

/// Eine nicht committete Datei; `sensitive` markiert Muster wie `.env`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyFile {
    pub path: String,
    pub state: FileState,
    pub sensitive: bool,
}

/// Ergebnis der Vorprüfung eines Projekts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPreflight {
    pub repo_root: String,
    pub location: ProjectLocation,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub worktrees_root: String,
    pub filesystem: String,
    pub free_bytes: u64,
    pub dirty_files: Vec<DirtyFile>,
    /// Gründe, die den Start verhindern.
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

/// Startbasis aller Kandidaten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BaseChoice {
    /// Letzter Commit.
    HeadCommit,
    /// Geprüfter Snapshot der aktuellen Änderungen. `include_untracked`
    /// nennt die vom Nutzer ausdrücklich gewählten unversionierten Dateien.
    Snapshot { include_untracked: Vec<String> },
}

/// Tatsächlich verwendete Basis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseInfo {
    pub choice: BaseChoice,
    /// Commit, auf dem die Kandidaten aufbauen (HEAD oder Snapshot-Commit).
    pub base_commit: String,
    /// HEAD des Zielprojekts beim Start; Vergleichswert bei der Übernahme.
    pub target_head: String,
}

/// Anfrage: eine Aufgabe, mehrere Kandidaten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartAgentFlowRequest {
    pub project_path: String,
    /// Ausdrückliche Freigabe für Host-Projekte.
    pub host_project_approved: bool,
    pub prompt: String,
    pub base: BaseChoice,
    pub candidates: Vec<CandidateSpec>,
    /// Dateien, die als Kontext an die Modelle gehen (relative Pfade). Leer = IAP wählt
    /// aus dem Text der Aufgabe.
    #[serde(default)]
    pub context_files: Vec<String>,
}

/// Modell und Budget eines Kandidaten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSpec {
    pub model_id: String,
    pub max_tokens: u32,
    pub max_seconds: u64,
}

/// Stand eines Kandidaten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CandidateStatus {
    /// Wartet in der seriellen Modell-Queue.
    Queued {
        position: u32,
    },
    /// Modell wird geladen.
    Loading,
    Generating,
    /// Patch wird geprüft und im Worktree angewendet.
    Applying,
    Ready,
    Failed {
        reason: String,
    },
    Cancelled,
}

/// Teststatus. Native Tests laufen ohne nachgewiesene Sandbox nicht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TestStatus {
    NotRun { reason: String },
    Passed { summary: String },
    Failed { summary: String },
}

/// Ressourcenverbrauch eines Kandidaten.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub tokens: u32,
    pub seconds: f64,
    pub peak_rss_bytes: Option<u64>,
}

/// Ein Kandidat mit Worktree und Branch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub index: u32,
    pub branch: String,
    pub worktree_path: String,
    pub model_id: String,
    pub status: CandidateStatus,
    pub usage: ResourceUsage,
    pub tests: TestStatus,
    pub changed_files: Vec<String>,
    pub base_commit: String,
}

/// Eine Aufgabe samt Kandidaten.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentFlowTask {
    pub id: String,
    pub project_path: String,
    pub location: ProjectLocation,
    pub prompt: String,
    pub base: BaseInfo,
    pub candidates: Vec<Candidate>,
    pub created_unix_ms: i64,
}

/// Konflikt bei der Übernahme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptConflict {
    pub path: String,
    pub reason: String,
}

/// Vollständige Vorschau vor der Übernahme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptPreview {
    pub candidate_id: String,
    /// Kompletter Unified-Diff Basis → Kandidat.
    pub diff: String,
    pub files: Vec<String>,
    /// Nicht leer: die Übernahme ist gestoppt, nichts wird geschrieben.
    pub conflicts: Vec<AdoptConflict>,
}

/// Ergebnis der Übernahme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptResult {
    pub written_files: Vec<String>,
    /// Nur zur Info: es wurde nichts gepusht und nichts committet.
    pub note: String,
}
