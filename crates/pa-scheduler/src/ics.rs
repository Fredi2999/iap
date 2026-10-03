//! Minimaler iCalendar-Import und -Export (Konzept 12).
//!
//! Umfang bewusst schmal: `VCALENDAR` mit `VEVENT`/`VTODO`, Timestamps als
//! `YYYYMMDDTHHMMSSZ` (UTC). Zeilenumbrüche als CRLF, Zeilenlängen ≤ 75
//! Zeichen mit Folgezeilen-Einrückung nach RFC 5545 werden beim Lesen
//! zusammengeführt; beim Schreiben werden Zeilen nicht künstlich gefaltet
//! (was RFC-konforme Reader problemlos akzeptieren).
//!
//! Zeitzonen: ausschließlich UTC. Die App liefert lokale Zeiten schon in
//! UTC-Millisekunden; Ortszeit-Anzeige geschieht im Frontend.

use thiserror::Error;

use crate::model::{Energy, Event, Task, TaskStatus};

const MS_PER_SECOND: i64 = 1_000;

#[derive(Debug, Error)]
pub enum IcsError {
    #[error("ICS-Parse: {0}")]
    Parse(String),
}

/// Ergebnis eines ICS-Imports: Kalender-Termine + Aufgaben (VTODO).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IcsImport {
    pub events: Vec<Event>,
    pub tasks: Vec<Task>,
}

/// Liest einen ICS-Text ein. Unbekannte Properties werden ignoriert.
pub fn import_ics(text: &str) -> Result<IcsImport, IcsError> {
    let lines = unfold_lines(text);
    let mut events = Vec::new();
    let mut tasks = Vec::new();
    let mut iter = lines.iter().enumerate();
    while let Some((_, line)) = iter.next() {
        match line.as_str() {
            "BEGIN:VEVENT" => {
                let event = read_vevent(&mut iter)?;
                events.push(event);
            }
            "BEGIN:VTODO" => {
                let task = read_vtodo(&mut iter)?;
                tasks.push(task);
            }
            _ => {}
        }
    }
    Ok(IcsImport { events, tasks })
}

/// Schreibt Termine + Aufgaben als ICS-Text.
pub fn export_ics(events: &[Event], tasks: &[Task]) -> String {
    let mut buffer = String::new();
    push_line(&mut buffer, "BEGIN:VCALENDAR");
    push_line(&mut buffer, "VERSION:2.0");
    push_line(&mut buffer, "PRODID:-//PortableAI//pa-scheduler//DE");
    for event in events {
        push_line(&mut buffer, "BEGIN:VEVENT");
        push_line(
            &mut buffer,
            &format!("UID:{}", event.external_uid.as_deref().unwrap_or(&event.id)),
        );
        push_line(
            &mut buffer,
            &format!("SUMMARY:{}", escape_text(&event.title)),
        );
        push_line(
            &mut buffer,
            &format!("DTSTART:{}", format_dt_utc(event.start_unix_ms)),
        );
        push_line(
            &mut buffer,
            &format!("DTEND:{}", format_dt_utc(event.end_unix_ms)),
        );
        if let Some(project) = &event.project {
            push_line(&mut buffer, &format!("CATEGORIES:{}", escape_text(project)));
        }
        if let Some(location) = &event.location {
            push_line(&mut buffer, &format!("LOCATION:{}", escape_text(location)));
        }
        push_line(&mut buffer, "END:VEVENT");
    }
    for task in tasks {
        push_line(&mut buffer, "BEGIN:VTODO");
        push_line(&mut buffer, &format!("UID:{}", task.id));
        push_line(
            &mut buffer,
            &format!("SUMMARY:{}", escape_text(&task.title)),
        );
        if let Some(due) = task.due_unix_ms {
            push_line(&mut buffer, &format!("DUE:{}", format_dt_utc(due)));
        }
        if let Some(project) = &task.project {
            push_line(&mut buffer, &format!("CATEGORIES:{}", escape_text(project)));
        }
        push_line(
            &mut buffer,
            &format!("PRIORITY:{}", priority_to_ics(task.priority)),
        );
        push_line(
            &mut buffer,
            &format!("STATUS:{}", status_to_ics(task.status)),
        );
        // Nutzeraddiertes Feld für Dauer + Energie (X-Prefix ist RFC-konform).
        push_line(
            &mut buffer,
            &format!("X-PORTABLEAI-DURATION-MIN:{}", task.duration_minutes),
        );
        push_line(
            &mut buffer,
            &format!("X-PORTABLEAI-ENERGY:{}", energy_to_ics(task.energy)),
        );
        push_line(&mut buffer, "END:VTODO");
    }
    push_line(&mut buffer, "END:VCALENDAR");
    buffer
}

fn push_line(buffer: &mut String, line: &str) {
    buffer.push_str(line);
    buffer.push_str("\r\n");
}

fn unfold_lines(text: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            if let Some(last) = result.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        result.push(line.to_owned());
    }
    result
}

