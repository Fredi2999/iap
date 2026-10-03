//! Fremde Kalender lesen (iCalendar-Feeds) und neue Termine schreiben.
//!
//! Anders als `ics` (Import/Export der eigenen Daten, nur UTC) versteht dieser Teil, was Apple und
//! Google tatsächlich liefern: Zeitzonen über `VTIMEZONE`, Ganztagstermine (`VALUE=DATE`), Serien
//! (`RRULE`) samt `EXDATE`, `RDATE` und einzeln geänderten Terminen (`RECURRENCE-ID`). Das Ergebnis
//! sind fertige Einzeltermine in einem Zeitfenster.
//!
//! Grenzen, bewusst und sichtbar: Serienregeln, die hier nicht unterstützt werden (stündlich,
//! `BYSETPOS`, Wochennummern, Tage im Jahr …), werden **nicht geraten**. Der erste Termin der Serie
//! erscheint, und [`Parsed::unsupported_rules`] zählt die Serie, damit die Oberfläche es melden kann.
//! Fehlt die Zeitzonen-Definition einer `TZID`, gilt der Versatz des Rechners (`local_offset_minutes`).
//!
//! Die Crate hat keine Zeitbibliothek (siehe `lib.rs`); gerechnet wird mit `civil`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::civil::{
    civil_from_days, days_from_civil, days_in_month, epoch_seconds, ics_date, ics_datetime_utc,
    iso_date, nth_weekday_of_month, weekday,
};

/// So viele Einzeltermine entstehen höchstens aus einer Serie.
const MAX_INSTANCES_PER_SERIES: usize = 1_000;
/// So viele Wiederholungsperioden werden höchstens durchlaufen (Schutz vor Endlosschleifen).
const MAX_PERIODS: u32 = 40_000;
/// Längste übernommene Notiz in Zeichen.
const MAX_NOTES_CHARS: usize = 2_000;
const MS_PER_DAY: i64 = 86_400_000;

/// Fehler beim Bau eines neuen Termins.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FeedError {
    #[error("{0}")]
    Invalid(String),
}

/// Zeitfenster, in dem Termine gesucht werden (halboffen: `from <= Zeit < to`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub from_unix_ms: i64,
    pub to_unix_ms: i64,
}

/// Ein einzelner Termin (bei Serien: ein Vorkommen).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    /// UID des Termins im Feed (bei Serien für alle Vorkommen gleich).
    pub uid: String,
    pub title: String,
    /// Beginn in Unix-Millisekunden (UTC). Bei Ganztagsterminen nur eine Näherung über den
    /// Rechner-Versatz; maßgeblich sind dort `start_date` und `end_date`.
    pub start_unix_ms: i64,
    /// Ende (exklusiv); bei Terminen ohne Dauer gleich dem Beginn.
    pub end_unix_ms: i64,
    pub all_day: bool,
    /// `YYYY-MM-DD`, nur bei Ganztagsterminen.
    pub start_date: Option<String>,
    /// `YYYY-MM-DD`, **exklusiv** (der Tag nach dem letzten Tag), nur bei Ganztagsterminen.
    pub end_date: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    /// Ob der Termin aus einer Serie stammt.
    pub recurring: bool,
}

/// Ergebnis des Lesens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    /// Nach Beginn und Titel sortiert.
    pub instances: Vec<Instance>,
    /// Serien mit einer Regel, die nicht ausgewertet werden kann.
    pub unsupported_rules: u32,
    /// Einträge, die unlesbar waren (etwa ohne Beginn).
    pub skipped_events: u32,
}

// ---------------------------------------------------------------------------------------------
// Zeilen und Eigenschaften
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
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

/// Trennt `text` an `sep` außerhalb von Anführungszeichen.
fn split_outside_quotes(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == sep && !in_quotes {
            parts.push(&text[start..i]);
            start = i + c.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

fn parse_prop(line: &str) -> Option<Prop> {
    let mut in_quotes = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == ':' && !in_quotes {
            colon = Some(i);
            break;
        }
    }
    let colon = colon?;
    let head = &line[..colon];
    let value = &line[colon + 1..];
    let mut pieces = split_outside_quotes(head, ';').into_iter();
    let name = pieces.next()?.trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let params = pieces
        .filter_map(|piece| {
            let (k, v) = piece.split_once('=')?;
            Some((
                k.trim().to_ascii_uppercase(),
                v.trim().trim_matches('"').to_owned(),
            ))
        })
        .collect();
    Some(Prop {
        name,
        params,
        value: value.to_owned(),
    })
}

fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Zeitpunkte und Zeitzonen
// ---------------------------------------------------------------------------------------------

/// Ein Datum mit optionaler Uhrzeit, wie es im Feed steht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Dt {
    days: i64,
    /// Sekunden seit Mitternacht; `None` bei `VALUE=DATE`.
    secs: Option<i64>,
    /// Ob der Wert mit `Z` endet.
    utc: bool,
}

fn parse_dt(value: &str) -> Option<Dt> {
    let value = value.trim();
    let (date_part, time_part) = match value.split_once(['T', 't']) {
        Some((d, t)) => (d, Some(t)),
        None => (value, None),
    };
    if date_part.len() != 8 || !date_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: i64 = date_part[0..4].parse().ok()?;
    let month: u32 = date_part[4..6].parse().ok()?;
    let day: u32 = date_part[6..8].parse().ok()?;
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let Some(time) = time_part else {
        return Some(Dt {
            days,
            secs: None,
            utc: false,
        });
    };
    let utc = time.ends_with(['Z', 'z']);
    let digits = time.trim_end_matches(['Z', 'z']);
    if digits.len() < 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hour: i64 = digits[0..2].parse().ok()?;
    let minute: i64 = digits[2..4].parse().ok()?;
    let second: i64 = if digits.len() >= 6 {
        digits[4..6].parse().ok()?
    } else {
        0
    };
    if hour > 24 || minute > 59 || second > 60 {
        return None;
    }
    Some(Dt {
        days,
        secs: Some(hour * 3600 + minute * 60 + second),
        utc,
    })
}

/// `+0100`, `-0530` oder `+010000` in Minuten.
fn parse_offset_minutes(value: &str) -> Option<i32> {
    let value = value.trim();
    let sign = match value.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let digits = &value[1..];
    if digits.len() < 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = digits[0..2].parse().ok()?;
    let minutes: i32 = digits[2..4].parse().ok()?;
    Some(sign * (hours * 60 + minutes))
}

/// `P1W`, `P2DT3H`, `PT45M` in Millisekunden (nur positive Dauern).
fn parse_duration_ms(value: &str) -> Option<i64> {
    let value = value.trim();
    let rest = value.strip_prefix('P')?;
    let mut total: i64 = 0;
    let mut number = String::new();
    let mut in_time = false;
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => number.push(c),
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                total += match (unit, in_time) {
                    ('W', false) => n * 7 * MS_PER_DAY,
                    ('D', false) => n * MS_PER_DAY,
                    ('H', true) => n * 3_600_000,
                    ('M', true) => n * 60_000,
                    ('S', true) => n * 1_000,
                    _ => return None,
                };
            }
        }
    }
    number.is_empty().then_some(total)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// Eine ausgewertete Wiederholungsregel.
#[derive(Debug, Clone)]
struct Rrule {
    freq: Freq,
    interval: u32,
    count: Option<u32>,
    until: Option<Dt>,
    /// `(Ordinal, Wochentag Mo=0)`; Ordinal 0 = jeder.
    by_day: Vec<(i32, u8)>,
    by_monthday: Vec<i32>,
    by_month: Vec<u32>,
    wkst: u8,
    /// Die Regel enthält etwas, das hier nicht ausgewertet wird.
    unsupported: bool,
}

fn weekday_code(code: &str) -> Option<u8> {
    Some(match code {
        "MO" => 0,
        "TU" => 1,
        "WE" => 2,
        "TH" => 3,
        "FR" => 4,
        "SA" => 5,
        "SU" => 6,
        _ => return None,
    })
}

