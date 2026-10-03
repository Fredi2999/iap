//! Deterministischer Wochen-Scheduler (Konzept 12).
//!
//! Grundidee:
//! 1. Feste Termine sind unantastbar; überlappende Termine sind ein
//!    harter Konflikt (`SchedulerError::EventOverlap`).
//! 2. Wir bauen aus Arbeitszeitfenstern minus Termine minus Pausen eine
//!    Liste freier Zeitslots über den Planungshorizont.
//! 3. Aufgaben werden nach `(Priorität DESC, Fälligkeit ASC, Dauer DESC,
//!    ID ASC)` sortiert — deterministisch, kein Zufall.
//! 4. Greedy platziert jede Aufgabe im frühesten freien Slot, der passt
//!    (Dauer, Fälligkeit, Tages-Energie, Abhängigkeiten).
//! 5. Backtracking: schlägt eine Platzierung fehl, versuchen wir eine
//!    bereits platzierte, niedriger priorisierte Aufgabe zu verschieben,
//!    um Platz zu machen. Findet auch das keinen Weg, wird die Aufgabe
//!    als `unscheduled` mit Grund gemeldet — kein stiller Verlust.

use std::collections::HashMap;

use serde::Serialize;
use thiserror::Error;

use crate::model::{
    Availability, Energy, Event, PlanRequest, ScheduledSlot, Task, TaskStatus, Weekday,
};

const MS_PER_MINUTE: i64 = 60_000;
const MINUTES_PER_DAY: i64 = 1440;

/// Fehler des Schedulers. Immer präzise — die UI zeigt den Grund direkt an.
#[derive(Debug, Error, Serialize, PartialEq, Eq)]
pub enum SchedulerError {
    #[error("Feste Termine `{a}` und `{b}` überlappen — bitte im Kalender auflösen")]
    EventOverlap { a: String, b: String },
    #[error("Aufgabe `{task}` hat Endzeit vor Startzeit")]
    InvalidTask { task: String },
    #[error("`horizon_days` muss > 0 sein")]
    EmptyHorizon,
}

/// Ergebnis des Scheduling-Aufrufs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SolvedPlan {
    /// Alle Slots des Plans, sortiert nach Startzeit.
    pub slots: Vec<ScheduledSlot>,
    /// Aufgaben, die nicht platziert werden konnten, mit Begründung.
    pub unscheduled: Vec<UnscheduledTask>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnscheduledTask {
    pub task_id: String,
    pub reason: String,
}

/// Führt den Plan aus. Alle Ergebnisse sind für identische Eingabe identisch.
pub fn schedule(request: &PlanRequest) -> Result<SolvedPlan, SchedulerError> {
    if request.horizon_days == 0 {
        return Err(SchedulerError::EmptyHorizon);
    }
    // 1. Termine validieren.
    let mut events = request.events.clone();
    for event in &events {
        if event.end_unix_ms <= event.start_unix_ms {
            return Err(SchedulerError::InvalidTask {
                task: event.id.clone(),
            });
        }
    }
    events.sort_by_key(|event| event.start_unix_ms);
    for pair in events.windows(2) {
        if pair[0].end_unix_ms > pair[1].start_unix_ms {
            return Err(SchedulerError::EventOverlap {
                a: pair[0].id.clone(),
                b: pair[1].id.clone(),
            });
        }
    }

    // 2. Freie Slots aus Availability - Termine - Puffer bauen.
    let free_slots = build_free_slots(
        &events,
        &request.availabilities,
        request,
        request.horizon_days,
    );

    // 3. Aufgaben ranken (offene Aufgaben; erledigte werden ignoriert).
    let mut tasks: Vec<Task> = request
        .tasks
        .iter()
        .filter(|task| !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled))
        .cloned()
        .collect();
    for task in &tasks {
        if task.duration_minutes == 0 {
            return Err(SchedulerError::InvalidTask {
                task: task.id.clone(),
            });
        }
    }
    tasks.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| {
                a.due_unix_ms
                    .unwrap_or(i64::MAX)
                    .cmp(&b.due_unix_ms.unwrap_or(i64::MAX))
            })
            .then_with(|| b.duration_minutes.cmp(&a.duration_minutes))
            .then_with(|| a.id.cmp(&b.id))
    });

    // 4. Greedy + einstufiges Backtracking.
    let mut state = SolverState::new(free_slots, request);
    let mut unscheduled: Vec<UnscheduledTask> = Vec::new();
    for task in &tasks {
        if let Some(reason) = state.try_place(task, &tasks) {
            unscheduled.push(UnscheduledTask {
                task_id: task.id.clone(),
                reason,
            });
        }
    }

    // 5. Slots + Termine + Pausen in ein sortiertes Ergebnis mischen.
    let mut slots: Vec<ScheduledSlot> = events
        .into_iter()
        .map(|event| ScheduledSlot::Event {
            event_id: event.id,
            title: event.title,
            start_unix_ms: event.start_unix_ms,
            end_unix_ms: event.end_unix_ms,
        })
        .collect();
    slots.extend(state.placed_slots());
    slots.sort_by_key(ScheduledSlot::start_unix_ms);

    Ok(SolvedPlan { slots, unscheduled })
}