fn read_vevent<'a>(
    iter: &mut impl Iterator<Item = (usize, &'a String)>,
) -> Result<Event, IcsError> {
    let mut uid = None;
    let mut summary = None;
    let mut dtstart = None;
    let mut dtend = None;
    let mut location = None;
    let mut categories = None;
    for (_, line) in iter.by_ref() {
        if line == "END:VEVENT" {
            let start = dtstart.ok_or_else(|| IcsError::Parse("VEVENT ohne DTSTART".to_owned()))?;
            let end = dtend.ok_or_else(|| IcsError::Parse("VEVENT ohne DTEND".to_owned()))?;
            let id = uid.clone().unwrap_or_else(|| format!("event-{start}"));
            return Ok(Event {
                id: id.clone(),
                title: summary.unwrap_or_else(|| "(ohne Titel)".to_owned()),
                start_unix_ms: start,
                end_unix_ms: end,
                location,
                project: categories,
                external_uid: uid,
            });
        }
        if let Some((key, value)) = split_property(line) {
            match key {
                "UID" => uid = Some(value.to_owned()),
                "SUMMARY" => summary = Some(unescape_text(value)),
                "DTSTART" => dtstart = Some(parse_dt_utc(value)?),
                "DTEND" => dtend = Some(parse_dt_utc(value)?),
                "LOCATION" => location = Some(unescape_text(value)),
                "CATEGORIES" => categories = Some(unescape_text(value)),
                _ => {}
            }
        }
    }
    Err(IcsError::Parse("VEVENT nicht abgeschlossen".to_owned()))
}

fn read_vtodo<'a>(iter: &mut impl Iterator<Item = (usize, &'a String)>) -> Result<Task, IcsError> {
    let mut uid = None;
    let mut summary = None;
    let mut due = None;
    let mut categories = None;
    let mut priority: Option<u8> = None;
    let mut duration_minutes: u32 = 30;
    let mut energy = Energy::Medium;
    let mut status = TaskStatus::Open;
    for (_, line) in iter.by_ref() {
        if line == "END:VTODO" {
            let id = uid.unwrap_or_else(|| format!("task-{}", summary.as_deref().unwrap_or("x")));
            return Ok(Task {
                id,
                title: summary.unwrap_or_else(|| "(ohne Titel)".to_owned()),
                project: categories,
                due_unix_ms: due,
                duration_minutes,
                energy,
                priority: priority.unwrap_or(3),
                depends_on: Vec::new(),
                status,
            });
        }
        if let Some((key, value)) = split_property(line) {
            match key {
                "UID" => uid = Some(value.to_owned()),
                "SUMMARY" => summary = Some(unescape_text(value)),
                "DUE" => due = Some(parse_dt_utc(value)?),
                "CATEGORIES" => categories = Some(unescape_text(value)),
                "PRIORITY" => priority = Some(priority_from_ics(value)),
                "STATUS" => status = status_from_ics(value),
                "X-PORTABLEAI-DURATION-MIN" => {
                    duration_minutes = value.parse::<u32>().unwrap_or(30);
                }
                "X-PORTABLEAI-ENERGY" => energy = energy_from_ics(value),
                _ => {}
            }
        }
    }
    Err(IcsError::Parse("VTODO nicht abgeschlossen".to_owned()))
}

fn split_property(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let (raw_key, rest) = line.split_at(colon);
    // Parameter (nach `;`) für unser MVP ignorieren, nur den reinen Property-
    // Namen verwenden.
    let key = raw_key.split(';').next().unwrap_or(raw_key);
    Some((key, &rest[1..]))
}

fn parse_dt_utc(value: &str) -> Result<i64, IcsError> {
    // Erwartet `YYYYMMDDTHHMMSSZ` (16 Zeichen) oder ohne Z (dann lokal;
    // wir behandeln es als UTC und dokumentieren das).
    let trimmed = value.trim_end_matches('Z');
    if trimmed.len() != 15 || trimmed.as_bytes().get(8) != Some(&b'T') {
        return Err(IcsError::Parse(format!("unerwartetes Datum `{value}`")));
    }
    let year: i32 = trimmed[0..4]
        .parse()
        .map_err(|_| IcsError::Parse("Jahr".into()))?;
    let month: u32 = trimmed[4..6]
        .parse()
        .map_err(|_| IcsError::Parse("Monat".into()))?;
    let day: u32 = trimmed[6..8]
        .parse()
        .map_err(|_| IcsError::Parse("Tag".into()))?;
    let hour: u32 = trimmed[9..11]
        .parse()
        .map_err(|_| IcsError::Parse("Stunde".into()))?;
    let minute: u32 = trimmed[11..13]
        .parse()
        .map_err(|_| IcsError::Parse("Minute".into()))?;
    let second: u32 = trimmed[13..15]
        .parse()
        .map_err(|_| IcsError::Parse("Sekunde".into()))?;
    let days = days_from_civil(year, month, day)?;
    let seconds = i64::from(days) * 86_400
        + i64::from(hour) * 3600
        + i64::from(minute) * 60
        + i64::from(second);
    Ok(seconds * MS_PER_SECOND)
}