fn parse_rrule(value: &str) -> Rrule {
    let mut rule = Rrule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_monthday: Vec::new(),
        by_month: Vec::new(),
        wkst: 0,
        unsupported: false,
    };
    let mut freq_seen = false;
    for part in value.split(';') {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq_seen = true;
                match val.trim().to_ascii_uppercase().as_str() {
                    "DAILY" => rule.freq = Freq::Daily,
                    "WEEKLY" => rule.freq = Freq::Weekly,
                    "MONTHLY" => rule.freq = Freq::Monthly,
                    "YEARLY" => rule.freq = Freq::Yearly,
                    _ => rule.unsupported = true,
                }
            }
            "INTERVAL" => match val.trim().parse::<u32>() {
                Ok(n) if n >= 1 => rule.interval = n,
                _ => rule.unsupported = true,
            },
            "COUNT" => match val.trim().parse::<u32>() {
                Ok(n) => rule.count = Some(n),
                Err(_) => rule.unsupported = true,
            },
            "UNTIL" => match parse_dt(val) {
                Some(dt) => rule.until = Some(dt),
                None => rule.unsupported = true,
            },
            "BYDAY" => {
                for item in val.split(',') {
                    let item = item.trim().to_ascii_uppercase();
                    if item.len() < 2 {
                        rule.unsupported = true;
                        continue;
                    }
                    let (num, code) = item.split_at(item.len() - 2);
                    let ordinal = if num.is_empty() {
                        Some(0)
                    } else {
                        num.parse::<i32>().ok().filter(|n| *n != 0 && n.abs() <= 53)
                    };
                    match (ordinal, weekday_code(code)) {
                        (Some(n), Some(wd)) => rule.by_day.push((n, wd)),
                        _ => rule.unsupported = true,
                    }
                }
            }
            "BYMONTHDAY" => {
                for item in val.split(',') {
                    match item.trim().parse::<i32>() {
                        Ok(n) if n != 0 && n.abs() <= 31 => rule.by_monthday.push(n),
                        _ => rule.unsupported = true,
                    }
                }
            }
            "BYMONTH" => {
                for item in val.split(',') {
                    match item.trim().parse::<u32>() {
                        Ok(n) if (1..=12).contains(&n) => rule.by_month.push(n),
                        _ => rule.unsupported = true,
                    }
                }
            }
            "WKST" => match weekday_code(&val.trim().to_ascii_uppercase()) {
                Some(wd) => rule.wkst = wd,
                None => rule.unsupported = true,
            },
            // Alles andere (BYSETPOS, BYWEEKNO, BYYEARDAY, BYHOUR …) wird nicht geraten.
            _ => rule.unsupported = true,
        }
    }
    if !freq_seen {
        rule.unsupported = true;
    }
    rule
}

/// Alle Tage eines Monats, auf die die Regel passt (`BYMONTHDAY` und/oder `BYDAY`, sonst
/// `default_day`). Aufsteigend und ohne Doppelte.
fn month_dates(year: i64, month: u32, rule: &Rrule, default_day: u32) -> Vec<i64> {
    let len = days_in_month(year, month);
    let first = days_from_civil(year, month, 1);
    let mut by_monthday: Vec<i64> = rule
        .by_monthday
        .iter()
        .filter_map(|&d| {
            let day = if d > 0 { d } else { len as i32 + d + 1 };
            (1..=len as i32)
                .contains(&day)
                .then(|| first + i64::from(day) - 1)
        })
        .collect();
    let mut by_day: Vec<i64> = Vec::new();
    for &(ordinal, wd) in &rule.by_day {
        if ordinal == 0 {
            by_day.extend(
                (0..i64::from(len))
                    .map(|i| first + i)
                    .filter(|d| weekday(*d) == wd),
            );
        } else if let Some(day) = nth_weekday_of_month(year, month, wd, ordinal) {
            by_day.push(day);
        }
    }
    let mut dates = match (rule.by_monthday.is_empty(), rule.by_day.is_empty()) {
        (true, true) => {
            if default_day <= len {
                vec![first + i64::from(default_day) - 1]
            } else {
                Vec::new()
            }
        }
        (false, true) => std::mem::take(&mut by_monthday),
        (true, false) => by_day,
        (false, false) => by_monthday
            .into_iter()
            .filter(|d| by_day.contains(d))
            .collect(),
    };
    dates.sort_unstable();
    dates.dedup();
    dates
}

/// Beobachtung (Winter- oder Sommerzeit) einer Zeitzone.
#[derive(Debug, Clone)]
struct Observance {
    /// Beginn als Ortszeit-Sekunden (so gerechnet, als wäre die Ortszeit UTC).
    onset: i64,
    from_minutes: i32,
    to_minutes: i32,
    rule: Option<Rrule>,
}

