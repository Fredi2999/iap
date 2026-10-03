use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Ungültige Anfrage: {0}")]
    InvalidRequest(String),
    #[error("ICS-Parse-Fehler: {0}")]
    IcsParseError(String),
    #[error("Solver-Fehler: {0}")]
    SolverError(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Energy {
    Low,
    Medium,
    High,
}

impl Energy {
    pub fn cost(&self) -> u32 {
        match self {
            Energy::Low => 1,
            Energy::Medium => 2,
            Energy::High => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    InProgress,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Availability {
    pub weekday: Weekday,
    pub start_minute: u32,
    pub end_minute: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub start_unix_ms: i64,
    pub end_unix_ms: i64,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub external_uid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub due_unix_ms: Option<i64>,
    pub duration_minutes: u32,
    pub energy: Energy,
    pub priority: u8,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanRequest {
    pub now_unix_ms: i64,
    pub tz_offset_minutes: i32,
    pub horizon_days: u32,
    pub availabilities: Vec<Availability>,
    pub events: Vec<Event>,
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub break_minutes: Option<u32>,
    #[serde(default)]
    pub day_energy_budget: Option<u32>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnscheduledTask {
    pub task_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolvedPlan {
    pub slots: Vec<ScheduledSlot>,
    pub unscheduled: Vec<UnscheduledTask>,
}
