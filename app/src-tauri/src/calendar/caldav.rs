//! CalDAV gegen Apple iCloud: Kalender finden, Termine abrufen, einen Termin anlegen.
//!
//! Anmeldung mit Apple-ID und **app-spezifischem Passwort** (HTTP Basic über TLS; Apple bietet für
//! CalDAV kein OAuth an). Alle Anfragen gehen über `net::request_with` und damit durch die feste
//! Host-Tabelle. Adressen, die der Server in Antworten nennt (Kalender-Heimat auf `pNN-caldav…`),
//! werden hier geprüft und dann ohne Weiterleitung angesprochen.

use std::time::Duration;

use pa_policy::egress::Connector;
use pa_scheduler::{
    civil::ics_datetime_utc,
    feed::{self, Parsed, Window},
};

use super::{xml, CalError};
use crate::mail::codec::base64_encode;
use crate::net::{self, Method, Request, Transport};

/// Wurzel der iCloud-Anmeldung. Von hier aus wird die eigene Server-Gruppe erfragt.
pub const ICLOUD_ROOT_HOST: &str = "caldav.icloud.com";
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_DISCOVERY: usize = 1024 * 1024;
const MAX_EVENTS_RESPONSE: usize = 16 * 1024 * 1024;

/// Apple-ID und app-spezifisches Passwort.
pub struct Credentials<'a> {
    pub user: &'a str,
    pub password: &'a str,
}

/// Ein gefundener Kalender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub host: String,
    pub href: String,
    pub name: String,
    pub color: Option<String>,
    pub can_write: bool,
}

/// Ziel einer Abfrage: Server und Pfad eines Kalenders.
pub struct Target<'a> {
    pub host: &'a str,
    pub href: &'a str,
}

fn auth_header(credentials: &Credentials<'_>) -> String {
    format!(
        "Basic {}",
        base64_encode(format!("{}:{}", credentials.user, credentials.password).as_bytes())
    )
}

/// Prüft einen Pfad, bevor er an den Netz-Client geht (der Client prüft zusätzlich).
fn safe_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains("..")
        && !path
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '?' | '#'))
}

/// Zerlegt eine `href` aus einer Serverantwort in (Server, Pfad). Relative Pfade gehören zum
/// bisherigen Server. Fremde Server, andere Ports und Nicht-HTTPS werden abgelehnt.
fn resolve_href(current_host: &str, href: &str) -> Result<(String, String), CalError> {
    let href = href.trim();
    let (host, path) = if let Some(rest) = href.strip_prefix("https://") {
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let host = authority.strip_suffix(":443").unwrap_or(authority);
        (host.to_ascii_lowercase(), path.to_owned())
    } else if href.starts_with('/') {
        (current_host.to_owned(), href.to_owned())
    } else {
        return Err(CalError::Protocol("Unbekanntes Adressformat".to_owned()));
    };
    if !Connector::Calendar.allows_host(&host) {
        return Err(CalError::Blocked(format!(
            "Der Server nennt eine Adresse außerhalb der erlaubten Hosts ({host})"
        )));
    }
    if !safe_path(&path) {
        return Err(CalError::Protocol(
            "Ungültiger Pfad in der Antwort".to_owned(),
        ));
    }
    Ok((host, path))
}

struct Reply {
    status: u16,
    body: String,
}

#[allow(clippy::too_many_arguments)]
fn call(
    transport: &dyn Transport,
    credentials: &Credentials<'_>,
    host: &str,
    method: Method,
    path: &str,
    extra_headers: &[(&str, &str)],
    body: &str,
    max_response: usize,
) -> Result<Reply, CalError> {
    let auth = auth_header(credentials);
    let mut headers: Vec<(&str, &str)> = vec![("Authorization", auth.as_str())];
    headers.extend_from_slice(extra_headers);
    let response = net::request_with(
        transport,
        &Request {
            connector: Connector::Calendar,
            host,
            method,
            path,
            query: &[],
            headers: &headers,
            body: body.as_bytes(),
            timeout: TIMEOUT,
            max_response,
        },
    )?;
    Ok(Reply {
        status: response.status,
        body: String::from_utf8_lossy(&response.body).into_owned(),
    })
}

