//! Serialisierbare IPC-Verträge zwischen Tauri-Frontend und Rust-Backend.
//!
//! Die Typen liegen in `pa-types`, damit Frontend, Backend und CLI dieselbe
//! Definition sehen. Änderungen an dieser Datei sind Vertragsänderungen und
//! werden durch [`chat_contract`](crate::chat) sowie
//! [`ipc_contract`](crate::ipc) Round-Trip-Tests abgesichert.

use serde::{Deserialize, Serialize};

use crate::{
    chat::{
        Conversation, Message, ServerTimingsDto, StreamErrorKind, StreamOutcomeDto, ThinkingLevel,
    },
    hardware::HardwareProfile,
    memory::{Fact, FactCategory, MemoryHit},
    model::{HardwareTier, KvQuantization, ResourcePlan},
};

/// Zusammenfassung des Startvorgangs für die UI (Manifest, Hardware, Ressourcen).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BootstrapStatus {
    pub manifest: ManifestSummary,
    pub hardware: HardwareProfile,
    pub plan: ResourcePlan,
    pub default_model: AvailableModel,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub vault_initialized: bool,
}

/// Kompakter Manifestbericht, damit die UI die Anzahl geprüfter Dateien anzeigen kann.
///
/// `embedding_model_id` ist optional (Konzept, Meilenstein 10): wenn im
/// Manifest gesetzt, weiß der Bootstrap, dass ein zweiter `llama-server` mit
/// `--embedding` gestartet werden soll und die App den `HashingEmbedder`
/// gegen den `LoopbackEmbedder` austauschen darf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestSummary {
    pub version: String,
    pub verified_files: u32,
    pub verified_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_model_id: Option<String>,
}

/// Beschreibt ein installiertes und wirklich vorhandenes Modell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableModel {
    pub id: String,
    pub display_name: String,
    pub family: String,
    pub gguf_bytes: u64,
    pub sha256: String,
    pub max_context_tokens: u32,
    pub is_default: bool,
}

/// Ein Modell aus dem Katalog `AI/models/*.model.toml`, auch wenn die GGUF-Datei fehlt.
///
/// Die Oberfläche zeigt damit alle bekannten Modelle samt Speicherbedarf und Quelle, damit der
/// Nutzer sieht, was er nachladen kann, statt nur die bereits installierten zu kennen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub display_name: String,
    pub family: String,
    pub file_bytes: u64,
    pub peak_ram_bytes_8k: u64,
    pub max_context_tokens: u32,
    pub license: Option<String>,
    pub source_url: Option<String>,
    pub installed: bool,
    pub is_default: bool,
}

/// Konversation samt Nachrichten für den Chat-Ansichts-Aufruf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationDetail {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
}

/// Anforderung, eine Nutzernachricht zu senden.
///
/// `conversation_id = None` bittet das Backend, eine neue Konversation anzulegen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub conversation_id: Option<String>,
    pub content: String,
    #[serde(default)]
    pub thinking_level: ThinkingLevel,
    /// Skill, den der Nutzer mit „/name“ für genau diesen Zug gewählt hat. Der
    /// Nachrichtentext bleibt unverändert, damit der Verlauf zeigt, was getippt wurde.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
}

/// Ereignis, das das Backend während eines Streaming-Turns emittiert.
///
/// Wird pro Turn genau einmal `Started`, beliebig oft `Delta`, und danach
/// entweder `Finished` oder `Failed` senden.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Der Turn wurde persistiert, der Server ist bereit; die UI kann den
    /// leeren Assistenten-Platzhalter zeigen.
    Started {
        conversation_id: String,
        user_message_id: String,
        assistant_message_id: String,
        engine_restarted: bool,
    },
    /// Ein weiteres Serverdelta ist eingetroffen.
    Delta {
        assistant_message_id: String,
        text: String,
    },
    /// Der Stream ist regulär oder per Abbruch beendet und persistiert.
    Finished {
        assistant_message_id: String,
        outcome: StreamOutcomeDto,
        dropped_older_turns: u32,
    },
    /// Transport-/Protokollfehler; `partial_text` wurde als aborted persistiert.
    Failed {
        assistant_message_id: Option<String>,
        error_kind: StreamErrorKind,
        message: String,
        partial_text: String,
        timings: ServerTimingsDto,
    },
}