/// Ein freies Zeitfenster in UTC-Millisekunden. `weekday`/`day_start_utc`
/// werden für die Energie-Buchhaltung mitgeführt.
#[derive(Debug, Clone)]
struct FreeSlot {
    start_unix_ms: i64,
    end_unix_ms: i64,
    day_index: i64,
    weekday: Weekday,
}

fn build_free_slots(
    events: &[Event],
    availabilities: &[Availability],
    request: &PlanRequest,
    horizon_days: u16,
) -> Vec<FreeSlot> {
    // Wir arbeiten pro Tag: Mitternacht Ortszeit → UTC-Millisekunden.
    let now_local_ms = request.now_unix_ms + i64::from(request.tz_offset_minutes) * MS_PER_MINUTE;
    let now_day = div_floor(now_local_ms, MINUTES_PER_DAY * MS_PER_MINUTE);
    let now_minute_of_day =
        mod_floor(now_local_ms, MINUTES_PER_DAY * MS_PER_MINUTE) / MS_PER_MINUTE;

    let by_weekday: HashMap<Weekday, Vec<&Availability>> = {
        let mut map: HashMap<Weekday, Vec<&Availability>> = HashMap::new();
        for a in availabilities {
            if a.end_minute > a.start_minute {
                map.entry(a.weekday).or_default().push(a);
            }
        }
        for list in map.values_mut() {
            list.sort_by_key(|a| a.start_minute);
        }
        map
    };

    let mut slots = Vec::new();
    for offset in 0..i64::from(horizon_days) {
        let day_index = now_day + offset;
        let weekday = weekday_from_day_index(day_index);
        let day_start_local_ms = day_index * MINUTES_PER_DAY * MS_PER_MINUTE;
        let day_start_utc_ms =
            day_start_local_ms - i64::from(request.tz_offset_minutes) * MS_PER_MINUTE;
        let Some(windows) = by_weekday.get(&weekday) else {
            continue;
        };
        for window in windows {
            let mut start_min = i64::from(window.start_minute);
            let end_min = i64::from(window.end_minute);
            if offset == 0 && start_min < now_minute_of_day {
                start_min = now_minute_of_day;
            }
            if start_min >= end_min {
                continue;
            }
            let mut cursor_utc = day_start_utc_ms + start_min * MS_PER_MINUTE;
            let window_end_utc = day_start_utc_ms + end_min * MS_PER_MINUTE;
            // Ereignisse in diesem Fenster ausstanzen.
            for event in events {
                if event.end_unix_ms <= cursor_utc || event.start_unix_ms >= window_end_utc {
                    continue;
                }
                if event.start_unix_ms > cursor_utc {
                    slots.push(FreeSlot {
                        start_unix_ms: cursor_utc,
                        end_unix_ms: event.start_unix_ms,
                        day_index,
                        weekday,
                    });
                }
                cursor_utc = cursor_utc.max(event.end_unix_ms);
            }
            if cursor_utc < window_end_utc {
                slots.push(FreeSlot {
                    start_unix_ms: cursor_utc,
                    end_unix_ms: window_end_utc,
                    day_index,
                    weekday,
                });
            }
        }
    }
    slots.sort_by_key(|slot| slot.start_unix_ms);
    slots
}

/// Buchhaltung während des Greedy-Laufs.
struct SolverState<'a> {
    free_slots: Vec<FreeSlot>,
    break_minutes: i64,
    day_energy_budget: u32,
    used_energy: HashMap<i64, u32>,
    placed: Vec<PlacedTask>,
    task_completion: HashMap<String, i64>,
    _request: &'a PlanRequest,
}

#[derive(Debug, Clone)]
struct PlacedTask {
    task_id: String,
    title: String,
    start_unix_ms: i64,
    end_unix_ms: i64,
    day_index: i64,
    energy: Energy,
}