fn check(reply: &Reply, ok: &[u16]) -> Result<(), CalError> {
    if ok.contains(&reply.status) {
        return Ok(());
    }
    match reply.status {
        401 => Err(CalError::Auth),
        412 => Err(CalError::Exists),
        status => Err(CalError::Rejected(status)),
    }
}

fn multistatus(reply: &Reply) -> Result<xml::Node, CalError> {
    check(reply, &[207])?;
    xml::parse(&reply.body).map_err(CalError::Protocol)
}

const NS: &str =
    r#"xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/""#;

fn propfind(
    transport: &dyn Transport,
    credentials: &Credentials<'_>,
    host: &str,
    path: &str,
    depth: &str,
    props: &str,
) -> Result<xml::Node, CalError> {
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><d:propfind {NS}><d:prop>{props}</d:prop></d:propfind>"#
    );
    let reply = call(
        transport,
        credentials,
        host,
        Method::Propfind,
        path,
        &[
            ("Depth", depth),
            ("Content-Type", "application/xml; charset=utf-8"),
        ],
        &body,
        MAX_DISCOVERY,
    )?;
    multistatus(&reply)
}

/// `#RRGGBB` aus Apples `#RRGGBBAA`; alles andere bleibt `None`.
fn normalize_color(raw: &str) -> Option<String> {
    let hex = raw.trim().strip_prefix('#')?;
    (matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("#{}", hex[..6].to_ascii_uppercase()))
}

/// Findet alle Kalender des Kontos (nur solche, die Termine enthalten dürfen).
///
/// Drei Schritte, wie Apple sie vorsieht: Benutzerkennung (`current-user-principal`), Heimatordner
/// der Kalender (`calendar-home-set`, liegt auf der Server-Gruppe des Kontos) und die Liste darin.
///
/// # Errors
/// [`CalError`] bei Anmeldefehler, Netzfehler oder unerwarteter Antwort.
pub fn discover(
    transport: &dyn Transport,
    credentials: &Credentials<'_>,
) -> Result<Vec<Found>, CalError> {
    let root = propfind(
        transport,
        credentials,
        ICLOUD_ROOT_HOST,
        "/",
        "0",
        "<d:current-user-principal/>",
    )?;
    let principal = root
        .find("current-user-principal")
        .and_then(|n| n.child("href"))
        .map(|n| n.text.clone())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| CalError::Protocol("Keine Benutzerkennung gefunden".to_owned()))?;
    let (principal_host, principal_path) = resolve_href(ICLOUD_ROOT_HOST, &principal)?;

    let home_doc = propfind(
        transport,
        credentials,
        &principal_host,
        &principal_path,
        "0",
        "<c:calendar-home-set/>",
    )?;
    let home = home_doc
        .find("calendar-home-set")
        .and_then(|n| n.child("href"))
        .map(|n| n.text.clone())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| CalError::Protocol("Kein Kalenderordner gefunden".to_owned()))?;
    let (home_host, home_path) = resolve_href(&principal_host, &home)?;

    let list = propfind(
        transport,
        credentials,
        &home_host,
        &home_path,
        "1",
        "<d:displayname/><d:resourcetype/><c:supported-calendar-component-set/><a:calendar-color/><d:current-user-privilege-set/>",
    )?;
    let mut responses = Vec::new();
    list.find_all("response", &mut responses);
    let mut found = Vec::new();
    for response in responses {
        let Some(href) = response.child("href").map(|n| n.text.as_str()) else {
            continue;
        };
        // Es zählt der Teil der Antwort mit Status 200; fehlende Eigenschaften stehen in 404-Blöcken.
        let Some(prop) = response
            .children_named("propstat")
            .find(|p| p.child("status").is_some_and(|s| s.text.contains(" 200")))
            .and_then(|p| p.child("prop"))
        else {
            continue;
        };
        let is_calendar = prop
            .child("resourcetype")
            .is_some_and(|r| r.child("calendar").is_some());
        let holds_events = prop
            .child("supported-calendar-component-set")
            .is_some_and(|set| {
                set.children_named("comp").any(|c| {
                    c.attr("name")
                        .is_some_and(|n| n.eq_ignore_ascii_case("VEVENT"))
                })
            });
        if !is_calendar || !holds_events {
            continue;
        }
        let Ok((host, path)) = resolve_href(&home_host, href) else {
            continue;
        };
        let path = if path.ends_with('/') {
            path
        } else {
            format!("{path}/")
        };
        let privileges = prop.child("current-user-privilege-set");
        let can_write = privileges.is_some_and(|set| {
            let has = |name: &str| {
                let mut hits = Vec::new();
                set.find_all(name, &mut hits);
                !hits.is_empty()
            };
            has("all") || has("write") || (has("bind") && has("write-content"))
        });
        let name = prop
            .child("displayname")
            .map(|n| n.text.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| {
                path.trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("Kalender")
                    .to_owned()
            });
        found.push(Found {
            host,
            href: path,
            name,
            color: prop
                .child("calendar-color")
                .and_then(|n| normalize_color(&n.text)),
            can_write,
        });
    }
    Ok(found)
}