/// Aktueller Stand der vom Nutzer beeinflussbaren Einstellungen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub tier_override: Option<HardwareTier>,
    pub model_id: String,
    pub context_tokens: u32,
    pub kv_quantization: KvQuantization,
    pub theme: ThemePreference,
    pub vault_path: String,
}

/// UI-Theme; wird im Vault persistiert und nur im Frontend interpretiert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Dark,
    Light,
}

/// Änderungswunsch. `None`-Felder bleiben unverändert, damit die UI partiell
/// aktualisieren kann.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SettingsUpdate {
    pub tier_override: Option<TierOverrideChange>,
    pub model_id: Option<String>,
    pub context_tokens: Option<u32>,
    pub theme: Option<ThemePreference>,
}

/// Modellbezogene Parameter bleiben im verschlüsselten Vault und werden beim
/// nächsten Modellwechsel wieder geladen. `None` nutzt den Ressourcenplan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSettings {
    pub model_id: String,
    pub context_tokens: Option<u32>,
    pub temperature: f32,
    pub top_p: f32,
}

/// Trennt „Tier-Override löschen" von „Tier-Override setzen".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TierOverrideChange {
    Clear,
    Set { tier: HardwareTier },
}

/// Anforderung, einen anderen Vault zu öffnen (Pfad + Passphrase).
///
/// Die Passphrase wird vom Backend sofort in eine `Zeroizing`-Struktur gezogen
/// und nach Ableitung überschrieben; sie darf niemals in Logs auftauchen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultSwitchRequest {
    pub new_vault_path: String,
    pub passphrase: String,
    pub create_if_missing: bool,
}

/// Metadaten eines verfügbaren oder erkannten Datentresors auf dem Speichermedium.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultInfo {
    pub name: String,
    pub path: String,
    pub is_default: bool,
    pub initialized: bool,
    pub size_bytes: u64,
}

/// Benutzerprofil und System-Instruktionen zur Personalisierung des KI-Agenten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserProfile {
    #[serde(default)]
    pub enabled: bool,
    pub user_name: String,
    pub user_about: String,
    pub custom_system_prompt: String,
}

// -------------------------------------------------------------
// Phase 2: Werkzeuge, Memory, Dateien, Logs — IPC-Erweiterungen
// -------------------------------------------------------------

/// Zusatzereignisse während eines Werkzeug-Streaming-Turns.
///
/// Werden über denselben `chat-stream`-Event ausgeliefert wie
/// [`StreamEvent`], damit der Chatverlauf in der UI ohne Zweitkanal
/// gerendert werden kann. „Text"-Ereignisse verwenden weiterhin
/// [`StreamEvent::Delta`]; das hier ist speziell für die
/// inline-Anzeige der Werkzeugschleife.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolStreamEvent {
    /// Modell hat sich für einen Werkzeugaufruf entschieden.
    ToolCall {
        assistant_message_id: String,
        tool: String,
        arguments_json: String,
    },
    /// Werkzeug hat geantwortet (Text bereits gekürzt und untrusted-markiert).
    ToolResult {
        assistant_message_id: String,
        tool: String,
        content: String,
        is_untrusted: bool,
    },
    /// Werkzeug wurde durch die Policy blockiert oder fehlte.
    ToolError {
        assistant_message_id: String,
        tool: String,
        message: String,
    },
    /// Nutzer muss eine Freigabe erteilen, bevor das Werkzeug ausgeführt wird.
    ///
    /// Der Aufrufer speichert die `pending_id`; die Antwort läuft über
    /// [`ToolPromptResponse`].
    PermissionRequested {
        pending_id: String,
        assistant_message_id: String,
        tool: String,
        arguments_json: String,
        reason: String,
    },
}

