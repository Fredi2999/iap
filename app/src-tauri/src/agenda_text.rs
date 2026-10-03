//! Termine und Aufgaben als Text für den Kalender-Baustein im Workflow.
//!
//! Warum ohne Datumsbibliothek: Der Kern bleibt klein (Größenbudget), und der Planer
//! rechnet schon mit Unix-Millisekunden. Das Datum wird aus der Tageszahl berechnet
//! (Algorithmus nach Howard Hinnant, gültig für den gregorianischen Kalender).

use pa_scheduler::{Event, Task, TaskStatus};

const DAY_MS: i64 = 86_400_000;
const WEEKDAYS: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];

/// Datum und Uhrzeit in lokaler Zeit (`offset_minutes` zu UTC).
struct Local {
    year: i64,
    month: u32,
    day: u32,
    weekday: usize,
    hour: i64,
    minute: i64,
}

fn local(ms: i64, offset_minutes: i32) -> Local {
    let shifted = ms + i64::from(offset_minutes) * 60_000;
    let days = shifted.div_euclid(DAY_MS);
    let in_day = shifted.rem_euclid(DAY_MS);
    // 1970-01-01 war ein Donnerstag (Index 3 bei Montag = 0).
    let weekday = usize::try_from((days + 3).rem_euclid(7)).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    Local {
        year: if month <= 2 { year + 1 } else { year },
        month,
        day,
        weekday,
        hour: in_day / 3_600_000,
        minute: in_day % 3_600_000 / 60_000,
    }
}

fn date_label(l: &Local) -> String {
    format!(
        "{} {:02}.{:02}.{}",
        WEEKDAYS[l.weekday], l.day, l.month, l.year
    )
}