/// Ruft die Termine eines Kalenders für ein Zeitfenster ab.
///
/// Der Server soll Serien selbst in Einzeltermine auflösen (`expand`); liefert er trotzdem Serien,
/// löst `pa_scheduler::feed` sie auf. Jede Ressource wird für sich gelesen, damit Ausnahmen einer
/// Serie nur diese Serie betreffen.
///
/// # Errors
/// [`CalError`] bei Anmeldefehler, Netzfehler oder unerwarteter Antwort.
pub fn fetch_events(
    transport: &dyn Transport,
    credentials: &Credentials<'_>,
    target: &Target<'_>,
    window: Window,
    local_offset_minutes: i32,
) -> Result<Parsed, CalError> {
    if !safe_path(target.href) {
        return Err(CalError::Invalid("Ungültiger Kalenderpfad".to_owned()));
    }
    let from = ics_datetime_utc(window.from_unix_ms);
    let to = ics_datetime_utc(window.to_unix_ms);
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query {NS}><d:prop><d:getetag/><c:calendar-data><c:expand start="{from}" end="{to}"/></c:calendar-data></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{from}" end="{to}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#
    );
    let reply = call(
        transport,
        credentials,
        target.host,
        Method::Report,
        target.href,
        &[
            ("Depth", "1"),
            ("Content-Type", "application/xml; charset=utf-8"),
        ],
        &body,
        MAX_EVENTS_RESPONSE,
    )?;
    let doc = multistatus(&reply)?;
    let mut items = Vec::new();
    doc.find_all("calendar-data", &mut items);
    let mut all = Parsed::default();
    for item in items {
        if item.text.is_empty() {
            continue;
        }
        let parsed = feed::parse_feed(&item.text, window, local_offset_minutes);
        all.instances.extend(parsed.instances);
        all.unsupported_rules += parsed.unsupported_rules;
        all.skipped_events += parsed.skipped_events;
    }
    all.instances
        .sort_by(|a, b| (a.start_unix_ms, &a.title).cmp(&(b.start_unix_ms, &b.title)));
    Ok(all)
}

