//! Kalender- und Aufgaben-Modell + deterministischer Scheduler
//! (Konzept 12).
//!
//! „Terminplanung ist ein Constraint-Problem. Ein 2B-Modell löst so etwas
//! nicht zuverlässig — es erzeugt Pläne, die plausibel klingen und
//! Überschneidungen enthalten." Diese Crate ist die deterministische
//! Rust-Hälfte: sie plant, das LLM zerlegt und erklärt nur.
//!
//! Aufbau:
//! - [`model`] — Datentypen (Termine, Aufgaben, Arbeitszeitfenster, Energie).
//! - [`solver`] — Greedy-Scheduler mit Backtracking bei Konflikten.
//! - [`ics`] — minimaler iCalendar-Import und -Export.
//!
//! Zeiten werden konsequent als Unix-Millisekunden (`i64`) modelliert; die
//! Crate hat bewusst keine `chrono`/`time`-Abhängigkeit, weil beide im
//! Offline-Vendor-Cache aktuell nicht vorhanden sind. Zeitzonen-Anzeige und
//! ICS-Roundtrip formatieren UTC.

pub mod civil;
pub mod feed;
pub mod ics;
pub mod model;
pub mod solver;

use thiserror::Error;

pub use ics::{export_ics, import_ics, IcsError};
pub use model::{
    day_energy_from_load, Availability, Energy, Event, PlanRequest, ScheduledSlot, Task,
    TaskStatus, Weekday,
};
pub use solver::{schedule, SchedulerError, SolvedPlan};

/// Bündel-Fehler für UI-Aufrufer.
#[derive(Debug, Error)]
pub enum SchedulerCrateError {
    #[error("Scheduler: {0}")]
    Solver(#[from] SchedulerError),
    #[error("ICS: {0}")]
    Ics(#[from] IcsError),
}
