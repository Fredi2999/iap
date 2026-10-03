//! Verträge für den kleinen Workflow-Builder (Flow Version, Bereich „Workflows“).
//!
//! Die Knotenarten sind fest und typisiert. Es gibt bewusst keinen frei
//! programmierbaren Knoten (JavaScript/Python) in dieser Version.

use serde::{Deserialize, Serialize};

/// Version des Graph-Formats; steigt bei inkompatiblen Änderungen.
pub const WORKFLOW_SCHEMA_VERSION: u32 = 1;

/// Datenart auf einem Anschluss. Verbindungen sind nur zwischen gleichen
/// Datenarten erlaubt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataKind {
    /// Reiner Startimpuls ohne Daten.
    Signal,
    /// Öffentlicher Text (Suchfrage, Modellantwort, Ergebnistext).
    Text,
    /// Suchtreffer mit Quellen (optional mit Seiteninhalt).
    Results,
    /// Ergebnis einer Prüfung (belegt / Lücken).
    Verdict,
}

/// Welcher Web-Konnektor (neben Exa) eine Suche beantwortet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebService {
    /// Wikipedia (Sprache aus dem Knoten).
    Wikipedia,
    /// Brave Search.
    Brave,
}

/// Regel einer Verzweigung. Beide Regeln liefern genau ein Ja oder Nein.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BranchRule {
    /// Ja, wenn der Text die Zeichenfolge enthält (ohne Beachtung der Groß-/Kleinschreibung).
    Contains { text: String },
    /// Ja, wenn das lokale Modell die Frage zum Text mit Ja beantwortet.
    ModelYesNo { question: String },
}

/// Wohin ein Vorschlag aus „Ablegen“ gehören würde. Geschrieben wird erst nach
/// Bestätigung durch den Nutzer; der Knoten selbst verändert nichts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoreTarget {
    /// Eine Notiz im Gedächtnis.
    Memory,
    /// Eine neue Aufgabe im Kalender.
    Task,
    /// Eine Datei im Arbeitsordner (mit Diff zur Bestätigung).
    File { relative_path: String },
}

/// Die festen Knotenarten.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeKind {
    /// Manueller Start durch den Nutzer.
    ManualStart,
    /// Sichtbar öffentlich eingegebene Suchfrage.
    Input { text: String },
    /// Exa-Suche (`POST /search`).
    ExaSearch { num_results: u32 },
    /// Exa-Seiteninhalte (`POST /contents`) für die Treffer.
    ExaContents { max_characters: u32 },
    /// Lokales Modell in getrenntem Kontext (ohne Chat, Memory, Dateien).
    LocalModel { instruction: String },
    /// Prüfung der Treffer gegen sichtbare Kriterien.
    Check { criteria: Vec<String> },
    /// Bedingung mit begrenzter Wiederholung: bei Lücken zurück zur Suche.
    Condition { max_iterations: u32 },
    /// Ergebnis mit Quellen und offenen Punkten.
    Output,
    /// Text, den der Nutzer beim Start eingibt (statt eines festen Textes im Ablauf).
    /// `public`: der Text darf als öffentliche Suchfrage zu Exa gehen; sonst bleibt er privat.
    RuntimeInput { label: String, public: bool },
    /// Termine und Aufgaben der nächsten Tage als Text (privat).
    Calendar {
        days_ahead: u32,
        include_tasks: bool,
    },
    /// Sucht im Gedächtnis (privat).
    MemorySearch { max_hits: u32 },
    /// Wendet einen Anleitungs-Skill (SKILL.md) mit dem lokalen Modell auf den Text an.
    Skill { skill_id: String },
    /// Verzweigt in „ja“ und „nein“.
    Branch { rule: BranchRule },
    /// Nimmt den Text, der aus einem der verbundenen Zweige ankommt.
    Join,
    /// Setzt bis zu drei Texte in eine Vorlage mit `{{a}}`, `{{b}}`, `{{c}}` ein.
    Merge { template: String },
    /// Zeigt am Ende des Laufs einen Hinweis.
    Notify { title: String },
    /// Legt einen Vorschlag zum Ablegen ins Ergebnis; geschrieben wird erst nach Bestätigung.
    Store { target: StoreTarget },
    /// Wikipedia-Suche (kostenlos, ohne Schlüssel); `lang` ist `de` oder `en`.
    WikipediaSearch { num_results: u32, lang: String },
    /// Brave-Suche (eigener Schlüssel).
    BraveSearch { num_results: u32 },
    /// Wetter für einen öffentlich eingegebenen Ort (Open-Meteo, kostenlos).
    Weather { days: u32 },
    /// Sucht im eigenen Gmail-Postfach (nur lesend). Der Suchfilter steht fest im Knoten und
    /// kommt nie aus Laufdaten; das Ergebnis ist privat und geht nie an externe Dienste.
    MailSearch {
        from: String,
        subject: String,
        unread_only: bool,
        limit: u32,
    },
}

