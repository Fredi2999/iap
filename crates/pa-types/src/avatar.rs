//! Zustand des Avatars, Fenstermodus und Job-Übersicht.

use serde::{Deserialize, Serialize};

/// Echte App-Zustände, die der Avatar zeigt. Er behauptet nie Sicherheit
/// oder Korrektheit eines Ergebnisses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvatarState {
    Ready,
    Listening,
    Transcribing,
    Processing,
    Speaking,
    AwaitingApproval,
    Stopped,
    Error,
}

/// Welches Fenster gerade sichtbar ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    Main,
    /// Hauptfenster geschlossen, kleines Pet sichtbar.
    Pet,
}

/// Art eines Hintergrundjobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Chat,
    Agent,
    AgentFlow,
    Workflow,
    Vision,
    SpeechToText,
    TextToSpeech,
    ModelSwitch,
    Compare,
    /// Postfach prüfen und Antworten vorbereiten (Gmail).
    Mail,
}

/// Zustand eines Jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Waiting,
    Running,
    Finished,
    Failed,
    Cancelled,
}

/// Sichtbarer Eintrag der Job-Übersicht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInfo {
    pub id: String,
    pub kind: JobKind,
    pub label: String,
    pub status: JobStatus,
    /// 1 = nächster in der seriellen Modell-Queue; nur bei `Waiting`.
    pub queue_position: Option<u32>,
    pub started_unix_ms: i64,
    pub cancelable: bool,
}
