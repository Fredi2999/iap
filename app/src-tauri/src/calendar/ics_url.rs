//! Google-Kalender über die „geheime Adresse im iCal-Format“.
//!
//! Google zeigt sie in den Kalender-Einstellungen unter „Integrieren“. Sie braucht keine
//! Anmeldung bei Google Cloud und liefert den Kalender nur zum **Lesen**. Wer die Adresse kennt,
//! kann den Kalender lesen; deshalb liegt sie wie ein Passwort im Tresor und geht nie an die
//! Oberfläche zurück. Der einzige erlaubte Server ist `calendar.google.com`.

use std::time::Duration;

use pa_policy::egress::Connector;
use pa_scheduler::feed::{self, Parsed, Window};

use super::CalError;
use crate::net::{self, Method, Request, Transport};

/// Server der geheimen Adresse.
pub const GOOGLE_ICS_HOST: &str = "calendar.google.com";
const MAX_URL_CHARS: usize = 600;
const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(45);

/// Zerlegt die geheime Adresse in den Pfad auf `calendar.google.com`.
///
/// Erlaubt ist genau `https://calendar.google.com/calendar/ical/…/basic.ics` ohne Abfrage-Teil.
/// Alles andere wird abgelehnt, damit hier keine beliebige Adresse eingetragen werden kann.
///
/// # Errors
/// [`CalError::Invalid`] mit einem Hinweis, was erwartet wird.
pub fn parse_url(url: &str) -> Result<String, CalError> {
    let invalid = || {
        CalError::Invalid(
            "Das ist keine geheime iCal-Adresse von Google. Sie beginnt mit https://calendar.google.com/calendar/ical/ und endet auf .ics."
                .to_owned(),
        )
    };
    let url = url.trim();
    if url.chars().count() > MAX_URL_CHARS
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(invalid());
    }
    let rest = url.strip_prefix("https://").ok_or_else(invalid)?;
    let path = rest
        .strip_prefix(GOOGLE_ICS_HOST)
        .filter(|p| p.starts_with('/'))
        .ok_or_else(invalid)?;
    if !path.starts_with("/calendar/ical/")
        || !path.ends_with(".ics")
        || path.contains(['?', '#'])
        || path.contains("..")
        || path.contains("//")
    {
        return Err(invalid());
    }
    Ok(path.to_owned())
}

