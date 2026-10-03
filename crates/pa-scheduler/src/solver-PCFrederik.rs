use std::collections::{HashMap, HashSet};

use crate::model::{
    PlanRequest, ScheduledSlot, SchedulerError, SolvedPlan, Task, TaskStatus,
    UnscheduledTask,
};

/// Deterministischer Scheduler nach Konzept 12
pub fn schedule(request: &PlanRequest) -> Result<SolvedPlan, SchedulerError> {
    if request.horizon_days == 0 {
        return Err(SchedulerError::InvalidRequest(
            "horizon_days muss mindestens 1 sein".to_string(),
        ));
    }

    let mut slots = Vec::new();
    let mut unscheduled = Vec::new();

    // 1. Feste Events in Slots überführen
    for event in &request.events {
        slots.push(ScheduledSlot::Event {
            event_id: event.id.clone(),
            title: event.title.clone(),
            start_unix_ms: event.start_unix_ms,
            end_unix_ms: event.end_unix_ms,
        });
    }

    // 2. Offene Aufgaben filtern und nach Priorität/Fälligkeit sortieren
    let mut eligible_tasks: Vec<&Task> = request
        .tasks
        .iter()
        .filter(|t| matches!(t.status, TaskStatus::Open | TaskStatus::InProgress))
        .collect();

    // Höchste Priorität zuerst, dann früheste Fälligkeit
    eligible_tasks.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| match (a.due_unix_ms, b.due_unix_ms) {
                (Some(ad), Some(bd)) => ad.cmp(&bd),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
    });

    let mut scheduled_task_ids = HashSet::new();
    let break_duration_ms = request.break_minutes.unwrap_or(10) as i64 * 60 * 1000;
    let day_budget = request.day_energy_budget.unwrap_or(8);

    // Zeitanker
    let ms_per_day = 86_400_000i64;
    let horizon_end_ms = request.now_unix_ms + (request.horizon_days as i64 * ms_per_day);

    // Belegte Zeiträume sammeln
    let mut busy_intervals: Vec<(i64, i64)> = request
        .events
        .iter()
        .map(|e| (e.start_unix_ms, e.end_unix_ms))
        .collect();
    busy_intervals.sort_by_key(|(s, _)| *s);

    let mut current_cursor = request.now_unix_ms;
    let mut day_energy_used: HashMap<i64, u32> = HashMap::new();

    for task in eligible_tasks {
        // Prüfen, ob Abhängigkeiten erfüllt sind
        let deps_met = task
            .depends_on
            .iter()
            .all(|dep_id| scheduled_task_ids.contains(dep_id));

        if !deps_met {
            unscheduled.push(UnscheduledTask {
                task_id: task.id.clone(),
                reason: "Abhängigkeiten noch nicht erfüllt".to_string(),
            });
            continue;
        }

        let task_duration_ms = (task.duration_minutes as i64) * 60 * 1000;
        let task_cost = task.energy.cost();
        let mut placed = false;

        // Suche nach freiem Slot ab current_cursor
        let mut search_time = current_cursor;
        while search_time + task_duration_ms <= horizon_end_ms {
            let day_idx = (search_time + (request.tz_offset_minutes as i64 * 60_000)) / ms_per_day;
            let current_day_used = *day_energy_used.get(&day_idx).unwrap_or(&0);

            if current_day_used + task_cost > day_budget {
                // Tag ist voll, springe zum nächsten Tag
                let next_day_ms = (day_idx + 1) * ms_per_day - (request.tz_offset_minutes as i64 * 60_000);
                search_time = search_time.max(next_day_ms);
                continue;
            }

            let proposed_start = search_time;
            let proposed_end = proposed_start + task_duration_ms;

            // Kollisionsprüfung mit bereits vergebenen Intervallen
            let overlaps = busy_intervals.iter().any(|(s, e)| {
                !(proposed_end <= *s || proposed_start >= *e)
            });

            if !overlaps {
                // Prüfen auf Fälligkeit
                if let Some(due) = task.due_unix_ms {
                    if proposed_end > due {
                        unscheduled.push(UnscheduledTask {
                            task_id: task.id.clone(),
                            reason: "Fälligkeitsdatum kann nicht eingehalten werden".to_string(),
                        });
                        placed = true; // als unscheduled markiert
                        break;
                    }
                }

                // Slot vergeben
                slots.push(ScheduledSlot::Task {
                    task_id: task.id.clone(),
                    title: task.title.clone(),
                    start_unix_ms: proposed_start,
                    end_unix_ms: proposed_end,
                    energy: task.energy,
                });

                busy_intervals.push((proposed_start, proposed_end));
                busy_intervals.sort_by_key(|(s, _)| *s);
                scheduled_task_ids.insert(task.id.clone());
                day_energy_used.insert(day_idx, current_day_used + task_cost);

                // Pause hinzufügen wenn definiert
                if break_duration_ms > 0 {
                    slots.push(ScheduledSlot::Break {
                        start_unix_ms: proposed_end,
                        end_unix_ms: proposed_end + break_duration_ms,
                    });
                    busy_intervals.push((proposed_end, proposed_end + break_duration_ms));
                    busy_intervals.sort_by_key(|(s, _)| *s);
                    current_cursor = proposed_end + break_duration_ms;
                } else {
                    current_cursor = proposed_end;
                }

                placed = true;
                break;
            }

            // Nächster Suchzeitpunkt: nach dem überlappenden Intervall
            let next_free = busy_intervals
                .iter()
                .filter(|(_s, e)| *e > search_time)
                .map(|(_, e)| *e)
                .min()
                .unwrap_or(search_time + 15 * 60 * 1000);

            search_time = search_time.max(next_free);
        }

        if !placed {
            unscheduled.push(UnscheduledTask {
                task_id: task.id.clone(),
                reason: "Kein freier Slot im Planungsfenster gefunden".to_string(),
            });
        }
    }

    // Slots chronologisch sortieren
    slots.sort_by_key(|slot| match slot {
        ScheduledSlot::Event { start_unix_ms, .. } => *start_unix_ms,
        ScheduledSlot::Task { start_unix_ms, .. } => *start_unix_ms,
        ScheduledSlot::Break { start_unix_ms, .. } => *start_unix_ms,
    });

    Ok(SolvedPlan { slots, unscheduled })
}