/// Nutzerantwort auf einen Freigabedialog (`PermissionRequested`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPromptResponse {
    pub pending_id: String,
    pub allow: bool,
    /// Optional: die Freigabe für die aktuelle Session beibehalten.
    pub remember_for_session: bool,
}

/// Ergebnis der Memory-Suche (Konzept 6.3 + 6.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryRetrieveResponse {
    pub hits: Vec<MemoryHit>,
    pub query: String,
}

/// Manuelles Anlegen oder Aktualisieren eines Faktes über die UI (Konzept 6.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryUpsertRequest {
    /// `None` → neuer Fakt; sonst wird der bestehende Fakt aktualisiert.
    pub id: Option<String>,
    pub text: String,
    pub category: FactCategory,
    pub user_verified: bool,
}

/// Exportformat für den Memory-Bereich (Konzept 6.5: „Export als lesbares JSON").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryExport {
    pub exported_at_unix_ms: i64,
    pub facts: Vec<Fact>,
}

/// Filter für den Logs-Bereich (Konzept 10.2 + UI-Anforderung).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuditFilter {
    /// z. B. „file_read", „file_write" — leer = alle.
    pub actions: Vec<String>,
    /// z. B. „allow", „deny", „prompt".
    pub outcomes: Vec<String>,
    /// Freitext, der mindestens im `reason` oder `target` vorkommen muss.
    pub contains: Option<String>,
    /// Nur Einträge ab diesem Zeitstempel (`None` = ohne Untergrenze).
    pub since_unix_ms: Option<i64>,
    /// Höchstzahl der Ergebnisse; im Default 500, damit die UI schnell bleibt.
    pub limit: Option<u32>,
}

/// Zeile im Logs-Bereich; entspricht einem `pa_vault::audit::AuditRow`,
/// aber ohne Byte-Blob (nur Hex-String, damit JSON leicht bleibt).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntryView {
    pub id: i64,
    pub created_unix_ms: i64,
    pub mode: String,
    pub action: String,
    pub target: Option<String>,
    pub outcome: String,
    pub reason: String,
    pub hash_hex: String,
    pub prev_hash_hex: String,
}

/// Verzeichniseintrag im Dateien-Bereich (Konzept 10.1 M1-M3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    pub name: String,
    pub relative_path: String,
    pub is_directory: bool,
    pub bytes: u64,
    pub modified_unix_ms: Option<i64>,
}

/// Antwort auf `list_workspace`; liefert immer sortierte Einträge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceListing {
    /// Der aufgelöste kanonische Pfad, damit die UI sehen kann, wohin sie
    /// gerade blickt (auch nützlich, wenn ein Symlink im Spiel wäre —
    /// pa-policy blockt den zwar, aber transparent bleibt es trotzdem).
    pub root: String,
    pub relative_path: String,
    pub entries: Vec<WorkspaceEntry>,
}

// -------------------------------------------------------------
// Phase 5: Konnektoren (Exa Research, WhatsApp, Gmail, Offline)
// -------------------------------------------------------------

