//! Datentypen für Kalender, Aufgaben und Planungsanforderungen.
//!
//! Alle Zeitpunkte sind Unix-Millisekunden (`i64`); Dauern sind Minuten
//! (`u32`). Der Scheduler arbeitet ausschließlich auf ganzen Minuten,
//! damit Vergleiche und Backtracking deterministisch und schnell bleiben.

use serde::{Deserialize, Serialize};

/// Wochentag, 0-basiert Montag..Sonntag (ISO).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    /// ISO-Index (Mo=0 .. So=6). Nützlich für Tages-Arrays.
    pub fn index(self) -> usize {
        match self {
            Weekday::Monday => 0,
            Weekday::Tuesday => 1,
            Weekday::Wednesday => 2,
            Weekday::Thursday => 3,
            Weekday::Friday => 4,
            Weekday::Saturday => 5,
            Weekday::Sunday => 6,
        }
    }

    pub fn all() -> [Weekday; 7] {
        [
            Weekday::Monday,
            Weekday::Tuesday,
            Weekday::Wednesday,
            Weekday::Thursday,
            Weekday::Friday,
            Weekday::Saturday,
            Weekday::Sunday,
        ]
    }
}

/// Grober Energiebedarf einer Aufgabe. Der Scheduler behandelt sie als
/// weichen Kontingent-Wert pro Tag (siehe [`day_energy_from_load`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Energy {
    Low,
    Medium,
    High,
}

impl Energy {
    pub fn load(self) -> u32 {
        match self {
            Energy::Low => 1,
            Energy::Medium => 2,
            Energy::High => 3,
        }
    }
}

/// Kurzform: Tages-Energiebudget aus einer Ganzzahl (0..=10). Wird in der
/// UI meist über einen Regler bedient.
pub fn day_energy_from_load(load: u32) -> u32 {
    load.clamp(1, 10)
}

/// Feste Termine — sie werden **niemals** durch den Scheduler verschoben.
/// Kollidieren zwei Termine miteinander, meldet der Scheduler das als
/// harten Fehler, statt still einen Vorschlag zu machen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub title: String,
    /// Startzeit (unix ms, UTC).
    pub start_unix_ms: i64,
    /// Endzeit (unix ms, UTC), exklusiv. Muss `> start_unix_ms` sein.
    pub end_unix_ms: i64,
    /// Freitextort, wird nicht ausgewertet, nur angezeigt.
    #[serde(default)]
    pub location: Option<String>,
    /// Kontext-/Projektzuordnung; hilft beim Bündeln.
    #[serde(default)]
    pub project: Option<String>,
    /// ICS-UID, wenn der Termin per Import gekommen ist.
    #[serde(default)]
    pub external_uid: Option<String>,
}

impl Event {
    pub fn duration_minutes(&self) -> u32 {
        let diff_ms = (self.end_unix_ms - self.start_unix_ms).max(0);
        (diff_ms / 60_000).min(i64::from(u32::MAX)) as u32
    }
}

/// Status einer Aufgabe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    InProgress,
    Done,
    Cancelled,
}

/// Aufgabe (Konzept 12): „Titel, Projekt, Fälligkeit, geschätzte Dauer,
/// Energiebedarf, Priorität, Abhängigkeiten, Status."
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub project: Option<String>,
    /// Fälligkeit (unix ms). Aufgaben ohne Fälligkeit haben `None`.
    #[serde(default)]
    pub due_unix_ms: Option<i64>,
    /// Geschätzte Bruttodauer in Minuten (inkl. Pufferbedarf).
    pub duration_minutes: u32,
    pub energy: Energy,
    /// Priorität 1..5; höhere Zahl = wichtiger. Deterministischer Tiebreaker.
    pub priority: u8,
    /// IDs anderer Aufgaben, die zuerst fertig sein müssen.
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default = "task_status_default")]
    pub status: TaskStatus,
}

fn task_status_default() -> TaskStatus {
    TaskStatus::Open
}

/// Ergebnisslot des Schedulers: entweder ein fester Termin (kopiert aus
/// [`Event`]) oder ein Task-Vorschlag mit konkreten Anfangs-/Endzeiten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduledSlot {
    Event {
        event_id: String,
        title: String,
        start_unix_ms: i64,
        end_unix_ms: i64,
    },
    Task {
        task_id: String,
        title: String,
        start_unix_ms: i64,
        end_unix_ms: i64,
        energy: Energy,
    },
    Break {
        start_unix_ms: i64,
        end_unix_ms: i64,
    },
}

impl ScheduledSlot {
    pub fn start_unix_ms(&self) -> i64 {
        match self {
            ScheduledSlot::Event { start_unix_ms, .. } => *start_unix_ms,
            ScheduledSlot::Task { start_unix_ms, .. } => *start_unix_ms,
            ScheduledSlot::Break { start_unix_ms, .. } => *start_unix_ms,
        }
    }

    pub fn end_unix_ms(&self) -> i64 {
        match self {
            ScheduledSlot::Event { end_unix_ms, .. } => *end_unix_ms,
            ScheduledSlot::Task { end_unix_ms, .. } => *end_unix_ms,
            ScheduledSlot::Break { end_unix_ms, .. } => *end_unix_ms,
        }
    }
}

/// Ein Arbeitszeitfenster pro Wochentag, in Minuten seit Mitternacht Ortszeit.
///
/// Ortszeit-Umrechnung: der Scheduler bekommt die planbaren Fenster in
/// UTC-Minuten pro Tag vom Aufrufer schon aufgelöst. Das hält die Crate frei
/// von Zeitzonen-Bibliotheken (siehe Modul-Doc in `lib.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Availability {
    pub weekday: Weekday,
    /// Startminute im Tag (0..1440).
    pub start_minute: u16,
    /// Endminute im Tag (Start..1440). Exklusiv.
    pub end_minute: u16,
}

impl Availability {
    pub fn contains(&self, minute_of_day: u16) -> bool {
        minute_of_day >= self.start_minute && minute_of_day < self.end_minute
    }
}

/// Was der Aufrufer dem Scheduler übergibt.
///
/// `now_unix_ms` bildet die untere Zeitgrenze; nichts wird in die
/// Vergangenheit gelegt. `horizon_days` beschränkt die Suche pragmatisch;
/// 7–14 sind übliche Werte (Wochenplanung).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanRequest {
    pub now_unix_ms: i64,
    /// Offset in Minuten zwischen UTC und Ortszeit (positiv = östlich; z. B.
    /// Wien im Winter = +60). Wird zur Umrechnung der `Availability`-Fenster
    /// auf UTC verwendet.
    pub tz_offset_minutes: i32,
    pub horizon_days: u16,
    pub availabilities: Vec<Availability>,
    pub events: Vec<Event>,
    pub tasks: Vec<Task>,
    /// Mindestpause zwischen aufeinander folgenden Slots (Minuten).
    #[serde(default = "default_break_minutes")]
    pub break_minutes: u16,
    /// Tages-Energiebudget (Summe der `Energy::load` pro Tag).
    #[serde(default = "default_day_energy")]
    pub day_energy_budget: u32,
}

fn default_break_minutes() -> u16 {
    10
}

fn default_day_energy() -> u32 {
    day_energy_from_load(6)
}