impl Observance {
    /// Zeitpunkte, an denen diese Beobachtung beginnt, für das Jahr und das Vorjahr.
    fn onsets_near(&self, year: i64) -> Vec<i64> {
        let Some(rule) = &self.rule else {
            return vec![self.onset];
        };
        let start_year = civil_from_days(self.onset.div_euclid(86_400)).0;
        let (_, base_month, base_day) = civil_from_days(self.onset.div_euclid(86_400));
        let time_of_day = self.onset.rem_euclid(86_400);
        let mut out = vec![self.onset];
        for y in [year - 1, year] {
            if y < start_year {
                continue;
            }
            let months: Vec<u32> = if rule.by_month.is_empty() {
                vec![base_month]
            } else {
                rule.by_month.clone()
            };
            for month in months {
                for day in month_dates(y, month, rule, base_day) {
                    let onset = epoch_seconds(day, time_of_day);
                    if onset >= self.onset {
                        out.push(onset);
                    }
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, Default)]
struct Zone {
    observances: Vec<Observance>,
}

impl Zone {
    /// Versatz zu UTC in Minuten für eine Ortszeit.
    fn offset_at_local(&self, local_secs: i64) -> i32 {
        let year = civil_from_days(local_secs.div_euclid(86_400)).0;
        let mut best: Option<(i64, i32)> = None;
        for observance in &self.observances {
            for onset in observance.onsets_near(year) {
                if onset <= local_secs && best.is_none_or(|(b, _)| onset > b) {
                    best = Some((onset, observance.to_minutes));
                }
            }
        }
        match best {
            Some((_, offset)) => offset,
            // Vor der ersten Umstellung gilt der Versatz „von“ der frühesten Beobachtung.
            None => self
                .observances
                .iter()
                .min_by_key(|o| o.onset)
                .map_or(0, |o| o.from_minutes),
        }
    }
}

struct Context {
    zones: HashMap<String, Zone>,
    local_offset_minutes: i32,
}

impl Context {
    fn offset_minutes(&self, tzid: Option<&str>, local_secs: i64) -> i32 {
        match tzid.and_then(|id| self.zones.get(id)) {
            Some(zone) if !zone.observances.is_empty() => zone.offset_at_local(local_secs),
            _ => self.local_offset_minutes,
        }
    }

    fn to_utc_ms(&self, dt: &Dt, tzid: Option<&str>) -> i64 {
        let local = epoch_seconds(dt.days, dt.secs.unwrap_or(0));
        if dt.utc {
            return local * 1_000;
        }
        (local - i64::from(self.offset_minutes(tzid, local)) * 60) * 1_000
    }
}

// ---------------------------------------------------------------------------------------------
// Aufbereitung der Bestandteile
// ---------------------------------------------------------------------------------------------

struct RawFeed {
    events: Vec<Vec<Prop>>,
    zones: HashMap<String, Zone>,
}

fn read_components(text: &str) -> RawFeed {
    let lines = unfold_lines(text);
    let mut events = Vec::new();
    let mut zones: HashMap<String, Zone> = HashMap::new();

    let mut current_event: Option<Vec<Prop>> = None;
    let mut alarm_depth = 0usize;

    let mut zone_id: Option<String> = None;
    let mut zone: Zone = Zone::default();
    let mut in_zone = false;
    let mut observance: Option<Vec<Prop>> = None;

    for line in &lines {
        let Some(prop) = parse_prop(line) else {
            continue;
        };
        let begin = prop.name == "BEGIN";
        let end = prop.name == "END";
        let kind = prop.value.trim().to_ascii_uppercase();

        if current_event.is_some() {
            if begin && kind == "VALARM" {
                alarm_depth += 1;
                continue;
            }
            if end && kind == "VALARM" {
                alarm_depth = alarm_depth.saturating_sub(1);
                continue;
            }
            if alarm_depth > 0 {
                continue;
            }
            if end && kind == "VEVENT" {
                if let Some(done) = current_event.take() {
                    events.push(done);
                }
                continue;
            }
            if let Some(props) = current_event.as_mut() {
                props.push(prop);
            }
            continue;
        }

        if in_zone {
            if let Some(props) = observance.as_mut() {
                if end && (kind == "STANDARD" || kind == "DAYLIGHT") {
                    if let Some(done) = observance.take() {
                        if let Some(parsed) = build_observance(&done) {
                            zone.observances.push(parsed);
                        }
                    }
                } else {
                    props.push(prop);
                }
                continue;
            }
            if begin && (kind == "STANDARD" || kind == "DAYLIGHT") {
                observance = Some(Vec::new());
                continue;
            }
            if end && kind == "VTIMEZONE" {
                if let Some(id) = zone_id.take() {
                    zones.insert(id, std::mem::take(&mut zone));
                }
                zone = Zone::default();
                in_zone = false;
                continue;
            }
            if prop.name == "TZID" {
                zone_id = Some(prop.value.trim().to_owned());
            }
            continue;
        }

        if begin && kind == "VEVENT" {
            current_event = Some(Vec::new());
            alarm_depth = 0;
        } else if begin && kind == "VTIMEZONE" {
            in_zone = true;
            zone_id = None;
            zone = Zone::default();
        }
    }
    RawFeed { events, zones }
}

fn build_observance(props: &[Prop]) -> Option<Observance> {
    let get = |name: &str| props.iter().find(|p| p.name == name);
    let start = parse_dt(&get("DTSTART")?.value)?;
    Some(Observance {
        onset: epoch_seconds(start.days, start.secs.unwrap_or(0)),
        from_minutes: parse_offset_minutes(&get("TZOFFSETFROM")?.value)?,
        to_minutes: parse_offset_minutes(&get("TZOFFSETTO")?.value)?,
        rule: get("RRULE").map(|p| parse_rrule(&p.value)),
    })
}

/// Ein Termin mit allen Angaben, die für die Auswertung gebraucht werden.
struct RawEvent {
    uid: String,
    title: String,
    location: Option<String>,
    notes: Option<String>,
    start: Dt,
    start_tzid: Option<String>,
    end: Option<(Dt, Option<String>)>,
    duration_ms: Option<i64>,
    rrule: Option<Rrule>,
    exdates: Vec<(Dt, Option<String>)>,
    rdates: Vec<(Dt, Option<String>)>,
    recurrence_id: Option<(Dt, Option<String>)>,
    cancelled: bool,
}

fn dt_list(prop: &Prop) -> Vec<(Dt, Option<String>)> {
    let tzid = prop.param("TZID").map(str::to_owned);
    prop.value
        .split(',')
        .filter_map(|v| parse_dt(v).map(|dt| (dt, tzid.clone())))
        .collect()
}

fn read_event(props: &[Prop]) -> Option<RawEvent> {
    let first = |name: &str| props.iter().find(|p| p.name == name);
    let start_prop = first("DTSTART")?;
    let start = parse_dt(&start_prop.value)?;
    let end = first("DTEND")
        .and_then(|p| parse_dt(&p.value).map(|dt| (dt, p.param("TZID").map(str::to_owned))));
    let notes = first("DESCRIPTION")
        .map(|p| unescape_text(&p.value))
        .filter(|n| !n.trim().is_empty())
        .map(|n| n.chars().take(MAX_NOTES_CHARS).collect());
    Some(RawEvent {
        uid: first("UID").map_or_else(String::new, |p| p.value.trim().to_owned()),
        title: first("SUMMARY")
            .map(|p| unescape_text(&p.value))
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| "(ohne Titel)".to_owned()),
        location: first("LOCATION")
            .map(|p| unescape_text(&p.value))
            .filter(|l| !l.trim().is_empty()),
        notes,
        start,
        start_tzid: start_prop.param("TZID").map(str::to_owned),
        end,
        duration_ms: first("DURATION").and_then(|p| parse_duration_ms(&p.value)),
        rrule: first("RRULE").map(|p| parse_rrule(&p.value)),
        exdates: props
            .iter()
            .filter(|p| p.name == "EXDATE")
            .flat_map(dt_list)
            .collect(),
        rdates: props
            .iter()
            .filter(|p| p.name == "RDATE")
            .flat_map(dt_list)
            .collect(),
        recurrence_id: first("RECURRENCE-ID")
            .and_then(|p| parse_dt(&p.value).map(|dt| (dt, p.param("TZID").map(str::to_owned)))),
        cancelled: first("STATUS")
            .is_some_and(|p| p.value.trim().eq_ignore_ascii_case("CANCELLED")),
    })
}

// ---------------------------------------------------------------------------------------------
// Auswertung
// ---------------------------------------------------------------------------------------------

/// Liest einen Feed und liefert alle Termine, die das Fenster berühren.
///
/// `local_offset_minutes` ist der Versatz des Rechners zu UTC (Wien im Winter = 60). Er gilt nur für
/// Zeiten ohne Zeitzone und für `TZID`s ohne Definition.
pub fn parse_feed(text: &str, window: Window, local_offset_minutes: i32) -> Parsed {
    let raw = read_components(text);
    let ctx = Context {
        zones: raw.zones,
        local_offset_minutes,
    };
    let mut parsed = Parsed::default();
    let mut events = Vec::new();
    for props in &raw.events {
        match read_event(props) {
            Some(event) => events.push(event),
            None => parsed.skipped_events += 1,
        }
    }

    // Einzeln geänderte oder abgesagte Vorkommen ersetzen das erzeugte Vorkommen der Serie.
    let overridden: HashSet<(String, i64)> = events
        .iter()
        .filter_map(|e| {
            e.recurrence_id
                .as_ref()
                .map(|(dt, tz)| (e.uid.clone(), ctx.to_utc_ms(dt, tz.as_deref())))
        })
        .collect();

    for event in &events {
        if event.cancelled {
            continue;
        }
        let skip: HashSet<i64> = overridden
            .iter()
            .filter(|(uid, _)| *uid == event.uid && !event.uid.is_empty())
            .map(|(_, key)| *key)
            .collect();
        expand_event(event, &ctx, window, &skip, &mut parsed);
    }
    parsed
        .instances
        .sort_by(|a, b| (a.start_unix_ms, &a.title).cmp(&(b.start_unix_ms, &b.title)));
    parsed
}

/// Regeln, die zwar lesbar sind, aber hier nicht ausgewertet werden: `BYMONTHDAY` bei täglichen
/// Serien und `BYDAY` ohne Monat bei jährlichen (bedeutet „im ganzen Jahr“).
fn needs_unsupported(rule: &Rrule) -> bool {
    (rule.freq == Freq::Daily && !rule.by_monthday.is_empty())
        || (rule.freq == Freq::Yearly && !rule.by_day.is_empty() && rule.by_month.is_empty())
}

fn overlaps(window: Window, start: i64, end: i64) -> bool {
    if end > start {
        end > window.from_unix_ms && start < window.to_unix_ms
    } else {
        start >= window.from_unix_ms && start < window.to_unix_ms
    }
}

/// Länge eines Vorkommens: Tage (Ganztag) oder Millisekunden.
enum Span {
    Days(i64),
    Millis(i64),
}

fn span_of(event: &RawEvent, ctx: &Context) -> Span {
    if event.start.secs.is_none() {
        let days = match (&event.end, event.duration_ms) {
            (Some((end, _)), _) => end.days - event.start.days,
            (None, Some(ms)) => (ms + MS_PER_DAY - 1) / MS_PER_DAY,
            (None, None) => 1,
        };
        return Span::Days(days.max(1));
    }
    let start_ms = ctx.to_utc_ms(&event.start, event.start_tzid.as_deref());
    let ms = match (&event.end, event.duration_ms) {
        (Some((end, tz)), _) => ctx.to_utc_ms(end, tz.as_deref()) - start_ms,
        (None, Some(ms)) => ms,
        (None, None) => 0,
    };
    Span::Millis(ms.max(0))
}

fn make_instance(
    event: &RawEvent,
    ctx: &Context,
    day: i64,
    secs: Option<i64>,
    span: &Span,
    recurring: bool,
) -> (i64, Instance) {
    let dt = Dt {
        days: day,
        secs,
        utc: event.start.utc,
    };
    let key = ctx.to_utc_ms(&dt, event.start_tzid.as_deref());
    let (start, end, all_day, start_date, end_date) = match span {
        Span::Days(n) => {
            let start = ctx.to_utc_ms(&dt, None);
            let end_dt = Dt {
                days: day + n,
                ..dt
            };
            (
                start,
                ctx.to_utc_ms(&end_dt, None),
                true,
                Some(iso_date(day)),
                Some(iso_date(day + n)),
            )
        }
        Span::Millis(ms) => (key, key + ms, false, None, None),
    };
    (
        key,
        Instance {
            uid: event.uid.clone(),
            title: event.title.clone(),
            start_unix_ms: start,
            end_unix_ms: end,
            all_day,
            start_date,
            end_date,
            location: event.location.clone(),
            notes: event.notes.clone(),
            recurring,
        },
    )
}

fn expand_event(
    event: &RawEvent,
    ctx: &Context,
    window: Window,
    overridden: &HashSet<i64>,
    out: &mut Parsed,
) {
    let span = span_of(event, ctx);
    let exdates: HashSet<i64> = event
        .exdates
        .iter()
        .map(|(dt, tz)| ctx.to_utc_ms(dt, tz.as_deref().or(event.start_tzid.as_deref())))
        .collect();
    let recurring =
        event.rrule.is_some() || !event.rdates.is_empty() || event.recurrence_id.is_some();
    let mut emitted = 0usize;
    let mut push = |key: i64, instance: Instance, out: &mut Parsed| {
        if exdates.contains(&key) || overridden.contains(&key) {
            return;
        }
        if emitted < MAX_INSTANCES_PER_SERIES
            && overlaps(window, instance.start_unix_ms, instance.end_unix_ms)
        {
            emitted += 1;
            out.instances.push(instance);
        }
    };

    let secs = event.start.secs;
    let base_days = event.start.days;
    match &event.rrule {
        None => {
            let (key, instance) = make_instance(event, ctx, base_days, secs, &span, recurring);
            push(key, instance, out);
        }
        Some(rule) if rule.unsupported || needs_unsupported(rule) => {
            out.unsupported_rules += 1;
            let (key, instance) = make_instance(event, ctx, base_days, secs, &span, true);
            push(key, instance, out);
        }
        Some(rule) => {
            let mut produced = 0u32;
            let (base_year, base_month, base_day) = civil_from_days(base_days);
            'periods: for k in 0..MAX_PERIODS {
                let step = i64::from(k) * i64::from(rule.interval);
                let dates: Vec<i64> = match rule.freq {
                    Freq::Daily => {
                        let day = base_days + step;
                        let (_, month, _) = civil_from_days(day);
                        let ok_day = rule.by_day.is_empty()
                            || rule.by_day.iter().any(|&(_, wd)| wd == weekday(day));
                        let ok_month = rule.by_month.is_empty() || rule.by_month.contains(&month);
                        if ok_day && ok_month {
                            vec![day]
                        } else {
                            Vec::new()
                        }
                    }
                    Freq::Weekly => {
                        let week_start = base_days
                            - (i64::from(weekday(base_days)) - i64::from(rule.wkst)).rem_euclid(7)
                            + step * 7;
                        let mut days: Vec<i64> = if rule.by_day.is_empty() {
                            vec![
                                week_start
                                    + (i64::from(weekday(base_days)) - i64::from(rule.wkst))
                                        .rem_euclid(7),
                            ]
                        } else {
                            rule.by_day
                                .iter()
                                .map(|&(_, wd)| {
                                    week_start
                                        + (i64::from(wd) - i64::from(rule.wkst)).rem_euclid(7)
                                })
                                .collect()
                        };
                        if !rule.by_month.is_empty() {
                            days.retain(|d| rule.by_month.contains(&civil_from_days(*d).1));
                        }
                        days.sort_unstable();
                        days.dedup();
                        days
                    }
                    Freq::Monthly => {
                        let index = base_year * 12 + i64::from(base_month) - 1 + step;
                        let (year, month) = (index.div_euclid(12), index.rem_euclid(12) as u32 + 1);
                        if rule.by_month.is_empty() || rule.by_month.contains(&month) {
                            month_dates(year, month, rule, base_day)
                        } else {
                            Vec::new()
                        }
                    }
                    Freq::Yearly => {
                        let year = base_year + step;
                        if rule.by_day.is_empty() || !rule.by_month.is_empty() {
                            let months: Vec<u32> = if rule.by_month.is_empty() {
                                vec![base_month]
                            } else {
                                let mut m = rule.by_month.clone();
                                m.sort_unstable();
                                m
                            };
                            months
                                .into_iter()
                                .flat_map(|m| month_dates(year, m, rule, base_day))
                                .collect()
                        } else {
                            Vec::new()
                        }
                    }
                };
                for day in dates {
                    if day < base_days {
                        continue;
                    }
                    if rule.count.is_some_and(|c| produced >= c) {
                        break 'periods;
                    }
                    produced += 1;
                    let (key, instance) = make_instance(event, ctx, day, secs, &span, true);
                    if let Some(until) = rule.until {
                        let past = match until.secs {
                            Some(_) => key > ctx.to_utc_ms(&until, event.start_tzid.as_deref()),
                            None => day > until.days,
                        };
                        if past {
                            break 'periods;
                        }
                    }
                    if instance.start_unix_ms >= window.to_unix_ms {
                        break 'periods;
                    }
                    push(key, instance, out);
                }
                if k == MAX_PERIODS - 1 {
                    out.unsupported_rules += 1;
                }
            }
        }
    }
    for (dt, tz) in &event.rdates {
        let tz = tz.as_deref().or(event.start_tzid.as_deref());
        let key_dt = Dt { utc: dt.utc, ..*dt };
        let (key, instance) = make_instance(
            &RawEvent {
                start: key_dt,
                start_tzid: tz.map(str::to_owned),
                ..clone_header(event)
            },
            ctx,
            dt.days,
            dt.secs.or(secs),
            &span,
            true,
        );
        push(key, instance, out);
    }
}

fn clone_header(event: &RawEvent) -> RawEvent {
    RawEvent {
        uid: event.uid.clone(),
        title: event.title.clone(),
        location: event.location.clone(),
        notes: event.notes.clone(),
        start: event.start,
        start_tzid: event.start_tzid.clone(),
        end: None,
        duration_ms: None,
        rrule: None,
        exdates: Vec::new(),
        rdates: Vec::new(),
        recurrence_id: None,
        cancelled: false,
    }
}

// ---------------------------------------------------------------------------------------------
// Neuen Termin schreiben
// ---------------------------------------------------------------------------------------------

/// Ein neuer Termin, wie ihn die Oberfläche übergibt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewEvent {
    pub title: String,
    pub all_day: bool,
    /// Beginn (UTC-Millisekunden); nur bei Terminen mit Uhrzeit.
    #[serde(default)]
    pub start_unix_ms: i64,
    /// Ende (UTC-Millisekunden, nach dem Beginn); nur bei Terminen mit Uhrzeit.
    #[serde(default)]
    pub end_unix_ms: i64,
    /// `YYYY-MM-DD`, erster Tag; nur bei Ganztagsterminen.
    #[serde(default)]
    pub start_date: Option<String>,
    /// `YYYY-MM-DD`, **letzter** Tag (einschließlich); nur bei Ganztagsterminen.
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Längster Titel in Zeichen.
pub const MAX_TITLE_CHARS: usize = 300;
/// Längster Termin: 31 Tage.
const MAX_SPAN_MS: i64 = 31 * MS_PER_DAY;

fn parse_iso_date(text: &str) -> Option<i64> {
    let mut parts = text.trim().split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (year, month, day): (i64, u32, u32) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    ((1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month))
        .then(|| days_from_civil(year, month, day))
}

fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

/// Faltet eine Zeile nach RFC 5545 auf höchstens 75 Byte (Folgezeilen beginnen mit einem Leerzeichen).
fn fold_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 8);
    let mut width = 0usize;
    for c in line.chars() {
        let size = c.len_utf8();
        if width + size > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += size;
    }
    out.push_str("\r\n");
    out
}