/// Ruft den Kalender ab und liest die Termine im Zeitfenster.
///
/// # Errors
/// [`CalError`] bei gesperrtem Netz, Netzfehler, falscher Adresse (404) oder unlesbarer Antwort.
pub fn fetch(
    transport: &dyn Transport,
    path: &str,
    window: Window,
    local_offset_minutes: i32,
) -> Result<Parsed, CalError> {
    let response = net::request_with(
        transport,
        &Request {
            connector: Connector::Calendar,
            host: GOOGLE_ICS_HOST,
            method: Method::Get,
            path,
            query: &[],
            headers: &[("Accept", "text/calendar")],
            body: &[],
            timeout: TIMEOUT,
            max_response: MAX_RESPONSE,
        },
    )?;
    match response.status {
        200 => {}
        404 => return Err(CalError::Protocol(
            "Die Adresse wurde nicht gefunden. Wurde die geheime Adresse in Google zurückgesetzt?"
                .to_owned(),
        )),
        status => return Err(CalError::Rejected(status)),
    }
    let text = String::from_utf8_lossy(&response.body);
    if !text.contains("BEGIN:VCALENDAR") {
        return Err(CalError::Protocol(
            "Die Antwort enthält keine Kalenderdaten".to_owned(),
        ));
    }
    Ok(feed::parse_feed(&text, window, local_offset_minutes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::caldav::tests::Scripted;

    const URL: &str = "https://calendar.google.com/calendar/ical/anna%40gmail.com/private-0123456789abcdef0123456789abcdef/basic.ics";

    #[test]
    fn only_the_secret_ical_address_on_google_passes() {
        assert_eq!(
            parse_url(&format!("  {URL}\n")).unwrap(),
            "/calendar/ical/anna%40gmail.com/private-0123456789abcdef0123456789abcdef/basic.ics"
        );
        assert!(parse_url("https://calendar.google.com/calendar/ical/x/public/basic.ics").is_ok());
        for bad in [
            "",
            "http://calendar.google.com/calendar/ical/x/basic.ics",
            "https://calendar.google.com.evil.example/calendar/ical/x/basic.ics",
            "https://evil.example/calendar/ical/x/basic.ics",
            "https://calendar.google.com/calendar/u/0/r",
            "https://calendar.google.com/calendar/ical/x/basic.ics?a=b",
            "https://calendar.google.com/calendar/ical/x/basic.ics#f",
            "https://calendar.google.com/calendar/ical/../x/basic.ics",
            "https://calendar.google.com/calendar/ical//x/basic.ics",
            "https://calendar.google.com/calendar/ical/x/basic.html",
            "https://calendar.google.com:8443/calendar/ical/x/basic.ics",
            "https://calendar.google.com@evil.example/calendar/ical/x/basic.ics",
            "https://calendar.google.com/calendar/ical/x y/basic.ics",
            "file:///calendar/ical/x/basic.ics",
        ] {
            assert!(parse_url(bad).is_err(), "{bad}");
        }
        assert!(
            parse_url(&format!("{URL}{}", "a".repeat(700))).is_err(),
            "zu lang"
        );
    }

    fn window() -> Window {
        let day = |d: u32| (pa_scheduler::civil::days_from_civil(2026, 10, d) * 86_400) * 1000;
        Window {
            from_unix_ms: day(1),
            to_unix_ms: day(31),
        }
    }

    #[test]
    fn fetch_gets_the_fixed_host_and_reads_the_feed() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:g1\r\nSUMMARY:Elternabend\r\nDTSTART:20261008T163000Z\r\nDTEND:20261008T180000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let fake = Scripted::new(&[(200, ics)]);
        let path = parse_url(URL).unwrap();
        let parsed = fetch(&fake, &path, window(), 60).unwrap();
        assert_eq!(parsed.instances.len(), 1);
        assert_eq!(parsed.instances[0].title, "Elternabend");
        let seen = fake.seen.lock().unwrap();
        let (host, method, target, headers, body) = &seen[0];
        assert_eq!(
            (host.as_str(), *method, target.as_str()),
            (GOOGLE_ICS_HOST, Method::Get, path.as_str())
        );
        assert!(
            headers.iter().all(|(k, _)| k != "Authorization"),
            "keine Anmeldung nötig"
        );
        assert!(body.is_empty());
    }

    #[test]
    fn statuses_and_foreign_content_become_clear_errors() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        let path = parse_url(URL).unwrap();
        assert!(
            matches!(fetch(&Scripted::new(&[(404, "")]), &path, window(), 0), Err(CalError::Protocol(m)) if m.contains("zurückgesetzt"))
        );
        assert!(matches!(
            fetch(&Scripted::new(&[(403, "")]), &path, window(), 0),
            Err(CalError::Rejected(403))
        ));
        assert!(matches!(
            fetch(
                &Scripted::new(&[(200, "<html>Login</html>")]),
                &path,
                window(),
                0
            ),
            Err(CalError::Protocol(_))
        ));
    }

    #[test]
    fn the_air_gap_blocks_before_anything_is_sent() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(true);
        let fake = Scripted::new(&[]);
        let path = parse_url(URL).unwrap();
        assert!(
            matches!(fetch(&fake, &path, window(), 0), Err(CalError::Blocked(m)) if m.contains("Air Gap"))
        );
        assert!(fake.seen.lock().unwrap().is_empty());
        net::set_air_gap(false);
    }
}
