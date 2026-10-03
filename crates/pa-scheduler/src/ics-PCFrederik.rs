use serde::{Deserialize, Serialize};

use crate::model::{Energy, Event, SchedulerError, Task, TaskStatus};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IcsImport {
    pub events: Vec<Event>,
    pub tasks: Vec<Task>,
}

/// Parst einen RFC 5545 iCalendar Text in Termine und Aufgaben.
pub fn import_ics(text: &str) -> Result<IcsImport, SchedulerError> {
    let mut events = Vec::new();
    let mut tasks = Vec::new();

    let mut in_vevent = false;
    let mut in_vtodo = false;

    let mut current_summary = String::new();
    let mut current_uid = String::new();
    let mut current_start = 0i64;
    let mut current_end = 0i64;
    let mut current_location: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line == "BEGIN:VEVENT" {
            in_vevent = true;
            current_summary.clear();
            current_uid.clear();
            current_start = 0;
            current_end = 0;
            current_location = None;
        } else if line == "END:VEVENT" {
            if in_vevent {
                in_vevent = false;
                let id = if current_uid.is_empty() {
                    format!("evt-{}", events.len() + 1)
                } else {
                    current_uid.clone()
                };
                let end = if current_end > current_start {
                    current_end
                } else {
                    current_start + 3600_000 // 1 Stunde Default
                };
                events.push(Event {
                    id,
                    title: if current_summary.is_empty() {
                        "Termin".to_string()
                    } else {
                        current_summary.clone()
                    },
                    start_unix_ms: current_start,
                    end_unix_ms: end,
                    location: current_location.clone(),
                    project: None,
                    external_uid: if current_uid.is_empty() {
                        None
                    } else {
                        Some(current_uid.clone())
                    },
                });
            }
        } else if line == "BEGIN:VTODO" {
            in_vtodo = true;
            current_summary.clear();
            current_uid.clear();
            current_start = 0;
            current_end = 0;
        } else if line == "END:VTODO" {
            if in_vtodo {
                in_vtodo = false;
                let id = if current_uid.is_empty() {
                    format!("tsk-{}", tasks.len() + 1)
                } else {
                    current_uid.clone()
                };
                tasks.push(Task {
                    id,
                    title: if current_summary.is_empty() {
                        "Aufgabe".to_string()
                    } else {
                        current_summary.clone()
                    },
                    project: None,
                    due_unix_ms: if current_end > 0 { Some(current_end) } else { None },
                    duration_minutes: 30,
                    energy: Energy::Medium,
                    priority: 3,
                    depends_on: Vec::new(),
                    status: TaskStatus::Open,
                });
            }
        } else if in_vevent || in_vtodo {
            if let Some((key, val)) = line.split_once(':') {
                let key_norm = key.split(';').next().unwrap_or(key).to_uppercase();
                match key_norm.as_str() {
                    "SUMMARY" => current_summary = val.to_string(),
                    "UID" => current_uid = val.to_string(),
                    "LOCATION" => current_location = Some(val.to_string()),
                    "DTSTART" => current_start = parse_ics_timestamp(val),
                    "DTEND" | "DUE" => current_end = parse_ics_timestamp(val),
                    _ => {}
                }
            }
        }
    }

    Ok(IcsImport { events, tasks })
}

/// Serialisiert Termine und Aufgaben als RFC 5545 iCalendar String.
pub fn export_ics(events: &[Event], tasks: &[Task]) -> String {
    let mut out = String::new();
    out.push_str("BEGIN:VCALENDAR\r\n");
    out.push_str("VERSION:2.0\r\n");
    out.push_str("PRODID:-//IAP//Deterministic Scheduler//DE\r\n");

    for event in events {
        out.push_str("BEGIN:VEVENT\r\n");
        out.push_str(&format!("UID:{}\r\n", event.id));
        out.push_str(&format!("SUMMARY:{}\r\n", escape_ics(&event.title)));
        out.push_str(&format!("DTSTART:{}\r\n", format_ics_timestamp(event.start_unix_ms)));
        out.push_str(&format!("DTEND:{}\r\n", format_ics_timestamp(event.end_unix_ms)));
        if let Some(loc) = &event.location {
            out.push_str(&format!("LOCATION:{}\r\n", escape_ics(loc)));
        }
        out.push_str("END:VEVENT\r\n");
    }

    for task in tasks {
        out.push_str("BEGIN:VTODO\r\n");
        out.push_str(&format!("UID:{}\r\n", task.id));
        out.push_str(&format!("SUMMARY:{}\r\n", escape_ics(&task.title)));
        if let Some(due) = task.due_unix_ms {
            out.push_str(&format!("DUE:{}\r\n", format_ics_timestamp(due)));
        }
        let status_str = match task.status {
            TaskStatus::Open => "NEEDS-ACTION",
            TaskStatus::InProgress => "IN-PROCESS",
            TaskStatus::Done => "COMPLETED",
            TaskStatus::Cancelled => "CANCELLED",
        };
        out.push_str(&format!("STATUS:{}\r\n", status_str));
        out.push_str("END:VTODO\r\n");
    }

    out.push_str("END:VCALENDAR\r\n");
    out
}

fn parse_ics_timestamp(s: &str) -> i64 {
    // Einfacher ISO / ICS Timestamp Parser (YYYYMMDDTHHMMSSZ oder YYYYMMDD)
    let s = s.trim().trim_end_matches('Z');
    if s.len() >= 8 {
        let year: i64 = s[0..4].parse().unwrap_or(2026);
        let month: i64 = s[4..6].parse().unwrap_or(1);
        let day: i64 = s[6..8].parse().unwrap_or(1);

        let mut hour: i64 = 0;
        let mut min: i64 = 0;
        let mut sec: i64 = 0;

        if s.len() >= 15 && s.chars().nth(8) == Some('T') {
            hour = s[9..11].parse().unwrap_or(0);
            min = s[11..13].parse().unwrap_or(0);
            sec = s[13..15].parse().unwrap_or(0);
        }

        // Näherungsweise Unix ms (reicht für Schedule-Darstellung)
        let days_approx = (year - 1970) * 365 + (year - 1968) / 4 + (month - 1) * 30 + day;
        let total_secs = days_approx * 86400 + hour * 3600 + min * 60 + sec;
        total_secs * 1000
    } else {
        0
    }
}

fn format_ics_timestamp(unix_ms: i64) -> String {
    let total_secs = unix_ms / 1000;
    let days = total_secs / 86400;
    let rem_secs = total_secs % 86400;
    let hour = rem_secs / 3600;
    let min = (rem_secs % 3600) / 60;
    let sec = rem_secs % 60;

    // Näherungsweises Jahr/Monat/Tag
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = (day_of_year / 30) + 1;
    let day = (day_of_year % 30) + 1;

    format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", year, month, day, hour, min, sec)
}

fn escape_ics(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}
