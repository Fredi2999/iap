use serde::{Deserialize, Serialize};

/// Begrenzt den Denkaufwand pro Turn, damit die UI eine wirkliche Inferenzoption übergibt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevel {
    Kurz,
    #[default]
    Standard,
    #[serde(rename = "sorgfältig")]
    Sorgfaeltig,
    Vertieft,
    Maximal,
}

/// Beschränkt persistierte Rollen auf die im MVP tatsächlich verwendeten Chatrollen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// Bewahrt abgebrochene Teilantworten als absichtlichen Zustand statt als defekte Nachricht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Complete,
    Streaming,
    Aborted,
}

/// Ist der schlanke IPC- und Persistenzvertrag einer Konversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
    /// Projekt, zu dem die Unterhaltung gehört; `None` = ohne Projekt.
    #[serde(default)]
    pub project_id: Option<String>,
}

/// Hält eine stabile Position, damit Reihenfolge nicht von Zeitstempelauflösung abhängt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub position: i64,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    pub created_at_unix_ms: i64,
}

/// Trennt vom Server tatsächlich gemeldete Raten von nicht gelieferten Feldern, damit die UI nichts schätzt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerTimingsDto {
    pub prompt_ms: Option<f64>,
    pub prompt_per_second: Option<f64>,
    pub predicted_per_second: Option<f64>,
}

/// Bewahrt Streaming-Ergebnisse an einer serialisierbaren Grenze inklusive Abbruchmarker.
///
/// `prompt_tokens` und `completion_tokens` sind `None`, wenn der Server keine
/// `usage`-Statistik liefert – niemals geschätzt.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StreamOutcomeDto {
    pub text: String,
    pub aborted: bool,
    pub timings: ServerTimingsDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
}

/// Klassifiziert Transportfehler in eine feste Menge, damit die UI ohne Textanalyse reagieren kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamErrorKind {
    Transport,
    Protocol,
    ProcessExited,
    Adapter,
}

/// Trägt Transportfehler und bereits gestreamten Teiltext gemeinsam zur Persistenzgrenze.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamErrorDto {
    pub kind: StreamErrorKind,
    pub message: String,
    pub partial_text: String,
    pub timings: ServerTimingsDto,
}
