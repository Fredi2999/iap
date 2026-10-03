//! Datumsrechnung im proleptischen gregorianischen Kalender, ohne Zeitbibliothek.
//!
//! Warum selbst geschrieben: `chrono`/`time` fehlen im Offline-Vendor-Cache (siehe Modul-Doc in
//! `lib.rs`), und der Feed-Parser braucht nur Tage, Wochentage und Monatslängen. Tage zählen ab
//! 1970-01-01 (Tag 0), Sekunden und Millisekunden ebenso.

/// Tage seit 1970-01-01 für ein Datum (Algorithmus von Howard Hinnant, gültig für alle Jahre).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Datum `(Jahr, Monat 1..=12, Tag 1..=31)` zu einem Tag seit 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { y + 1 } else { y }, month, day)
}

/// Wochentag eines Tages, Montag = 0 … Sonntag = 6 (ISO).
pub fn weekday(days: i64) -> u8 {
    // 1970-01-01 war ein Donnerstag (3).
    (days + 3).rem_euclid(7) as u8
}

/// Ob `year` ein Schaltjahr ist.
pub fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Anzahl der Tage im Monat.
pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

/// Der `n`-te Wochentag `wd` eines Monats (`n > 0` von vorn, `n < 0` von hinten, `-1` = letzter).
/// `None`, wenn es ihn in diesem Monat nicht gibt (etwa ein fünfter Montag).
pub fn nth_weekday_of_month(year: i64, month: u32, wd: u8, n: i32) -> Option<i64> {
    if n == 0 {
        return None;
    }
    let first = days_from_civil(year, month, 1);
    let len = i64::from(days_in_month(year, month));
    let offset = (i64::from(wd) - i64::from(weekday(first))).rem_euclid(7);
    let day = if n > 0 {
        offset + 7 * i64::from(n - 1)
    } else {
        let last_offset = (i64::from(wd) - i64::from(weekday(first + len - 1))).rem_euclid(7);
        // Letzter passender Tag liegt `(7 - last_offset) % 7` Tage vor dem Monatsende.
        let back = (7 - last_offset) % 7;
        len - 1 - back - 7 * i64::from(-n - 1)
    };
    (0..len).contains(&day).then_some(first + day)
}

/// Sekunden seit 1970-01-01 für Datum und Uhrzeit, als wäre beides UTC.
pub fn epoch_seconds(days: i64, seconds_of_day: i64) -> i64 {
    days * 86_400 + seconds_of_day
}

/// `YYYY-MM-DD` für einen Tag.
pub fn iso_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYYMMDD` für einen Tag (iCalendar `VALUE=DATE`).
pub fn ics_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}")
}

/// `YYYYMMDDTHHMMSSZ` für Unix-Millisekunden (UTC).
pub fn ics_datetime_utc(unix_ms: i64) -> String {
    let secs = unix_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    format!(
        "{}T{:02}{:02}{:02}Z",
        ics_date(days),
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_dates_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2026, 10, 2), 20_728);
        assert_eq!(civil_from_days(20_728), (2026, 10, 2));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        for days in (-800_000..800_000).step_by(997) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn weekdays_are_iso_monday_zero() {
        assert_eq!(weekday(0), 3, "1970-01-01 war ein Donnerstag");
        assert_eq!(weekday(days_from_civil(2026, 10, 2)), 4, "Freitag");
        assert_eq!(weekday(days_from_civil(2026, 10, 5)), 0, "Montag");
        assert_eq!(weekday(days_from_civil(1969, 12, 31)), 2, "Mittwoch");
    }

    #[test]
    fn month_lengths_follow_leap_rules() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    #[test]
    fn nth_weekday_counts_from_both_ends() {
        // Oktober 2026: Fr 2., 9., 16., 23., 30.; Mo 5., 12., 19., 26.
        let d = |day| days_from_civil(2026, 10, day);
        assert_eq!(nth_weekday_of_month(2026, 10, 4, 1), Some(d(2)));
        assert_eq!(nth_weekday_of_month(2026, 10, 4, 5), Some(d(30)));
        assert_eq!(nth_weekday_of_month(2026, 10, 4, -1), Some(d(30)));
        assert_eq!(nth_weekday_of_month(2026, 10, 4, -2), Some(d(23)));
        assert_eq!(nth_weekday_of_month(2026, 10, 0, 1), Some(d(5)));
        assert_eq!(
            nth_weekday_of_month(2026, 10, 0, 5),
            None,
            "kein fünfter Montag"
        );
        assert_eq!(nth_weekday_of_month(2026, 10, 0, -1), Some(d(26)));
        // Letzter Sonntag im März 2026 (Beginn der Sommerzeit): 29.
        assert_eq!(
            nth_weekday_of_month(2026, 3, 6, -1),
            Some(days_from_civil(2026, 3, 29))
        );
        assert_eq!(nth_weekday_of_month(2026, 10, 4, 0), None);
    }

    #[test]
    fn dates_format_for_ics_and_iso() {
        assert_eq!(iso_date(20_728), "2026-10-02");
        assert_eq!(ics_date(20_728), "20261002");
        // 2026-10-02T07:30:15Z
        let ms = (20_728 * 86_400 + 7 * 3600 + 30 * 60 + 15) * 1000;
        assert_eq!(ics_datetime_utc(ms), "20261002T073015Z");
        assert_eq!(ics_datetime_utc(0), "19700101T000000Z");
    }
}