/// Schreibt die Termine der nächsten `days_ahead` Tage und (optional) die offenen Aufgaben.
///
/// Die Ausgabe ist bewusst schlicht und stabil sortiert, damit das Modell sie zuverlässig lesen kann.
pub fn format_agenda(
    events: &[Event],
    tasks: &[Task],
    now_ms: i64,
    days_ahead: u32,
    include_tasks: bool,
    offset_minutes: i32,
) -> String {
    let start_of_today = {
        let shifted = now_ms + i64::from(offset_minutes) * 60_000;
        shifted.div_euclid(DAY_MS) * DAY_MS - i64::from(offset_minutes) * 60_000
    };
    let until = start_of_today + i64::from(days_ahead.max(1)) * DAY_MS;

    let mut upcoming: Vec<&Event> = events
        .iter()
        .filter(|e| e.end_unix_ms > start_of_today && e.start_unix_ms < until)
        .collect();
    upcoming.sort_by_key(|e| (e.start_unix_ms, e.end_unix_ms));

    let mut out = format!("Termine der nächsten {days_ahead} Tage:\n");
    if upcoming.is_empty() {
        out.push_str("- keine Termine\n");
    }
    let mut last_day = String::new();
    for event in upcoming {
        let from = local(event.start_unix_ms, offset_minutes);
        let to = local(event.end_unix_ms, offset_minutes);
        let day = date_label(&from);
        if day != last_day {
            out.push_str(&format!("{day}\n"));
            last_day = day;
        }
        out.push_str(&format!(
            "- {:02}:{:02}-{:02}:{:02} {}",
            from.hour, from.minute, to.hour, to.minute, event.title
        ));
        if let Some(place) = event.location.as_deref().filter(|p| !p.trim().is_empty()) {
            out.push_str(&format!(" ({place})"));
        }
        out.push('\n');
    }

    if include_tasks {
        let mut open: Vec<&Task> = tasks
            .iter()
            .filter(|t| !matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
            .collect();
        open.sort_by_key(|t| {
            (
                std::cmp::Reverse(t.priority),
                t.due_unix_ms.unwrap_or(i64::MAX),
            )
        });
        out.push_str("\nOffene Aufgaben:\n");
        if open.is_empty() {
            out.push_str("- keine offenen Aufgaben\n");
        }
        for task in open {
            out.push_str(&format!(
                "- {} ({} Min., Wichtigkeit {})",
                task.title, task.duration_minutes, task.priority
            ));
            if let Some(due) = task.due_unix_ms {
                out.push_str(&format!(
                    ", fällig {}",
                    date_label(&local(due, offset_minutes))
                ));
            }
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_scheduler::Energy;

    fn utc(y: i64, m: i64, d: i64, h: i64, min: i64) -> i64 {
        // Gegenprobe zur Datumsrechnung: Tage seit 1970 nach derselben Formel rückwärts.
        let y2 = if m <= 2 { y - 1 } else { y };
        let era = y2.div_euclid(400);
        let yoe = y2 - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        days * DAY_MS + h * 3_600_000 + min * 60_000
    }

    fn event(id: &str, title: &str, from: i64, to: i64) -> Event {
        Event {
            id: id.into(),
            title: title.into(),
            start_unix_ms: from,
            end_unix_ms: to,
            location: None,
            project: None,
            external_uid: None,
        }
    }

    fn task(id: &str, title: &str, status: TaskStatus, priority: u8) -> Task {
        Task {
            id: id.into(),
            title: title.into(),
            project: None,
            due_unix_ms: None,
            duration_minutes: 30,
            energy: Energy::Medium,
            priority,
            depends_on: vec![],
            status,
        }
    }

    #[test]
    fn dates_are_computed_correctly_including_leap_days_and_offsets() {
        let l = local(utc(2024, 2, 29, 12, 0), 0);
        assert_eq!((l.year, l.month, l.day, l.weekday), (2024, 2, 29, 3)); // Donnerstag
        let l = local(utc(2026, 10, 1, 8, 30), 0);
        assert_eq!((l.year, l.month, l.day, l.weekday), (2026, 10, 1, 3));
        // Mitternacht in Wien (UTC+2) liegt am Vortag um 22 Uhr UTC.
        let l = local(utc(2026, 10, 1, 22, 0), 120);
        assert_eq!((l.month, l.day, l.hour), (10, 2, 0));
        let l = local(utc(1999, 12, 31, 23, 59), 0);
        assert_eq!((l.year, l.month, l.day), (1999, 12, 31));
    }

    #[test]
    fn the_agenda_lists_only_the_window_sorted_with_local_times() {
        let now = utc(2026, 10, 1, 6, 0); // 8:00 in Wien
        let events = vec![
            event(
                "b",
                "Zahnarzt",
                utc(2026, 10, 2, 8, 0),
                utc(2026, 10, 2, 9, 0),
            ),
            event("a", "Team", utc(2026, 10, 1, 8, 0), utc(2026, 10, 1, 9, 0)),
            event(
                "x",
                "Später",
                utc(2026, 10, 20, 8, 0),
                utc(2026, 10, 20, 9, 0),
            ),
            event(
                "y",
                "Gestern",
                utc(2026, 9, 30, 8, 0),
                utc(2026, 9, 30, 9, 0),
            ),
        ];
        let text = format_agenda(&events, &[], now, 3, false, 120);
        assert!(text.contains("Do 01.10.2026\n- 10:00-11:00 Team"), "{text}");
        assert!(
            text.contains("Fr 02.10.2026\n- 10:00-11:00 Zahnarzt"),
            "{text}"
        );
        assert!(
            !text.contains("Später") && !text.contains("Gestern"),
            "{text}"
        );
        assert!(text.find("Team") < text.find("Zahnarzt"));
        assert!(!text.contains("Offene Aufgaben"));
    }

    #[test]
    fn tasks_are_listed_by_priority_without_finished_ones() {
        let tasks = vec![
            task("1", "Mails", TaskStatus::Open, 2),
            task("2", "Steuer", TaskStatus::Done, 5),
            task("3", "Präsentation", TaskStatus::InProgress, 5),
        ];
        let text = format_agenda(&[], &tasks, utc(2026, 10, 1, 6, 0), 7, true, 0);
        assert!(text.contains("- keine Termine"));
        assert!(!text.contains("Steuer"));
        assert!(text.find("Präsentation") < text.find("Mails"), "{text}");
        let empty = format_agenda(&[], &[], utc(2026, 10, 1, 6, 0), 7, true, 0);
        assert!(empty.contains("- keine offenen Aufgaben"));
    }
}