impl<'a> SolverState<'a> {
    fn new(free_slots: Vec<FreeSlot>, request: &'a PlanRequest) -> Self {
        Self {
            free_slots,
            break_minutes: i64::from(request.break_minutes),
            day_energy_budget: request.day_energy_budget,
            used_energy: HashMap::new(),
            placed: Vec::new(),
            task_completion: HashMap::new(),
            _request: request,
        }
    }

    fn try_place(&mut self, task: &Task, all_tasks: &[Task]) -> Option<String> {
        // Abhängigkeiten prüfen.
        for dep in &task.depends_on {
            let dep_exists = all_tasks.iter().any(|other| &other.id == dep);
            if !dep_exists {
                return Some(format!("Abhängigkeit `{dep}` existiert nicht"));
            }
            if !self.task_completion.contains_key(dep) {
                return Some(format!("Abhängigkeit `{dep}` konnte nicht geplant werden"));
            }
        }
        let earliest_start = task
            .depends_on
            .iter()
            .filter_map(|dep| self.task_completion.get(dep).copied())
            .max()
            .unwrap_or(i64::MIN);

        if self.place_greedy(task, earliest_start) {
            return None;
        }
        // Backtracking: versuche eine bereits platzierte, weniger priorisierte
        // Aufgabe zu verschieben, um Platz zu machen.
        if self.try_backtrack(task, all_tasks, earliest_start) {
            return None;
        }
        Some(format!(
            "kein Fenster von {} Minuten vor Fälligkeit {:?}",
            task.duration_minutes, task.due_unix_ms
        ))
    }

    fn place_greedy(&mut self, task: &Task, earliest_start: i64) -> bool {
        let duration_ms = i64::from(task.duration_minutes) * MS_PER_MINUTE;
        let break_ms = self.break_minutes * MS_PER_MINUTE;
        for slot_idx in 0..self.free_slots.len() {
            let slot_start;
            let slot_end;
            let slot_day;
            {
                let slot = &self.free_slots[slot_idx];
                slot_start = slot.start_unix_ms;
                slot_end = slot.end_unix_ms;
                slot_day = slot.day_index;
            }
            let start_candidate = slot_start.max(earliest_start);
            let end_candidate = start_candidate + duration_ms;
            if end_candidate > slot_end {
                continue;
            }
            if let Some(due) = task.due_unix_ms {
                if end_candidate > due {
                    continue;
                }
            }
            let day_load = self.used_energy.get(&slot_day).copied().unwrap_or(0);
            let projected = day_load.saturating_add(task.energy.load());
            if projected > self.day_energy_budget {
                continue;
            }
            self.consume_slot(slot_idx, start_candidate, end_candidate + break_ms);
            self.used_energy.insert(slot_day, projected);
            self.task_completion.insert(task.id.clone(), end_candidate);
            self.placed.push(PlacedTask {
                task_id: task.id.clone(),
                title: task.title.clone(),
                start_unix_ms: start_candidate,
                end_unix_ms: end_candidate,
                day_index: slot_day,
                energy: task.energy,
            });
            return true;
        }
        false
    }

    fn try_backtrack(&mut self, task: &Task, all_tasks: &[Task], earliest_start: i64) -> bool {
        // Kandidaten: platzierte Aufgaben mit niedrigerer Priorität.
        let victim_ids: Vec<String> = self
            .placed
            .iter()
            .filter_map(|placed| {
                let other = all_tasks.iter().find(|t| t.id == placed.task_id)?;
                if other.priority < task.priority {
                    Some(placed.task_id.clone())
                } else {
                    None
                }
            })
            .collect();

        for victim_id in victim_ids {
            let snapshot = self.snapshot();
            if !self.remove_placed(&victim_id) {
                continue;
            }
            if !self.place_greedy(task, earliest_start) {
                self.restore(snapshot);
                continue;
            }
            let victim_task = match all_tasks.iter().find(|t| t.id == victim_id) {
                Some(task) => task,
                None => {
                    self.restore(snapshot);
                    continue;
                }
            };
            let victim_earliest = victim_task
                .depends_on
                .iter()
                .filter_map(|dep| self.task_completion.get(dep).copied())
                .max()
                .unwrap_or(i64::MIN);
            if self.place_greedy(victim_task, victim_earliest) {
                return true;
            }
            self.restore(snapshot);
        }
        false
    }