/// Legt einen Termin an (`PUT … If-None-Match: *`, überschreibt also nie etwas).
///
/// `file_name` ist die Kennung ohne Endung (nur Buchstaben, Ziffern, `-`, `_`); `ics` kommt aus
/// `feed::build_event_ics`.
///
/// # Errors
/// [`CalError::Exists`] bei Kollision, sonst Anmelde-, Netz- oder Serverfehler.
pub fn create_event(
    transport: &dyn Transport,
    credentials: &Credentials<'_>,
    target: &Target<'_>,
    file_name: &str,
    ics: &str,
) -> Result<(), CalError> {
    if file_name.is_empty()
        || !file_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(CalError::Invalid("Ungültiger Dateiname".to_owned()));
    }
    if !target.href.ends_with('/') || !safe_path(target.href) {
        return Err(CalError::Invalid("Ungültiger Kalenderpfad".to_owned()));
    }
    let path = format!("{}{file_name}.ics", target.href);
    let reply = call(
        transport,
        credentials,
        target.host,
        Method::Put,
        &path,
        &[
            ("Content-Type", "text/calendar; charset=utf-8"),
            ("If-None-Match", "*"),
        ],
        ics,
        MAX_DISCOVERY,
    )?;
    check(&reply, &[201, 204])
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::net::{HttpsResponse, NetError};

    /// Antwortet der Reihe nach und merkt sich Server, Verb, Pfad, Kopfzeilen und Rumpf.
    /// Server, Verb, Pfad, Kopfzeilen, Rumpf einer gesendeten Anfrage.
    pub type Seen = (String, Method, String, Vec<(String, String)>, String);

    pub struct Scripted {
        replies: Mutex<Vec<(u16, String)>>,
        pub seen: Mutex<Vec<Seen>>,
    }

    impl Scripted {
        pub fn new(replies: &[(u16, &str)]) -> Self {
            Self {
                replies: Mutex::new(
                    replies
                        .iter()
                        .rev()
                        .map(|(s, b)| (*s, (*b).to_owned()))
                        .collect(),
                ),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    impl Transport for Scripted {
        fn send(
            &self,
            host: &str,
            method: Method,
            target: &str,
            headers: &[(&str, &str)],
            body: &[u8],
            _timeout: Duration,
            _max: usize,
        ) -> Result<HttpsResponse, NetError> {
            self.seen.lock().unwrap().push((
                host.to_owned(),
                method,
                target.to_owned(),
                headers
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
                String::from_utf8_lossy(body).into_owned(),
            ));
            let (status, body) = self
                .replies
                .lock()
                .unwrap()
                .pop()
                .expect("unerwartete Anfrage");
            Ok(HttpsResponse {
                status,
                body: body.into_bytes(),
            })
        }
    }

    fn creds() -> Credentials<'static> {
        Credentials {
            user: "anna@icloud.com",
            password: "abcd-efgh-ijkl-mnop",
        }
    }

    const PRINCIPAL: &str = r#"<?xml version="1.0"?><multistatus xmlns="DAV:"><response><href>/</href><propstat><prop><current-user-principal><href>/1234567890/principal/</href></current-user-principal></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#;
    const HOME: &str = r#"<?xml version="1.0"?><multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav"><response><href>/1234567890/principal/</href><propstat><prop><C:calendar-home-set><href>https://p12-caldav.icloud.com:443/1234567890/calendars/</href></C:calendar-home-set></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#;
    const LIST: &str = r#"<?xml version="1.0"?><multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:A="http://apple.com/ns/ical/">
<response><href>/1234567890/calendars/</href><propstat><prop><displayname/><resourcetype><collection/></resourcetype></prop><status>HTTP/1.1 200 OK</status></propstat></response>
<response><href>/1234567890/calendars/home/</href><propstat><prop><displayname>Privat</displayname><resourcetype><collection/><C:calendar/></resourcetype><C:supported-calendar-component-set><C:comp name="VEVENT"/><C:comp name="VTODO"/></C:supported-calendar-component-set><A:calendar-color>#FF2968FF</A:calendar-color><current-user-privilege-set><privilege><read/></privilege><privilege><write/></privilege></current-user-privilege-set></prop><status>HTTP/1.1 200 OK</status></propstat></response>
<response><href>/1234567890/calendars/reminders/</href><propstat><prop><displayname>Erinnerungen</displayname><resourcetype><collection/><C:calendar/></resourcetype><C:supported-calendar-component-set><C:comp name="VTODO"/></C:supported-calendar-component-set></prop><status>HTTP/1.1 200 OK</status></propstat></response>
<response><href>/1234567890/calendars/shared</href><propstat><prop><displayname>Familie (nur lesen)</displayname><resourcetype><collection/><C:calendar/></resourcetype><C:supported-calendar-component-set><C:comp name="VEVENT"/></C:supported-calendar-component-set><current-user-privilege-set><privilege><read/></privilege></current-user-privilege-set></prop><status>HTTP/1.1 200 OK</status></propstat></response>
<response><href>/1234567890/calendars/inbox/</href><propstat><prop><displayname>Posteingang</displayname><resourcetype><collection/></resourcetype></prop><status>HTTP/1.1 200 OK</status></propstat></response>
</multistatus>"#;

    #[test]
    fn discovery_follows_principal_and_home_to_the_account_shard_and_lists_only_event_calendars() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        let fake = Scripted::new(&[(207, PRINCIPAL), (207, HOME), (207, LIST)]);
        let found = discover(&fake, &creds()).unwrap();
        assert_eq!(
            found,
            [
                Found {
                    host: "p12-caldav.icloud.com".to_owned(),
                    href: "/1234567890/calendars/home/".to_owned(),
                    name: "Privat".to_owned(),
                    color: Some("#FF2968".to_owned()),
                    can_write: true,
                },
                Found {
                    host: "p12-caldav.icloud.com".to_owned(),
                    href: "/1234567890/calendars/shared/".to_owned(),
                    name: "Familie (nur lesen)".to_owned(),
                    color: None,
                    can_write: false,
                },
            ]
        );
        let seen = fake.seen.lock().unwrap();
        let steps: Vec<_> = seen
            .iter()
            .map(|(h, m, p, _, _)| (h.as_str(), *m, p.as_str()))
            .collect();
        assert_eq!(
            steps,
            [
                ("caldav.icloud.com", Method::Propfind, "/"),
                (
                    "caldav.icloud.com",
                    Method::Propfind,
                    "/1234567890/principal/"
                ),
                (
                    "p12-caldav.icloud.com",
                    Method::Propfind,
                    "/1234567890/calendars/"
                ),
            ]
        );
        // Anmeldung: Basic mit Apple-ID und Passwort, Tiefe je Schritt.
        let (_, _, _, headers, body) = &seen[0];
        let auth = headers.iter().find(|(k, _)| k == "Authorization").unwrap();
        assert_eq!(
            auth.1,
            format!(
                "Basic {}",
                base64_encode(b"anna@icloud.com:abcd-efgh-ijkl-mnop")
            )
        );
        assert!(headers.contains(&("Depth".to_owned(), "0".to_owned())));
        assert!(body.contains("current-user-principal"));
        assert!(seen[2].3.contains(&("Depth".to_owned(), "1".to_owned())));
    }

    #[test]
    fn a_server_naming_a_foreign_host_is_refused() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        for evil in [
            "https://evil.example/1/calendars/",
            "https://p12-caldav.icloud.com.evil.example/1/calendars/",
            "http://p12-caldav.icloud.com/1/calendars/",
            "//evil.example/x",
            "p12-caldav.icloud.com/x",
        ] {
            let home = HOME.replace(
                "https://p12-caldav.icloud.com:443/1234567890/calendars/",
                evil,
            );
            let fake = Scripted::new(&[(207, PRINCIPAL), (207, &home)]);
            let result = discover(&fake, &creds());
            assert!(
                matches!(result, Err(CalError::Blocked(_) | CalError::Protocol(_))),
                "{evil}: {result:?}"
            );
            assert_eq!(
                fake.seen.lock().unwrap().len(),
                2,
                "{evil}: kein dritter Aufruf"
            );
        }
    }

    #[test]
    fn hrefs_with_traversal_or_query_are_refused() {
        for path in ["/a/../b/", "/a?x=1", "/a#b", "/a b/", "/a\r\nHost: x/"] {
            assert!(resolve_href("caldav.icloud.com", path).is_err(), "{path:?}");
        }
        assert_eq!(
            resolve_href("p5-caldav.icloud.com", "/1/calendars/").unwrap(),
            (
                "p5-caldav.icloud.com".to_owned(),
                "/1/calendars/".to_owned()
            )
        );
        assert_eq!(
            resolve_href("x", "https://P7-CalDAV.iCloud.com/1/").unwrap(),
            ("p7-caldav.icloud.com".to_owned(), "/1/".to_owned())
        );
    }

    #[test]
    fn wrong_credentials_and_server_errors_are_told_apart() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        assert!(matches!(
            discover(&Scripted::new(&[(401, "")]), &creds()),
            Err(CalError::Auth)
        ));
        assert!(matches!(
            discover(&Scripted::new(&[(503, "")]), &creds()),
            Err(CalError::Rejected(503))
        ));
        assert!(matches!(
            discover(&Scripted::new(&[(207, "<kaputt")]), &creds()),
            Err(CalError::Protocol(_))
        ));
        assert!(matches!(
            discover(
                &Scripted::new(&[(207, "<multistatus xmlns=\"DAV:\"/>")]),
                &creds()
            ),
            Err(CalError::Protocol(_))
        ));
    }

    #[test]
    fn the_air_gap_stops_discovery_before_anything_is_sent() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(true);
        let fake = Scripted::new(&[]);
        assert!(
            matches!(discover(&fake, &creds()), Err(CalError::Blocked(m)) if m.contains("Air Gap"))
        );
        assert!(fake.seen.lock().unwrap().is_empty());
        net::set_air_gap(false);
    }

    fn ms(y: i64, m: u32, d: u32, h: i64) -> i64 {
        (pa_scheduler::civil::days_from_civil(y, m, d) * 86_400 + h * 3600) * 1000
    }

    fn report(events: &[&str]) -> String {
        let items: String = events
            .iter()
            .enumerate()
            .map(|(i, ics)| {
                format!("<response><href>/1/calendars/home/e{i}.ics</href><propstat><prop><getetag>\"1\"</getetag><calendar-data xmlns=\"urn:ietf:params:xml:ns:caldav\">{}</calendar-data></prop><status>HTTP/1.1 200 OK</status></propstat></response>", xml::escape(ics))
            })
            .collect();
        format!(r#"<?xml version="1.0"?><multistatus xmlns="DAV:">{items}</multistatus>"#)
    }

    #[test]
    fn events_are_requested_with_expand_and_read_per_resource() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        let one = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Zahnarzt\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T090000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let two = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Serie\r\nDTSTART:20261001T070000Z\r\nDTEND:20261001T073000Z\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let fake = Scripted::new(&[(207, &report(&[one, two]))]);
        let window = Window {
            from_unix_ms: ms(2026, 10, 1, 0),
            to_unix_ms: ms(2026, 11, 1, 0),
        };
        let parsed = fetch_events(
            &fake,
            &creds(),
            &Target {
                host: "p12-caldav.icloud.com",
                href: "/1/calendars/home/",
            },
            window,
            60,
        )
        .unwrap();
        let titles: Vec<_> = parsed
            .instances
            .iter()
            .map(|i| (i.title.as_str(), i.start_unix_ms))
            .collect();
        assert_eq!(
            titles,
            [
                ("Serie", ms(2026, 10, 1, 7)),
                ("Serie", ms(2026, 10, 2, 7)),
                ("Serie", ms(2026, 10, 3, 7)),
                ("Zahnarzt", ms(2026, 10, 5, 8)),
            ]
        );
        let seen = fake.seen.lock().unwrap();
        let (host, method, path, headers, body) = &seen[0];
        assert_eq!(
            (host.as_str(), *method, path.as_str()),
            (
                "p12-caldav.icloud.com",
                Method::Report,
                "/1/calendars/home/"
            )
        );
        assert!(
            body.contains(r#"<c:expand start="20261001T000000Z" end="20261101T000000Z"/>"#),
            "{body}"
        );
        assert!(body.contains(r#"<c:time-range start="20261001T000000Z" end="20261101T000000Z"/>"#));
        assert!(headers.contains(&("Depth".to_owned(), "1".to_owned())));
    }

    #[test]
    fn an_unreadable_resource_does_not_hide_the_others() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        let good = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Gut\r\nDTSTART:20261005T080000Z\r\nDTEND:20261005T090000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let bad = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:b\r\nSUMMARY:Ohne Beginn\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let fake = Scripted::new(&[(207, &report(&[bad, good]))]);
        let window = Window {
            from_unix_ms: ms(2026, 10, 1, 0),
            to_unix_ms: ms(2026, 11, 1, 0),
        };
        let parsed = fetch_events(
            &fake,
            &creds(),
            &Target {
                host: "p12-caldav.icloud.com",
                href: "/1/calendars/home/",
            },
            window,
            0,
        )
        .unwrap();
        assert_eq!(parsed.instances.len(), 1);
        assert_eq!(parsed.skipped_events, 1);
    }

    #[test]
    fn creating_puts_with_if_none_match_and_maps_the_statuses() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        let target = Target {
            host: "p12-caldav.icloud.com",
            href: "/1/calendars/home/",
        };
        let fake = Scripted::new(&[(201, "")]);
        create_event(
            &fake,
            &creds(),
            &target,
            "0123abcd",
            "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        {
            let seen = fake.seen.lock().unwrap();
            let (host, method, path, headers, body) = &seen[0];
            assert_eq!(
                (host.as_str(), *method, path.as_str()),
                (
                    "p12-caldav.icloud.com",
                    Method::Put,
                    "/1/calendars/home/0123abcd.ics"
                )
            );
            assert!(headers.contains(&("If-None-Match".to_owned(), "*".to_owned())));
            assert!(headers.contains(&(
                "Content-Type".to_owned(),
                "text/calendar; charset=utf-8".to_owned()
            )));
            assert_eq!(body, "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n");
        }
        assert!(create_event(&Scripted::new(&[(204, "")]), &creds(), &target, "x", "i").is_ok());
        assert!(matches!(
            create_event(&Scripted::new(&[(412, "")]), &creds(), &target, "x", "i"),
            Err(CalError::Exists)
        ));
        assert!(matches!(
            create_event(&Scripted::new(&[(401, "")]), &creds(), &target, "x", "i"),
            Err(CalError::Auth)
        ));
        assert!(matches!(
            create_event(&Scripted::new(&[(403, "")]), &creds(), &target, "x", "i"),
            Err(CalError::Rejected(403))
        ));
        assert!(
            matches!(
                create_event(&Scripted::new(&[(200, "")]), &creds(), &target, "x", "i"),
                Err(CalError::Rejected(200))
            ),
            "nur 201/204 gelten als angelegt"
        );
    }

    #[test]
    fn create_rejects_bad_names_and_paths_without_sending() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        let fake = Scripted::new(&[]);
        let ok = Target {
            host: "p12-caldav.icloud.com",
            href: "/1/calendars/home/",
        };
        for name in ["", "../x", "a/b", "a b", "a.ics", "ä"] {
            assert!(
                matches!(
                    create_event(&fake, &creds(), &ok, name, "i"),
                    Err(CalError::Invalid(_))
                ),
                "{name:?}"
            );
        }
        for href in ["/1/calendars/home", "/1/../x/", "/x?y/"] {
            let target = Target {
                host: "p12-caldav.icloud.com",
                href,
            };
            assert!(
                matches!(
                    create_event(&fake, &creds(), &target, "x", "i"),
                    Err(CalError::Invalid(_))
                ),
                "{href}"
            );
        }
        let foreign = Target {
            host: "evil.example",
            href: "/1/",
        };
        assert!(matches!(
            create_event(&fake, &creds(), &foreign, "x", "i"),
            Err(CalError::Blocked(_))
        ));
        assert!(fake.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn colors_are_normalised_or_dropped() {
        assert_eq!(normalize_color("#FF2968FF").as_deref(), Some("#FF2968"));
        assert_eq!(normalize_color(" #00ff00 ").as_deref(), Some("#00FF00"));
        for bad in [
            "",
            "red",
            "#12",
            "#GGGGGG",
            "FF2968",
            "#FF2968FF00",
            "#FF2968;x",
        ] {
            assert_eq!(normalize_color(bad), None, "{bad}");
        }
    }
}