/// Vollständige Konnektor-Konfiguration inkl. Offline-Kill-Switch.
///
/// `Debug` ist von Hand geschrieben und zeigt den Exa-Schlüssel nie, damit er
/// auch bei versehentlichem `{:?}` nicht in Protokolle gelangt.
///
/// `serde(default)`: Ältere gespeicherte Konfigurationen ohne die neuen Felder laden weiter.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectorConfig {
    /// Wenn aktiv, sind alle externen Netzwerkzugriffe (Exa, Wikipedia, Open-Meteo,
    /// Brave, Gmail) gesperrt.
    pub offline_mode: bool,
    /// Exa-Websuche aktiviert.
    pub exa_enabled: bool,
    /// API-Key für Exa (wird im Vault sicher abgelegt).
    pub exa_api_key: String,
    /// Wikipedia-Konnektor aktiviert (kein Schlüssel nötig).
    pub wikipedia_enabled: bool,
    /// Open-Meteo-Konnektor (Wetter) aktiviert (kein Schlüssel nötig).
    pub open_meteo_enabled: bool,
    /// Brave-Suche aktiviert.
    pub brave_enabled: bool,
    /// API-Schlüssel für Brave (liegt wie der Exa-Schlüssel im Tresor).
    pub brave_api_key: String,
    /// Gmail aktiviert (Zugangsdaten und Einstellungen sind eingerichtet).
    pub gmail_enabled: bool,
    /// Eigenes Gmail-Konto. Das App-Passwort liegt getrennt im Tresor und wird nie zurückgegeben.
    pub gmail_address: String,
    /// Der eine Absender, dem die Auto-Antwort gilt. Mails anderer Absender werden ignoriert.
    pub gmail_target_email: String,
    /// Prüfintervall in Minuten (Standard 5, erlaubt 1 bis 60).
    pub gmail_check_interval_minutes: u32,
    /// Entwurf ablegen oder direkt senden.
    pub gmail_reply_mode: MailReplyMode,
    /// Die Nutzerin oder der Nutzer hat das automatische Senden bewusst bestätigt. Das
    /// Backend lehnt `Send` ohne diese Bestätigung ab.
    pub gmail_send_acknowledged: bool,
    /// Höchstzahl Antworten pro Stunde (Standard 6).
    pub gmail_max_per_hour: u32,
    /// Höchstzahl Antworten pro Tag (Standard 30).
    pub gmail_max_per_day: u32,
    /// Zusatzanweisung an das Modell für die Antworten (Ton, Sprache), höchstens 1000 Zeichen.
    pub gmail_instruction: String,
    /// Chat und Workflows dürfen das Postfach lesen (nur lesend, Mails gelten als privat).
    pub gmail_allow_read: bool,
}

impl std::fmt::Debug for ConnectorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectorConfig")
            .field("offline_mode", &self.offline_mode)
            .field("exa_enabled", &self.exa_enabled)
            .field(
                "exa_api_key",
                &if self.exa_api_key.is_empty() {
                    "<leer>"
                } else {
                    "<geschwärzt>"
                },
            )
            .field("wikipedia_enabled", &self.wikipedia_enabled)
            .field("open_meteo_enabled", &self.open_meteo_enabled)
            .field("brave_enabled", &self.brave_enabled)
            .field(
                "brave_api_key",
                &if self.brave_api_key.is_empty() {
                    "<leer>"
                } else {
                    "<geschwärzt>"
                },
            )
            .field("gmail_enabled", &self.gmail_enabled)
            .finish_non_exhaustive()
    }
}

impl Default for ConnectorConfig {
    fn default() -> Self {
        Self {
            offline_mode: true, // Standardmäßig offline und airgapped!
            exa_enabled: false,
            exa_api_key: String::new(),
            wikipedia_enabled: false,
            open_meteo_enabled: false,
            brave_enabled: false,
            brave_api_key: String::new(),
            gmail_enabled: false,
            gmail_address: String::new(),
            gmail_target_email: String::new(),
            gmail_check_interval_minutes: 5,
            gmail_reply_mode: MailReplyMode::Draft,
            gmail_send_acknowledged: false,
            gmail_max_per_hour: 6,
            gmail_max_per_day: 30,
            gmail_instruction: String::new(),
            gmail_allow_read: false,
        }
    }
}

/// Was mit einer Mail geschieht, auf die IAP antwortet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MailReplyMode {
    /// Die Antwort landet als Entwurf im Gmail-Konto und wird nie gesendet.
    #[default]
    Draft,
    /// Die Antwort wird direkt gesendet (nur nach ausdrücklicher Bestätigung).
    Send,
}