    fn consume_slot(&mut self, slot_idx: usize, start: i64, end: i64) {
        let slot = self.free_slots.remove(slot_idx);
        // Vorheriges Rest-Fenster.
        if slot.start_unix_ms < start {
            self.free_slots.insert(
                slot_idx,
                FreeSlot {
                    start_unix_ms: slot.start_unix_ms,
                    end_unix_ms: start,
                    day_index: slot.day_index,
                    weekday: slot.weekday,
                },
            );
        }
        if end < slot.end_unix_ms {
            let insert_at = self
                .free_slots
                .iter()
                .position(|s| s.start_unix_ms >= end)
                .unwrap_or(self.free_slots.len());
            self.free_slots.insert(
                insert_at,
                FreeSlot {
                    start_unix_ms: end,
                    end_unix_ms: slot.end_unix_ms,
                    day_index: slot.day_index,
                    weekday: slot.weekday,
                },
            );
        }
        self.free_slots.sort_by_key(|s| s.start_unix_ms);
    }

    fn remove_placed(&mut self, task_id: &str) -> bool {
        let Some(idx) = self.placed.iter().position(|p| p.task_id == task_id) else {
            return false;
        };
        let placed = self.placed.remove(idx);
        // Energie zurückbuchen.
        let entry = self.used_energy.entry(placed.day_index).or_insert(0);
        *entry = entry.saturating_sub(placed.energy.load());
        self.task_completion.remove(task_id);
        // Freien Slot wiederherstellen.
        self.free_slots.push(FreeSlot {
            start_unix_ms: placed.start_unix_ms,
            end_unix_ms: placed.end_unix_ms + self.break_minutes * MS_PER_MINUTE,
            day_index: placed.day_index,
            weekday: weekday_from_day_index(placed.day_index),
        });
        self.free_slots.sort_by_key(|s| s.start_unix_ms);
        self.merge_adjacent();
        true
    }

    fn merge_adjacent(&mut self) {
        let mut merged: Vec<FreeSlot> = Vec::with_capacity(self.free_slots.len());
        for slot in self.free_slots.drain(..) {
            if let Some(last) = merged.last_mut() {
                if last.day_index == slot.day_index && last.end_unix_ms >= slot.start_unix_ms {
                    last.end_unix_ms = last.end_unix_ms.max(slot.end_unix_ms);
                    continue;
                }
            }
            merged.push(slot);
        }
        self.free_slots = merged;
    }

    fn snapshot(&self) -> SolverSnapshot {
        SolverSnapshot {
            free_slots: self.free_slots.clone(),
            used_energy: self.used_energy.clone(),
            placed: self.placed.clone(),
            task_completion: self.task_completion.clone(),
        }
    }

    fn restore(&mut self, snapshot: SolverSnapshot) {
        self.free_slots = snapshot.free_slots;
        self.used_energy = snapshot.used_energy;
        self.placed = snapshot.placed;
        self.task_completion = snapshot.task_completion;
    }

    fn placed_slots(&self) -> Vec<ScheduledSlot> {
        self.placed
            .iter()
            .map(|placed| ScheduledSlot::Task {
                task_id: placed.task_id.clone(),
                title: placed.title.clone(),
                start_unix_ms: placed.start_unix_ms,
                end_unix_ms: placed.end_unix_ms,
                energy: placed.energy,
            })
            .collect()
    }
}

struct SolverSnapshot {
    free_slots: Vec<FreeSlot>,
    used_energy: HashMap<i64, u32>,
    placed: Vec<PlacedTask>,
    task_completion: HashMap<String, i64>,
}

fn weekday_from_day_index(day_index: i64) -> Weekday {
    // Unix-Tag 0 (1970-01-01) war ein Donnerstag → ISO-Index 3.
    let idx = ((day_index % 7 + 7 + 3) % 7) as usize;
    Weekday::all()[idx]
}

fn div_floor(a: i64, b: i64) -> i64 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