fn format_dt_utc(unix_ms: i64) -> String {
    let total_seconds = unix_ms.div_euclid(MS_PER_SECOND);
    let seconds_of_day = total_seconds.rem_euclid(86_400) as i32;
    let days = total_seconds.div_euclid(86_400) as i32;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day / 60) % 60;
    let second = seconds_of_day % 60;
    format!(
        "{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z",
        year = year,
        month = month,
        day = day,
        hour = hour,
        minute = minute,
        second = second,
    )
}

/// Howard-Hinnant „days_from_civil" (public domain). Konvertiert ein
/// Gregorianisches Datum in Tage seit 1970-01-01.
fn days_from_civil(year: i32, month: u32, day: u32) -> Result<i32, IcsError> {
    if !(1..=12).contains(&month) {
        return Err(IcsError::Parse("Monat außerhalb 1..12".into()));
    }
    if !(1..=31).contains(&day) {
        return Err(IcsError::Parse("Tag außerhalb 1..31".into()));
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32; // 0..399
    let m_adj = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m_adj + 2) / 5 + day - 1; // 0..365
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // 0..146096
    Ok(era * 146_097 + doe as i32 - 719_468)
}

fn civil_from_days(days: i32) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i32 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

fn escape_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(',', "\\,")
        .replace(';', "\\;")
}

fn unescape_text(text: &str) -> String {
    text.replace("\\n", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

fn priority_to_ics(priority: u8) -> u8 {
    // ICS-Priorität: 0 = keine, 1 = hoch, 9 = niedrig.
    match priority {
        5 => 1,
        4 => 3,
        3 => 5,
        2 => 7,
        1 => 9,
        _ => 0,
    }
}

fn priority_from_ics(value: &str) -> u8 {
    match value.trim().parse::<u8>().unwrap_or(0) {
        1..=2 => 5,
        3..=4 => 4,
        5..=6 => 3,
        7..=8 => 2,
        9 => 1,
        _ => 3,
    }
}

fn status_to_ics(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Open => "NEEDS-ACTION",
        TaskStatus::InProgress => "IN-PROCESS",
        TaskStatus::Done => "COMPLETED",
        TaskStatus::Cancelled => "CANCELLED",
    }
}

fn status_from_ics(value: &str) -> TaskStatus {
    match value {
        "IN-PROCESS" => TaskStatus::InProgress,
        "COMPLETED" => TaskStatus::Done,
        "CANCELLED" => TaskStatus::Cancelled,
        _ => TaskStatus::Open,
    }
}

fn energy_to_ics(energy: Energy) -> &'static str {
    match energy {
        Energy::Low => "low",
        Energy::Medium => "medium",
        Energy::High => "high",
    }
}

fn energy_from_ics(value: &str) -> Energy {
    match value.trim() {
        "low" => Energy::Low,
        "high" => Energy::High,
        _ => Energy::Medium,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_event() {
        let event = Event {
            id: "e-1".to_owned(),
            title: "Test, mit Komma".to_owned(),
            start_unix_ms: 1_700_000_000_000,
            end_unix_ms: 1_700_003_600_000,
            location: Some("Zimmer 12".to_owned()),
            project: Some("Alpha".to_owned()),
            external_uid: None,
        };
        let text = export_ics(std::slice::from_ref(&event), &[]);
        let parsed = import_ics(&text).unwrap();
        assert_eq!(parsed.events.len(), 1);
        assert_eq!(parsed.events[0].title, "Test, mit Komma");
        assert_eq!(parsed.events[0].start_unix_ms, event.start_unix_ms);
        assert_eq!(parsed.events[0].end_unix_ms, event.end_unix_ms);
    }

    #[test]
    fn round_trip_task_with_x_props() {
        let task = Task {
            id: "t-1".to_owned(),
            title: "Bericht schreiben".to_owned(),
            project: None,
            due_unix_ms: Some(1_700_090_000_000),
            duration_minutes: 90,
            energy: Energy::High,
            priority: 5,
            depends_on: Vec::new(),
            status: TaskStatus::Open,
        };
        let text = export_ics(&[], std::slice::from_ref(&task));
        let parsed = import_ics(&text).unwrap();
        assert_eq!(parsed.tasks.len(), 1);
        let t = &parsed.tasks[0];
        assert_eq!(t.title, "Bericht schreiben");
        assert_eq!(t.duration_minutes, 90);
        assert_eq!(t.energy, Energy::High);
        assert_eq!(t.priority, 5);
        assert_eq!(t.due_unix_ms, Some(1_700_090_000_000));
    }

    #[test]
    fn parses_folded_lines() {
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:xy\r\nSUMMARY:Sehr\r\n  langer Titel\r\nDTSTART:20261119T090000Z\r\nDTEND:20261119T100000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let parsed = import_ics(text).unwrap();
        assert_eq!(parsed.events[0].title, "Sehr langer Titel");
    }

    #[test]
    fn dt_format_is_reversible() {
        let ms = 1_732_000_000_000_i64;
        let text = format_dt_utc(ms);
        let parsed = parse_dt_utc(&text).unwrap();
        assert_eq!(parsed, ms - ms % 1000);
    }
}