fn clean_single_line(text: &str, what: &str, max: usize) -> Result<String, FeedError> {
    let text = text.trim();
    if text.chars().any(char::is_control) {
        return Err(FeedError::Invalid(format!("{what} enthält Steuerzeichen")));
    }
    if text.chars().count() > max {
        return Err(FeedError::Invalid(format!(
            "{what} ist zu lang (höchstens {max} Zeichen)"
        )));
    }
    Ok(text.to_owned())
}

/// Baut den iCalendar-Text für einen neuen Termin.
///
/// Warum so streng: Der Text geht an einen fremden Server. Titel, Ort und Notiz werden auf
/// Steuerzeichen und Länge geprüft, damit keine zusätzlichen iCalendar-Zeilen (etwa ein `VALARM`
/// mit E-Mail-Aktion) eingeschleust werden können; Sonderzeichen werden maskiert.
///
/// # Errors
/// [`FeedError::Invalid`] bei leerem oder zu langem Titel, Steuerzeichen, ungültigem Datum, Ende vor
/// Beginn, Dauer über 31 Tagen oder ungültiger UID.
pub fn build_event_ics(uid: &str, event: &NewEvent, now_unix_ms: i64) -> Result<String, FeedError> {
    if uid.is_empty()
        || uid.len() > 100
        || !uid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
    {
        return Err(FeedError::Invalid("Ungültige Termin-Kennung".to_owned()));
    }
    let title = clean_single_line(&event.title, "Der Titel", MAX_TITLE_CHARS)?;
    if title.is_empty() {
        return Err(FeedError::Invalid("Der Titel fehlt".to_owned()));
    }
    let location = match event
        .location
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        Some(l) => Some(clean_single_line(l, "Der Ort", 300)?),
        None => None,
    };
    let notes = match event
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        Some(n) => {
            if n.chars().any(|c| c.is_control() && c != '\n' && c != '\r') {
                return Err(FeedError::Invalid(
                    "Die Notiz enthält Steuerzeichen".to_owned(),
                ));
            }
            if n.chars().count() > MAX_NOTES_CHARS {
                return Err(FeedError::Invalid(format!(
                    "Die Notiz ist zu lang (höchstens {MAX_NOTES_CHARS} Zeichen)"
                )));
            }
            Some(n.to_owned())
        }
        None => None,
    };

    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".to_owned(),
        "PRODID:-//IAP//Kalender//DE".to_owned(),
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{uid}"),
        format!("DTSTAMP:{}", ics_datetime_utc(now_unix_ms)),
    ];
    if event.all_day {
        let start = event
            .start_date
            .as_deref()
            .and_then(parse_iso_date)
            .ok_or_else(|| FeedError::Invalid("Das Startdatum ist ungültig".to_owned()))?;
        let last = event
            .end_date
            .as_deref()
            .and_then(parse_iso_date)
            .ok_or_else(|| FeedError::Invalid("Das Enddatum ist ungültig".to_owned()))?;
        if last < start {
            return Err(FeedError::Invalid(
                "Das Ende liegt vor dem Beginn".to_owned(),
            ));
        }
        if (last - start + 1) * MS_PER_DAY > MAX_SPAN_MS {
            return Err(FeedError::Invalid(
                "Ein Termin darf höchstens 31 Tage dauern".to_owned(),
            ));
        }
        lines.push(format!("DTSTART;VALUE=DATE:{}", ics_date(start)));
        lines.push(format!("DTEND;VALUE=DATE:{}", ics_date(last + 1)));
    } else {
        if event.end_unix_ms <= event.start_unix_ms {
            return Err(FeedError::Invalid(
                "Das Ende muss nach dem Beginn liegen".to_owned(),
            ));
        }
        if event.end_unix_ms - event.start_unix_ms > MAX_SPAN_MS {
            return Err(FeedError::Invalid(
                "Ein Termin darf höchstens 31 Tage dauern".to_owned(),
            ));
        }
        lines.push(format!("DTSTART:{}", ics_datetime_utc(event.start_unix_ms)));
        lines.push(format!("DTEND:{}", ics_datetime_utc(event.end_unix_ms)));
    }
    lines.push(format!("SUMMARY:{}", escape_text(&title)));
    if let Some(location) = location {
        lines.push(format!("LOCATION:{}", escape_text(&location)));
    }
    if let Some(notes) = notes {
        lines.push(format!("DESCRIPTION:{}", escape_text(&notes)));
    }
    lines.push("END:VEVENT".to_owned());
    lines.push("END:VCALENDAR".to_owned());
    Ok(lines.iter().map(|line| fold_line(line)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(y: i64, m: u32, d: u32, h: i64, min: i64) -> i64 {
        (days_from_civil(y, m, d) * 86_400 + h * 3600 + min * 60) * 1000
    }

    fn window(from: (i64, u32, u32), to: (i64, u32, u32)) -> Window {
        Window {
            from_unix_ms: ms(from.0, from.1, from.2, 0, 0),
            to_unix_ms: ms(to.0, to.1, to.2, 0, 0),
        }
    }

    fn feed(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}END:VCALENDAR\r\n")
    }

    const VIENNA: &str = "BEGIN:VTIMEZONE\r\nTZID:Europe/Vienna\r\n\
BEGIN:DAYLIGHT\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nTZNAME:CEST\r\n\
DTSTART:19700329T020000\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU\r\nEND:DAYLIGHT\r\n\
BEGIN:STANDARD\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nTZNAME:CET\r\n\
DTSTART:19701025T030000\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU\r\nEND:STANDARD\r\n\
END:VTIMEZONE\r\n";

    fn event(lines: &str) -> String {
        feed(&format!("BEGIN:VEVENT\r\nUID:u1\r\n{lines}END:VEVENT\r\n"))
    }

    fn starts(parsed: &Parsed) -> Vec<i64> {
        parsed.instances.iter().map(|i| i.start_unix_ms).collect()
    }

    #[test]
    fn single_utc_event_inside_the_window() {
        let text = event("SUMMARY:Zahnarzt\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T090000Z\r\nLOCATION:Linz\\, Hauptplatz\r\nDESCRIPTION:Zeile 1\\nZeile 2\r\n");
        let parsed = parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 60);
        assert_eq!(parsed.instances.len(), 1);
        let i = &parsed.instances[0];
        assert_eq!(i.title, "Zahnarzt");
        assert_eq!(i.start_unix_ms, ms(2026, 10, 5, 8, 0));
        assert_eq!(i.end_unix_ms, ms(2026, 10, 5, 9, 0));
        assert_eq!(i.location.as_deref(), Some("Linz, Hauptplatz"));
        assert_eq!(i.notes.as_deref(), Some("Zeile 1\nZeile 2"));
        assert!(!i.all_day && !i.recurring);
        assert_eq!(i.start_date, None);
    }

    #[test]
    fn events_outside_the_window_are_dropped_and_overlap_counts() {
        let text =
            event("SUMMARY:Nachts\r\nDTSTART:20260930T230000Z\r\nDTEND:20261001T010000Z\r\n");
        // Beginnt vor dem Fenster, reicht hinein: bleibt.
        assert_eq!(
            parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0)
                .instances
                .len(),
            1
        );
        // Endet genau am Fensteranfang: bleibt weg.
        let text = event("SUMMARY:Davor\r\nDTSTART:20260930T230000Z\r\nDTEND:20261001T000000Z\r\n");
        assert!(parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0)
            .instances
            .is_empty());
        // Beginnt genau am Fensterende: bleibt weg.
        let text =
            event("SUMMARY:Danach\r\nDTSTART:20261101T000000Z\r\nDTEND:20261101T010000Z\r\n");
        assert!(parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0)
            .instances
            .is_empty());
    }

    #[test]
    fn tzid_uses_the_vtimezone_for_summer_and_winter() {
        let body = format!(
            "{VIENNA}BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Sommer\r\nDTSTART;TZID=Europe/Vienna:20260701T090000\r\nDTEND;TZID=Europe/Vienna:20260701T100000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Winter\r\nDTSTART;TZID=Europe/Vienna:20261201T090000\r\nDTEND;TZID=Europe/Vienna:20261201T100000\r\nEND:VEVENT\r\n"
        );
        let parsed = parse_feed(&feed(&body), window((2026, 6, 1), (2027, 1, 1)), 999);
        assert_eq!(
            parsed.instances[0].start_unix_ms,
            ms(2026, 7, 1, 7, 0),
            "UTC+2"
        );
        assert_eq!(
            parsed.instances[1].start_unix_ms,
            ms(2026, 12, 1, 8, 0),
            "UTC+1"
        );
    }

    #[test]
    fn dst_switch_days_pick_the_right_side() {
        // Sommerzeit 2026: 29.3. 02:00 -> 03:00; Ende: 25.10. 03:00 -> 02:00.
        let zone = Zone {
            observances: read_components(VIENNA)
                .zones
                .remove("Europe/Vienna")
                .unwrap()
                .observances,
        };
        let local =
            |y: i64, m: u32, d: u32, h: i64| epoch_seconds(days_from_civil(y, m, d), h * 3600);
        assert_eq!(zone.offset_at_local(local(2026, 3, 29, 1)), 60);
        assert_eq!(zone.offset_at_local(local(2026, 3, 29, 4)), 120);
        assert_eq!(zone.offset_at_local(local(2026, 10, 25, 1)), 120);
        assert_eq!(zone.offset_at_local(local(2026, 10, 25, 4)), 60);
        assert_eq!(
            zone.offset_at_local(local(1999, 7, 1, 12)),
            120,
            "Regeln gelten rückwärts ab 1970"
        );
    }

    #[test]
    fn floating_time_and_unknown_tzid_use_the_machine_offset() {
        let text = event("SUMMARY:Lokal\r\nDTSTART:20261005T090000\r\nDTEND:20261005T100000\r\n");
        let parsed = parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 120);
        assert_eq!(parsed.instances[0].start_unix_ms, ms(2026, 10, 5, 7, 0));
        let text = event("SUMMARY:Unbekannt\r\nDTSTART;TZID=Mars/Olympus:20261005T090000\r\nDTEND;TZID=Mars/Olympus:20261005T100000\r\n");
        let parsed = parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 60);
        assert_eq!(parsed.instances[0].start_unix_ms, ms(2026, 10, 5, 8, 0));
    }

    #[test]
    fn all_day_events_carry_dates_and_an_exclusive_end() {
        let text = event(
            "SUMMARY:Feiertag\r\nDTSTART;VALUE=DATE:20261026\r\nDTEND;VALUE=DATE:20261027\r\n",
        );
        let parsed = parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 60);
        let i = &parsed.instances[0];
        assert!(i.all_day);
        assert_eq!(i.start_date.as_deref(), Some("2026-10-26"));
        assert_eq!(i.end_date.as_deref(), Some("2026-10-27"));
        // Ohne DTEND: ein Tag. Dreitägig: Ende exklusiv.
        let text =
            event("SUMMARY:Urlaub\r\nDTSTART;VALUE=DATE:20261012\r\nDTEND;VALUE=DATE:20261015\r\n");
        let i = &parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0).instances[0];
        assert_eq!(i.end_date.as_deref(), Some("2026-10-15"));
        let text = event("SUMMARY:Ein Tag\r\nDTSTART;VALUE=DATE:20261012\r\n");
        let i = &parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0).instances[0];
        assert_eq!(i.end_date.as_deref(), Some("2026-10-13"));
    }

    #[test]
    fn duration_replaces_dtend() {
        let text = event("SUMMARY:D\r\nDTSTART:20261005T080000Z\r\nDURATION:PT1H30M\r\n");
        let i = &parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0).instances[0];
        assert_eq!(i.end_unix_ms - i.start_unix_ms, 90 * 60_000);
        assert_eq!(parse_duration_ms("P1W"), Some(7 * MS_PER_DAY));
        assert_eq!(
            parse_duration_ms("P2DT3H"),
            Some(2 * MS_PER_DAY + 3 * 3_600_000)
        );
        assert_eq!(parse_duration_ms("PT"), Some(0));
        assert_eq!(parse_duration_ms("garbage"), None);
        assert_eq!(parse_duration_ms("P1H"), None, "Stunden brauchen das T");
    }

    #[test]
    fn weekly_series_keeps_the_wall_clock_across_the_dst_end() {
        // Montags 09:00 Wien ab 19.10.; Zeitumstellung am 25.10.
        let body = format!(
            "{VIENNA}BEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Montag\r\nDTSTART;TZID=Europe/Vienna:20261019T090000\r\nDTEND;TZID=Europe/Vienna:20261019T100000\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nEND:VEVENT\r\n"
        );
        let parsed = parse_feed(&feed(&body), window((2026, 10, 1), (2026, 12, 1)), 0);
        assert_eq!(
            starts(&parsed),
            [
                ms(2026, 10, 19, 7, 0),
                ms(2026, 10, 26, 8, 0),
                ms(2026, 11, 2, 8, 0)
            ]
        );
        assert!(parsed.instances.iter().all(|i| i.recurring));
    }

    #[test]
    fn weekly_byday_interval_and_until() {
        // Mo+Mi, alle 2 Wochen, ab Mo 5.10.2026 bis einschließlich 21.10.
        let text = event("SUMMARY:Kurs\r\nDTSTART:20261005T100000Z\r\nDTEND:20261005T110000Z\r\nRRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;UNTIL=20261021T235959Z\r\n");
        let parsed = parse_feed(&text, window((2026, 10, 1), (2027, 1, 1)), 0);
        assert_eq!(
            starts(&parsed),
            [
                ms(2026, 10, 5, 10, 0),
                ms(2026, 10, 7, 10, 0),
                ms(2026, 10, 19, 10, 0),
                ms(2026, 10, 21, 10, 0)
            ]
        );
    }

    #[test]
    fn weekly_respects_wkst_for_interval_two() {
        // So+Mo mit WKST=SU: Woche beginnt Sonntag; Start Montag 5.10.: gleiche Woche enthält So 4.10. (davor, übersprungen).
        let text = event("SUMMARY:W\r\nDTSTART:20261005T100000Z\r\nDTEND:20261005T110000Z\r\nRRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=SU,MO;WKST=SU;COUNT=3\r\n");
        let parsed = parse_feed(&text, window((2026, 10, 1), (2027, 1, 1)), 0);
        // Woche 1 (So 4.10.–Sa 10.10.): nur Mo 5.10. (So 4.10. liegt vor dem Start).
        // Woche 3 (So 18.10.): So 18.10. und Mo 19.10.
        assert_eq!(
            starts(&parsed),
            [
                ms(2026, 10, 5, 10, 0),
                ms(2026, 10, 18, 10, 0),
                ms(2026, 10, 19, 10, 0)
            ]
        );
    }

    #[test]
    fn daily_series_with_count_and_exdate() {
        let text = event("SUMMARY:Täglich\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T083000Z\r\nRRULE:FREQ=DAILY;COUNT=4\r\nEXDATE:20261006T080000Z\r\n");
        let parsed = parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0);
        assert_eq!(
            starts(&parsed),
            [
                ms(2026, 10, 5, 8, 0),
                ms(2026, 10, 7, 8, 0),
                ms(2026, 10, 8, 8, 0)
            ],
            "EXDATE zählt für COUNT mit, fehlt aber im Ergebnis"
        );
    }

    #[test]
    fn monthly_rules_nth_weekday_last_weekday_and_month_end() {
        let w = window((2026, 10, 1), (2027, 3, 1));
        // Zweiter Dienstag.
        let text = event("SUMMARY:A\r\nDTSTART:20261013T100000Z\r\nDTEND:20261013T110000Z\r\nRRULE:FREQ=MONTHLY;BYDAY=2TU;COUNT=3\r\n");
        assert_eq!(
            starts(&parse_feed(&text, w, 0)),
            [
                ms(2026, 10, 13, 10, 0),
                ms(2026, 11, 10, 10, 0),
                ms(2026, 12, 8, 10, 0)
            ]
        );
        // Letzter Freitag.
        let text = event("SUMMARY:B\r\nDTSTART:20261030T100000Z\r\nDTEND:20261030T110000Z\r\nRRULE:FREQ=MONTHLY;BYDAY=-1FR;COUNT=3\r\n");
        assert_eq!(
            starts(&parse_feed(&text, w, 0)),
            [
                ms(2026, 10, 30, 10, 0),
                ms(2026, 11, 27, 10, 0),
                ms(2026, 12, 25, 10, 0)
            ]
        );
        // Der 31.: Monate ohne 31. werden übersprungen.
        let text = event("SUMMARY:C\r\nDTSTART:20261031T100000Z\r\nDTEND:20261031T110000Z\r\nRRULE:FREQ=MONTHLY;COUNT=3\r\n");
        assert_eq!(
            starts(&parse_feed(&text, w, 0)),
            [
                ms(2026, 10, 31, 10, 0),
                ms(2026, 12, 31, 10, 0),
                ms(2027, 1, 31, 10, 0)
            ]
        );
        // Letzter Tag des Monats.
        let text = event("SUMMARY:D\r\nDTSTART:20261031T100000Z\r\nDTEND:20261031T110000Z\r\nRRULE:FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=3\r\n");
        assert_eq!(
            starts(&parse_feed(&text, w, 0)),
            [
                ms(2026, 10, 31, 10, 0),
                ms(2026, 11, 30, 10, 0),
                ms(2026, 12, 31, 10, 0)
            ]
        );
    }

    #[test]
    fn yearly_series_including_leap_day() {
        let w = window((2027, 1, 1), (2037, 1, 1));
        let text = event(
            "SUMMARY:Geburtstag\r\nDTSTART;VALUE=DATE:20240229\r\nRRULE:FREQ=YEARLY;COUNT=4\r\n",
        );
        let parsed = parse_feed(&text, w, 0);
        let dates: Vec<_> = parsed
            .instances
            .iter()
            .map(|i| i.start_date.clone().unwrap())
            .collect();
        // 2024 ist vor dem Fenster; 2025–2027 haben keinen 29.2.; COUNT zählt nur gültige Tage.
        assert_eq!(dates, ["2028-02-29", "2032-02-29", "2036-02-29"]);
        let text = event("SUMMARY:Jahrestag\r\nDTSTART;VALUE=DATE:20260314\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYMONTHDAY=14\r\n");
        let parsed = parse_feed(&text, window((2026, 1, 1), (2029, 1, 1)), 0);
        assert_eq!(parsed.instances.len(), 3);
    }

    #[test]
    fn long_running_series_start_before_the_window_without_losing_count() {
        // Täglich seit 2020, nur 5 Vorkommen: alle liegen vor dem Fenster.
        let text = event("SUMMARY:Alt\r\nDTSTART:20200101T080000Z\r\nDTEND:20200101T090000Z\r\nRRULE:FREQ=DAILY;COUNT=5\r\n");
        assert!(parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0)
            .instances
            .is_empty());
        // Ohne Ende: im Fenster sind genau die Tage des Fensters.
        let text = event("SUMMARY:Ewig\r\nDTSTART:20200101T080000Z\r\nDTEND:20200101T090000Z\r\nRRULE:FREQ=DAILY\r\n");
        let parsed = parse_feed(&text, window((2026, 10, 1), (2026, 10, 8)), 0);
        assert_eq!(parsed.instances.len(), 7);
        assert_eq!(parsed.instances[0].start_unix_ms, ms(2026, 10, 1, 8, 0));
    }

    #[test]
    fn changed_and_cancelled_occurrences_replace_the_generated_ones() {
        let text = feed(
            "BEGIN:VEVENT\r\nUID:s\r\nSUMMARY:Serie\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T090000Z\r\nRRULE:FREQ=DAILY;COUNT=4\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:s\r\nRECURRENCE-ID:20261006T080000Z\r\nSUMMARY:Verschoben\r\nDTSTART:20261006T140000Z\r\nDTEND:20261006T150000Z\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:s\r\nRECURRENCE-ID:20261007T080000Z\r\nSTATUS:CANCELLED\r\nSUMMARY:Serie\r\nDTSTART:20261007T080000Z\r\nDTEND:20261007T090000Z\r\nEND:VEVENT\r\n",
        );
        let parsed = parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0);
        assert!(
            parsed.instances.iter().all(|i| i.recurring),
            "auch der verschobene Termin gehört zur Serie"
        );
        let got: Vec<_> = parsed
            .instances
            .iter()
            .map(|i| (i.title.as_str(), i.start_unix_ms))
            .collect();
        assert_eq!(
            got,
            [
                ("Serie", ms(2026, 10, 5, 8, 0)),
                ("Verschoben", ms(2026, 10, 6, 14, 0)),
                ("Serie", ms(2026, 10, 8, 8, 0)),
            ]
        );
    }

    #[test]
    fn rdate_adds_an_extra_occurrence() {
        let text = event("SUMMARY:Extra\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T090000Z\r\nRDATE:20261012T080000Z\r\n");
        let parsed = parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0);
        assert_eq!(
            starts(&parsed),
            [ms(2026, 10, 5, 8, 0), ms(2026, 10, 12, 8, 0)]
        );
        assert_eq!(
            parsed.instances[1].end_unix_ms - parsed.instances[1].start_unix_ms,
            3_600_000
        );
    }

    #[test]
    fn unsupported_rules_are_counted_not_guessed() {
        let w = window((2026, 10, 1), (2026, 12, 1));
        for rule in [
            "FREQ=HOURLY;COUNT=5",
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
            "FREQ=YEARLY;BYDAY=MO",
            "FREQ=WEEKLY;BYWEEKNO=3",
            "COUNT=3",
            "FREQ=DAILY;BYMONTHDAY=1",
        ] {
            let text = event(&format!("SUMMARY:X\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T090000Z\r\nRRULE:{rule}\r\n"));
            let parsed = parse_feed(&text, w, 0);
            assert_eq!(parsed.unsupported_rules, 1, "{rule}");
            assert_eq!(
                starts(&parsed),
                [ms(2026, 10, 5, 8, 0)],
                "{rule}: nur der erste Termin"
            );
        }
    }

    #[test]
    fn alarms_do_not_leak_into_the_event_and_broken_events_are_counted() {
        let text = feed(
            "BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Echt\r\nDESCRIPTION:Echte Notiz\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T090000Z\r\n\
BEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:ALARM\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Ohne Beginn\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:c\r\nSUMMARY:Kaputt\r\nDTSTART:2026-10-05\r\nEND:VEVENT\r\n",
        );
        let parsed = parse_feed(&text, window((2026, 10, 1), (2026, 11, 1)), 0);
        assert_eq!(parsed.instances.len(), 1);
        assert_eq!(parsed.instances[0].notes.as_deref(), Some("Echte Notiz"));
        assert_eq!(parsed.skipped_events, 2);
    }

    #[test]
    fn folded_lines_quoted_params_and_blank_titles() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\nSUMMARY:Ein sehr lan\r\n ger Titel\r\nLOCATION;ALTREP=\"http://a.example/b:c\":Ort\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T090000Z\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:y\r\nDTSTART:20261006T080000Z\r\nDTEND:20261006T090000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let parsed = parse_feed(text, window((2026, 10, 1), (2026, 11, 1)), 0);
        assert_eq!(parsed.instances[0].title, "Ein sehr langer Titel");
        assert_eq!(parsed.instances[0].location.as_deref(), Some("Ort"));
        assert_eq!(parsed.instances[1].title, "(ohne Titel)");
        // LF statt CRLF, Kleinbuchstaben in Namen.
        let text = "begin:vcalendar\nbegin:vevent\nuid:z\nsummary:LF\ndtstart:20261005T080000Z\ndtend:20261005T090000Z\nend:vevent\nend:vcalendar\n";
        assert_eq!(
            parse_feed(text, window((2026, 10, 1), (2026, 11, 1)), 0)
                .instances
                .len(),
            1
        );
    }

    #[test]
    fn garbage_input_never_panics() {
        for text in [
            "",
            "BEGIN:VEVENT",
            "END:VEVENT\r\nEND:VTIMEZONE",
            "\u{0}\u{1}:::;;;",
            "BEGIN:VEVENT\r\nDTSTART:99999999T999999\r\nEND:VEVENT",
        ] {
            let _ = parse_feed(text, window((2026, 10, 1), (2026, 11, 1)), 0);
        }
    }

    #[test]
    fn a_series_is_capped() {
        let text = event("SUMMARY:Viele\r\nDTSTART:20200101T080000Z\r\nDTEND:20200101T080500Z\r\nRRULE:FREQ=DAILY\r\n");
        let parsed = parse_feed(&text, window((2020, 1, 1), (2030, 1, 1)), 0);
        assert_eq!(parsed.instances.len(), MAX_INSTANCES_PER_SERIES);
    }

    fn new_event() -> NewEvent {
        NewEvent {
            title: "Besprechung, wichtig; bitte".to_owned(),
            all_day: false,
            start_unix_ms: ms(2026, 10, 5, 8, 0),
            end_unix_ms: ms(2026, 10, 5, 9, 30),
            start_date: None,
            end_date: None,
            location: Some("Raum 2".to_owned()),
            notes: Some("Unterlagen\nmitbringen".to_owned()),
        }
    }

    #[test]
    fn new_events_are_written_in_crlf_with_escaping_and_survive_a_round_trip() {
        let ics = build_event_ics("iap-1@iap.local", &new_event(), ms(2026, 10, 2, 12, 0)).unwrap();
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\n") && ics.ends_with("END:VCALENDAR\r\n"));
        assert!(ics.contains("UID:iap-1@iap.local\r\n"));
        assert!(ics.contains("DTSTAMP:20261002T120000Z\r\n"));
        assert!(ics.contains("SUMMARY:Besprechung\\, wichtig\\; bitte\r\n"));
        assert!(ics.contains("DESCRIPTION:Unterlagen\\nmitbringen\r\n"));
        assert!(!ics.contains("\n\n") && !ics.replace("\r\n", "").contains('\n'));
        let parsed = parse_feed(&ics, window((2026, 10, 1), (2026, 11, 1)), 0);
        let i = &parsed.instances[0];
        assert_eq!(i.title, "Besprechung, wichtig; bitte");
        assert_eq!(i.start_unix_ms, ms(2026, 10, 5, 8, 0));
        assert_eq!(i.end_unix_ms, ms(2026, 10, 5, 9, 30));
        assert_eq!(i.location.as_deref(), Some("Raum 2"));
        assert_eq!(i.notes.as_deref(), Some("Unterlagen\nmitbringen"));
    }

    #[test]
    fn new_all_day_events_use_value_date_with_an_exclusive_end() {
        let mut event = new_event();
        event.all_day = true;
        event.start_date = Some("2026-10-12".to_owned());
        event.end_date = Some("2026-10-14".to_owned());
        let ics = build_event_ics("u@v", &event, 0).unwrap();
        assert!(ics.contains("DTSTART;VALUE=DATE:20261012\r\n"));
        assert!(ics.contains("DTEND;VALUE=DATE:20261015\r\n"));
        let parsed = parse_feed(&ics, window((2026, 10, 1), (2026, 11, 1)), 60);
        let i = &parsed.instances[0];
        assert_eq!(i.start_date.as_deref(), Some("2026-10-12"));
        assert_eq!(i.end_date.as_deref(), Some("2026-10-15"));
    }

    #[test]
    fn long_lines_are_folded_at_75_bytes_without_splitting_characters() {
        let mut event = new_event();
        event.title = "Ä".repeat(120);
        let ics = build_event_ics("u@v", &event, 0).unwrap();
        for line in ics.split("\r\n") {
            assert!(line.len() <= 75, "{} Byte: {line}", line.len());
        }
        let parsed = parse_feed(&ics, window((2026, 10, 1), (2026, 11, 1)), 0);
        assert_eq!(parsed.instances[0].title, "Ä".repeat(120));
    }

    #[test]
    fn new_events_reject_injection_and_nonsense() {
        let ok = new_event();
        let bad = |change: fn(&mut NewEvent)| {
            let mut e = ok.clone();
            change(&mut e);
            build_event_ics("u@v", &e, 0)
        };
        assert!(bad(|e| e.title = "  ".into()).is_err());
        assert!(bad(|e| e.title = "A\r\nBEGIN:VALARM".into()).is_err());
        assert!(bad(|e| e.title = "x".repeat(MAX_TITLE_CHARS + 1)).is_err());
        assert!(bad(|e| e.location = Some("Ort\nEND:VEVENT".into())).is_err());
        assert!(bad(|e| e.notes = Some("a\u{0007}b".into())).is_err());
        assert!(bad(|e| e.end_unix_ms = e.start_unix_ms).is_err());
        assert!(bad(|e| e.end_unix_ms = e.start_unix_ms + 32 * MS_PER_DAY).is_err());
        assert!(bad(|e| {
            e.all_day = true;
            e.start_date = None;
        })
        .is_err());
        assert!(bad(|e| {
            e.all_day = true;
            e.start_date = Some("2026-02-30".into());
            e.end_date = Some("2026-03-01".into());
        })
        .is_err());
        assert!(bad(|e| {
            e.all_day = true;
            e.start_date = Some("2026-10-05".into());
            e.end_date = Some("2026-10-04".into());
        })
        .is_err());
        for uid in ["", "a b", "a\r\nb", "ä", &"x".repeat(101)] {
            assert!(build_event_ics(uid, &ok, 0).is_err(), "{uid:?}");
        }
        // Newlines in der Notiz sind erlaubt und werden maskiert, Windows-Zeilenenden ebenso.
        assert!(bad(|e| e.notes = Some("a\r\nb".into())).is_ok());
    }
}