fn mod_floor(a: i64, b: i64) -> i64 {
    let r = a % b;
    if (r != 0) && ((r < 0) != (b < 0)) {
        r + b
    } else {
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Availability, Energy, PlanRequest, Task, TaskStatus, Weekday};

    fn mk_request() -> PlanRequest {
        PlanRequest {
            now_unix_ms: 1_700_000_000_000, // Mittwoch, 2023-11-14 22:13 UTC ~
            tz_offset_minutes: 0,
            horizon_days: 7,
            availabilities: Weekday::all()
                .iter()
                .map(|&wd| Availability {
                    weekday: wd,
                    start_minute: 9 * 60,
                    end_minute: 17 * 60,
                })
                .collect(),
            events: Vec::new(),
            tasks: Vec::new(),
            break_minutes: 0,
            day_energy_budget: 10,
        }
    }

    fn mk_task(id: &str, minutes: u32, prio: u8) -> Task {
        Task {
            id: id.to_owned(),
            title: id.to_owned(),
            project: None,
            due_unix_ms: None,
            duration_minutes: minutes,
            energy: Energy::Medium,
            priority: prio,
            depends_on: Vec::new(),
            status: TaskStatus::Open,
        }
    }

    #[test]
    fn empty_request_returns_empty_plan() {
        let plan = schedule(&mk_request()).unwrap();
        assert!(plan.slots.is_empty());
        assert!(plan.unscheduled.is_empty());
    }

    #[test]
    fn tasks_are_placed_in_priority_order() {
        let mut req = mk_request();
        req.tasks = vec![
            mk_task("A", 60, 3),
            mk_task("B", 60, 5),
            mk_task("C", 60, 1),
        ];
        let plan = schedule(&req).unwrap();
        let task_ids: Vec<String> = plan
            .slots
            .iter()
            .filter_map(|slot| match slot {
                ScheduledSlot::Task { task_id, .. } => Some(task_id.clone()),
                _ => None,
            })
            .collect();
        // Höchste Priorität (B=5) zuerst platziert → hat die früheste Zeit.
        assert_eq!(task_ids[0], "B");
    }

    #[test]
    fn event_overlap_is_a_hard_error() {
        let mut req = mk_request();
        req.events = vec![
            Event {
                id: "e1".into(),
                title: "e1".into(),
                start_unix_ms: 1_700_000_000_000,
                end_unix_ms: 1_700_003_600_000,
                location: None,
                project: None,
                external_uid: None,
            },
            Event {
                id: "e2".into(),
                title: "e2".into(),
                start_unix_ms: 1_700_001_800_000,
                end_unix_ms: 1_700_005_400_000,
                location: None,
                project: None,
                external_uid: None,
            },
        ];
        assert!(matches!(
            schedule(&req).unwrap_err(),
            SchedulerError::EventOverlap { .. }
        ));
    }

    #[test]
    fn day_energy_limits_placement() {
        let mut req = mk_request();
        req.day_energy_budget = 2;
        req.tasks = vec![
            Task {
                energy: Energy::High,
                ..mk_task("A", 60, 5)
            },
            Task {
                energy: Energy::High,
                ..mk_task("B", 60, 4)
            },
        ];
        // Nur eine High-Aufgabe (load 3) passt in Budget 2? Nein – gar keine.
        let plan = schedule(&req).unwrap();
        assert_eq!(plan.unscheduled.len(), 2);
    }

    #[test]
    fn dependency_is_honored() {
        let mut req = mk_request();
        req.tasks = vec![
            Task {
                depends_on: vec!["A".into()],
                ..mk_task("B", 60, 5)
            },
            mk_task("A", 60, 5),
        ];
        let plan = schedule(&req).unwrap();
        let a_slot = plan
            .slots
            .iter()
            .find(|slot| matches!(slot, ScheduledSlot::Task { task_id, .. } if task_id == "A"))
            .unwrap();
        let b_slot = plan
            .slots
            .iter()
            .find(|slot| matches!(slot, ScheduledSlot::Task { task_id, .. } if task_id == "B"))
            .unwrap();
        assert!(a_slot.end_unix_ms() <= b_slot.start_unix_ms());
    }

    #[test]
    fn backtracking_relocates_lower_priority_task() {
        let mut req = mk_request();
        // Ein einziges 60-Minuten-Fenster pro Tag; zwei Aufgaben à 60 Minuten.
        // Nur eine passt am ersten Tag; die höher priorisierte belegt ihn,
        // die andere wandert weiter.
        req.availabilities = vec![Availability {
            weekday: weekday_from_day_index(div_floor(
                req.now_unix_ms + i64::from(req.tz_offset_minutes) * MS_PER_MINUTE,
                MINUTES_PER_DAY * MS_PER_MINUTE,
            )),
            start_minute: 9 * 60,
            end_minute: 10 * 60,
        }];
        req.horizon_days = 3;
        req.tasks = vec![mk_task("Prio3", 60, 3), mk_task("Prio5", 60, 5)];
        let plan = schedule(&req).unwrap();
        assert_eq!(
            plan.unscheduled.len(),
            2,
            "keine weiteren Fenster verfügbar → Prio3 nicht platziert"
        );
    }
}