/// Ergebnis einer Websuche (Exa, Wikipedia, Brave).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExaSearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

// -------------------------------------------------------------
// Spuren auf dem Host (Feature „Spurlos beenden“)
// -------------------------------------------------------------

/// Ein Ort auf dem Host-PC, an dem IAP Daten ablegt.
///
/// Die Kennung ist stabil, damit die Oberfläche eine übersetzte Beschriftung
/// wählen kann, ohne Pfade zu deuten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostTrace {
    /// `model_cache`, `vault_hot` oder `legacy_webview`.
    pub id: String,
    pub path: String,
    pub size_bytes: u64,
}

/// Übersicht für den Beenden-Dialog und die Einstellungen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostTraceReport {
    pub traces: Vec<HostTrace>,
    pub total_bytes: u64,
    /// `false`, wenn der Nutzer diesen PC als eigenen markiert hat.
    pub ask_on_exit: bool,
}

// -------------------------------------------------------------
// Projekte (Feature 5) und Dokumente (Feature 1)
// -------------------------------------------------------------

/// Ein Projekt bündelt Unterhaltungen und gibt ihnen eine gemeinsame Grundanweisung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Wird dem Systemprompt jeder Unterhaltung des Projekts vorangestellt.
    pub system_prompt: String,
    pub created_at_unix_ms: i64,
    pub updated_at_unix_ms: i64,
}

/// Ein ins Gedächtnis übernommenes Dokument, dessen Abschnitte im Chat zitiert werden können.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentInfo {
    pub id: String,
    pub name: String,
    /// `txt`, `md`, `docx` oder `pdf`.
    pub kind: String,
    pub size_bytes: u64,
    pub chunk_count: u32,
    pub created_at_unix_ms: i64,
}

/// Eine nummerierte Quelle, auf die sich eine Antwort mit [n] bezieht.
///
/// Name und Auszug werden mitgespeichert, damit die Quelle lesbar bleibt,
/// auch wenn das Dokument später gelöscht wird.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSource {
    pub message_id: String,
    pub number: u32,
    pub document_id: String,
    pub document_name: String,
    pub chunk_ordinal: u32,
    pub excerpt: String,
}

// -------------------------------------------------------------
// PCI (Personal Computer Information): sichtbarer Aktivitäts-Begleiter
// -------------------------------------------------------------

/// Nutzung einer Anwendung, wie sie aus einem Import im Tresor liegt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PciAppUsage {
    pub app: String,
    pub total_ms: i64,
    pub focus_count: u32,
    pub last_seen_unix_ms: i64,
    /// Beispieltitel (z. B. besuchte Seiten), bereits gekürzt.
    pub sample_titles: Vec<String>,
}

/// Ein PC, von dem Aktivität gespeichert wurde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PciComputer {
    pub host: String,
    pub import_count: u32,
    pub total_active_ms: i64,
    pub first_unix_ms: i64,
    pub last_unix_ms: i64,
}

/// Alle Aktivität eines PCs, verdichtet über alle Importe (je Anwendung zusammengezählt).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PciActivity {
    pub host: String,
    pub total_active_ms: i64,
    pub first_unix_ms: i64,
    pub last_unix_ms: i64,
    pub apps: Vec<PciAppUsage>,
}

/// Zustand des PCI-Begleiters für die Oberfläche.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PciStatus {
    /// Auf dieser Plattform gibt es den Begleiter (nur Windows).
    pub supported: bool,
    /// Das Begleitprogramm liegt auf diesem PC (installiert).
    pub installed: bool,
    /// Es läuft gerade (sichtbarer Prozess).
    pub running: bool,
    /// Ungelesenes Journal vorhanden (es gibt etwas zu importieren).
    pub journal_pending: bool,
    /// Aktive Zeit im wartenden Journal in Millisekunden (0, wenn nichts zu importieren ist).
    pub journal_active_ms: i64,
    pub host: String,
}
