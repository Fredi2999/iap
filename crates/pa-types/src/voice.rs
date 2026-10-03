//! Verträge für das lokale Sprachgespräch (Aufnahme, Erkennung, Vorlesen).

use serde::{Deserialize, Serialize};

/// Zustand der Sprachsitzung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceState {
    Idle,
    /// Mikrofon ist offen und sichtbar aktiv.
    Listening,
    Transcribing,
    /// Das Sprachmodell antwortet.
    Processing,
    Speaking,
}

/// Ein optionales, abschaltbares Paket (Erkennung, Stimme, Bildprojektor, Git).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    /// Vom Nutzer eingeschaltet (nur sinnvoll bei installiertem Paket).
    pub enabled: bool,
    pub size_bytes: u64,
    /// Version laut Manifest (nur bei installiertem Paket).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Lizenz laut Manifest, damit sie in der Oberfläche sichtbar ist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Warum nicht nutzbar, in Klartext.
    pub missing_reason: Option<String>,
}

/// Warum Sprache gerade gesperrt ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceBlock {
    /// Paket fehlt oder ist ausgeschaltet.
    PackMissing,
    /// T0-Rechner ohne bestandene Messung.
    BenchmarkPending,
    VaultLocked,
    /// Betriebssystem verweigert das Mikrofon.
    MicrophoneDenied,
}

/// Gesamtstatus der Sprachfunktion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceStatus {
    pub available: bool,
    pub block: Option<VoiceBlock>,
    pub state: VoiceState,
    pub muted: bool,
    pub packs: Vec<PackStatus>,
    /// Sprache der Erkennung; Startwert ist die Oberflächensprache.
    pub language: String,
    /// Für die gewählte Sprache gibt es eine lokale Stimme.
    pub voice_available: bool,
    pub benchmark: Option<VoiceBenchmark>,
}

/// Messung auf diesem PC; bestimmt die Freischaltung auf T0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceBenchmark {
    pub measured_unix_ms: i64,
    pub ram_reserve_bytes: u64,
    /// Erkennungsdauer / Audiodauer (kleiner ist besser).
    pub stt_real_time_factor: f64,
    pub tts_first_audio_ms: u64,
    pub load_ms: u64,
    pub passed: bool,
    pub notes: Vec<String>,
}

/// Zustandsänderung für Oberfläche und Avatar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceStateEvent {
    pub state: VoiceState,
    pub muted: bool,
    pub elapsed_ms: u64,
    /// Pegel 0..1 während der Aufnahme.
    pub level: f32,
    pub message: Option<String>,
}

/// Feste, harmlose Navigationsbefehle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceCommand {
    OpenChat,
    OpenFlow,
    OpenWorkflows,
    OpenHome,
}

/// Erkannter Text. Er wird nie automatisch gesendet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    pub language: String,
    pub audio_ms: u64,
    pub transcribe_ms: u64,
    /// Nur bei eindeutigem Treffer gesetzt.
    pub command: Option<VoiceCommand>,
}