/// Ein Knoten auf der Zeichenfläche.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    pub kind: NodeKind,
    pub x: f64,
    pub y: f64,
}

/// Eine Verbindung zwischen zwei benannten Anschlüssen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub id: String,
    pub from: String,
    pub from_port: String,
    pub to: String,
    pub to_port: String,
}

/// Der gesamte Graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowGraph {
    pub version: u32,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
}

/// Gespeicherte Definition im Vault.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: String,
    pub name: String,
    pub graph: WorkflowGraph,
    pub updated_unix_ms: i64,
}

/// Ein Befund der Vorabprüfung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphProblem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<String>,
    pub message: String,
}

/// Ergebnis der Vorabprüfung; ein Lauf startet nur bei `ok`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowValidation {
    pub ok: bool,
    pub problems: Vec<GraphProblem>,
}

/// Sichtbare Höchstwerte eines Laufs. Ein ausgeschöpftes Budget beendet den
/// Lauf ehrlich; es gibt keine Endlosschleife.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RunBudget {
    pub max_searches: u32,
    pub max_iterations: u32,
    pub max_seconds: u64,
    pub max_tokens: u32,
    /// Höchste erlaubte Exa-Ausgabe in US-Dollar laut `costDollars.total`.
    pub max_cost_usd: f64,
}

/// Bisheriger Verbrauch eines Laufs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct RunUsage {
    pub searches: u32,
    pub iterations: u32,
    pub seconds: f64,
    pub tokens: u32,
    pub cost_usd: f64,
}

/// Endzustand beziehungsweise Zwischenzustand eines Laufs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    /// Kriterien belegt.
    Finished,
    /// Kriterien innerhalb der Grenzen nicht erreicht: „nicht ausreichend belegt“.
    NotSufficientlySupported,
    BudgetExhausted,
    Cancelled,
    Failed,
}

/// Art eines Protokolleintrags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    Started,
    NodeStarted,
    NodeFinished,
    /// Ein Datenübergang zwischen zwei Knoten.
    DataFlow,
    BudgetUpdate,
    /// Ein Hinweis aus einem „Hinweis“-Knoten.
    Notice,
    /// Die Policy hat einen Schritt verweigert.
    Denied,
    Error,
    Finished,
}

/// Ein Eintrag im Laufprotokoll (ohne Geheimnisse).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEvent {
    pub seq: u32,
    pub at_unix_ms: i64,
    pub kind: RunEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub message: String,
    /// Herkunft der übergebenen Daten (`user_public`, `web_content`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// Eine Quelle im Ergebnis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSource {
    pub title: String,
    pub url: String,
}

/// Ein Vorschlag aus einem „Ablegen“-Knoten. Der Lauf schreibt nie selbst.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowProposal {
    pub node_id: String,
    pub target: StoreTarget,
    pub text: String,
}

/// Ergebnis mit Quellen und offenen Punkten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowResult {
    pub answer: String,
    pub sources: Vec<WorkflowSource>,
    pub open_points: Vec<String>,
    /// Vorschläge zum Ablegen; erst nach Bestätigung des Nutzers wirksam.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<WorkflowProposal>,
}

/// Zusammenfassung eines Laufs für die Oberfläche.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRunReport {
    pub run_id: String,
    pub workflow_id: String,
    pub status: RunStatus,
    pub usage: RunUsage,
    pub budget: RunBudget,
    pub started_unix_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_unix_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<WorkflowResult>,
    pub events: Vec<RunEvent>,
}

/// Anfrage zum Start eines Laufs. `exa_approved` ist die ausdrückliche
/// Freigabe genau dieses Laufs; sie wird nie gespeichert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StartRunRequest {
    pub workflow_id: String,
    pub budget: RunBudget,
    pub exa_approved: bool,
    /// Texte für „Eingabe beim Start“-Knoten, nach Knoten-ID.
    #[serde(default)]
    pub inputs: std::collections::HashMap<String, String>,
    /// Abweichung der lokalen Zeit von UTC in Minuten, damit der Kalender-Baustein
    /// Uhrzeiten so schreibt, wie der Nutzer sie sieht.
    #[serde(default)]
    pub tz_offset_minutes: i32,
}

/// Zustand der Exa-Anbindung für die Oberfläche (ohne Schlüssel).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExaStatus {
    pub has_key: bool,
    pub enabled: bool,
    pub air_gap: bool,
}
