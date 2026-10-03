//! Abgleich einer Quelle: Termine abrufen und in die Form bringen, die die Oberfläche anzeigt;
//! einen neuen Termin im Kalender des Kontos anlegen.
//!
//! Dieser Teil kennt weder Tresor noch Oberfläche. Er bekommt die Quelle, ihr Geheimnis und einen
//! Transport und liefert Ergebnisse oder einen [`CalError`]. So lässt er sich ohne Netz testen.

use std::time::Duration;

use pa_policy::CapabilityAction;
use pa_scheduler::feed::{self, Instance, NewEvent, Window};

use super::{
    caldav::{self, Credentials, Target},
    ics_url, CalError, CalendarRef, RemoteEvent, Source, SourceKind,
};
use crate::net::{HttpsResponse, Method, NetError, Transport};

const DAY_MS: i64 = 86_400_000;
/// So weit zurück und voraus werden Termine abgerufen.
const DAYS_BACK: i64 = 30;
const DAYS_AHEAD: i64 = 180;

/// Das Zeitfenster eines Abrufs rund um `now`.
pub fn window_around(now_unix_ms: i64) -> Window {
    Window {
        from_unix_ms: now_unix_ms - DAYS_BACK * DAY_MS,
        to_unix_ms: now_unix_ms + DAYS_AHEAD * DAY_MS,
    }
}

/// Prüft jede Anfrage gegen die Policy, bevor sie den Transport erreicht.
///
/// Warum unter dem Transport und nicht davor: So kann **keine** Anfrage – auch nicht an eine vom
/// Server genannte Adresse – an der Policy vorbeigehen. Die Prüfung läuft pro Anfrage und liest
/// den Air Gap dabei frisch (`gate` fragt ihn ab und schreibt den Audit-Eintrag).
pub struct Gated<'a> {
    pub inner: &'a dyn Transport,
    pub gate: &'a (dyn Fn(&str, CapabilityAction) -> Result<(), String> + Sync),
}

impl Transport for Gated<'_> {
    fn send(
        &self,
        host: &str,
        method: Method,
        target: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        timeout: Duration,
        max_response: usize,
    ) -> Result<HttpsResponse, NetError> {
        let action = if method == Method::Put {
            CapabilityAction::CalendarWrite
        } else {
            CapabilityAction::CalendarRead
        };
        (self.gate)(host, action).map_err(NetError::Blocked)?;
        self.inner
            .send(host, method, target, headers, body, timeout, max_response)
    }
}

/// Ergebnis eines Abrufs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncResult {
    pub events: Vec<RemoteEvent>,
    pub unsupported_rules: u32,
    pub skipped_events: u32,
}

fn to_remote(
    source: &Source,
    index: usize,
    calendar_name: &str,
    color: &Option<String>,
    instance: Instance,
) -> RemoteEvent {
    RemoteEvent {
        id: format!(
            "{}|{index}|{}|{}",
            source.id, instance.uid, instance.start_unix_ms
        ),
        source_id: source.id.clone(),
        source_label: source.label.clone(),
        calendar_name: calendar_name.to_owned(),
        color: color.clone(),
        title: instance.title,
        start_unix_ms: instance.start_unix_ms,
        end_unix_ms: instance.end_unix_ms,
        all_day: instance.all_day,
        start_date: instance.start_date,
        end_date: instance.end_date,
        location: instance.location,
        notes: instance.notes,
        recurring: instance.recurring,
    }
}

/// Ruft alle Kalender einer Quelle ab. Bei einem Fehler wird nichts Halbes zurückgegeben: der
/// Aufrufer behält dann den bisherigen Stand.
///
/// # Errors
/// [`CalError`] des ersten fehlgeschlagenen Kalenders.
pub fn sync_source(
    transport: &dyn Transport,
    source: &Source,
    secret: &str,
    window: Window,
    local_offset_minutes: i32,
) -> Result<SyncResult, CalError> {
    let mut result = SyncResult::default();
    match source.kind {
        SourceKind::Icloud => {
            let credentials = Credentials {
                user: &source.account,
                password: secret,
            };
            for (index, calendar) in source.calendars.iter().enumerate() {
                let parsed = caldav::fetch_events(
                    transport,
                    &credentials,
                    &Target {
                        host: &calendar.host,
                        href: &calendar.href,
                    },
                    window,
                    local_offset_minutes,
                )?;
                result.unsupported_rules += parsed.unsupported_rules;
                result.skipped_events += parsed.skipped_events;
                result.events.extend(
                    parsed
                        .instances
                        .into_iter()
                        .map(|i| to_remote(source, index, &calendar.name, &calendar.color, i)),
                );
            }
        }
        SourceKind::GoogleIcs => {
            let path = ics_url::parse_url(secret)?;
            let parsed = ics_url::fetch(transport, &path, window, local_offset_minutes)?;
            result.unsupported_rules = parsed.unsupported_rules;
            result.skipped_events = parsed.skipped_events;
            let (name, color) = source
                .calendars
                .first()
                .map_or((source.label.as_str(), None), |c| {
                    (c.name.as_str(), c.color.clone())
                });
            result.events = parsed
                .instances
                .into_iter()
                .map(|i| to_remote(source, 0, name, &color, i))
                .collect();
        }
    }
    result
        .events
        .sort_by(|a, b| (a.start_unix_ms, &a.title).cmp(&(b.start_unix_ms, &b.title)));
    Ok(result)
}

/// Sucht den beschreibbaren Kalender einer Quelle.
fn writable_calendar<'a>(
    source: &'a Source,
    href: &str,
) -> Result<(usize, &'a CalendarRef), CalError> {
    if !source.kind.can_write() {
        return Err(CalError::Invalid(
            "In diese Quelle kann IAP keine Termine eintragen (nur Lesen).".to_owned(),
        ));
    }
    let (index, calendar) = source
        .calendars
        .iter()
        .enumerate()
        .find(|(_, c)| c.href == href)
        .ok_or_else(|| CalError::Invalid("Dieser Kalender gehört nicht zur Quelle.".to_owned()))?;
    if !calendar.can_write {
        return Err(CalError::Invalid(
            "Dieser Kalender ist schreibgeschützt (etwa ein geteilter Kalender).".to_owned(),
        ));
    }
    Ok((index, calendar))
}

/// Was zum Anlegen eines Termins gehört.
pub struct CreateRequest<'a> {
    /// Pfad des Zielkalenders (muss zur Quelle gehören und beschreibbar sein).
    pub calendar_href: &'a str,
    pub event: &'a NewEvent,
    pub now_unix_ms: i64,
    pub local_offset_minutes: i32,
    /// Bestimmt die Kennung des Termins (16 Zufallsbytes aus dem Betriebssystem).
    pub entropy: [u8; 16],
}

/// Legt einen Termin im Apple-Kalender an und liefert ihn so zurück, wie er angezeigt wird.
///
/// # Errors
/// [`CalError`] bei ungültigem Termin, schreibgeschütztem Kalender oder Serverfehler. Wurde der
/// Termin nicht bestätigt (kein 201/204), gilt er als **nicht** angelegt.
pub fn create_in_icloud(
    transport: &dyn Transport,
    source: &Source,
    secret: &str,
    request: &CreateRequest<'_>,
) -> Result<RemoteEvent, CalError> {
    let (index, calendar) = writable_calendar(source, request.calendar_href)?;
    let file_name = hex::encode(request.entropy);
    let uid = format!("{file_name}@iap");
    let ics = feed::build_event_ics(&uid, request.event, request.now_unix_ms)
        .map_err(|e| CalError::Invalid(e.to_string()))?;
    // Zuerst lesen, was wir senden, damit die Anzeige exakt dem entspricht, was im Kalender liegt.
    let parsed = feed::parse_feed(
        &ics,
        Window {
            from_unix_ms: i64::MIN / 4,
            to_unix_ms: i64::MAX / 4,
        },
        request.local_offset_minutes,
    );
    let instance = parsed
        .instances
        .into_iter()
        .next()
        .ok_or_else(|| CalError::Invalid("Der Termin ist nicht lesbar.".to_owned()))?;
    caldav::create_event(
        transport,
        &Credentials {
            user: &source.account,
            password: secret,
        },
        &Target {
            host: &calendar.host,
            href: &calendar.href,
        },
        &file_name,
        &ics,
    )?;
    Ok(to_remote(
        source,
        index,
        &calendar.name,
        &calendar.color,
        instance,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::calendar::caldav::tests::Scripted;
    use crate::net;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        let guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        guard
    }

    fn ms(y: i64, m: u32, d: u32, h: i64) -> i64 {
        (pa_scheduler::civil::days_from_civil(y, m, d) * 86_400 + h * 3600) * 1000
    }

    fn icloud(can_write: bool) -> Source {
        Source {
            id: "src-1".to_owned(),
            kind: SourceKind::Icloud,
            label: "Apple".to_owned(),
            account: "anna@icloud.com".to_owned(),
            calendars: vec![
                CalendarRef {
                    host: "p12-caldav.icloud.com".to_owned(),
                    href: "/1/calendars/home/".to_owned(),
                    name: "Privat".to_owned(),
                    color: Some("#FF2968".to_owned()),
                    can_write,
                },
                CalendarRef {
                    host: "p12-caldav.icloud.com".to_owned(),
                    href: "/1/calendars/work/".to_owned(),
                    name: "Arbeit".to_owned(),
                    color: None,
                    can_write: true,
                },
            ],
            last_sync_unix_ms: None,
            last_error: None,
            unsupported_rules: 0,
        }
    }

    fn report(uid: &str, title: &str, day: u32) -> String {
        let ics = format!("BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nSUMMARY:{title}\r\nDTSTART:202610{day:02}T080000Z\r\nDTEND:202610{day:02}T090000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
        format!("<multistatus xmlns=\"DAV:\"><response><href>/x.ics</href><propstat><prop><calendar-data xmlns=\"urn:ietf:params:xml:ns:caldav\">{}</calendar-data></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>", crate::calendar::xml::escape(&ics))
    }

    #[test]
    fn the_window_spans_a_month_back_and_half_a_year_ahead() {
        let w = window_around(1_000 * DAY_MS);
        assert_eq!(w.from_unix_ms, 970 * DAY_MS);
        assert_eq!(w.to_unix_ms, 1_180 * DAY_MS);
    }

    #[test]
    fn icloud_sync_merges_all_calendars_with_stable_ids_names_and_colors() {
        let _g = lock();
        let fake = Scripted::new(&[
            (207, &report("a", "Zahnarzt", 7)),
            (207, &report("b", "Sitzung", 5)),
        ]);
        let result = sync_source(
            &fake,
            &icloud(true),
            "pw",
            window_around(ms(2026, 10, 2, 0)),
            60,
        )
        .unwrap();
        let got: Vec<_> = result
            .events
            .iter()
            .map(|e| {
                (
                    e.id.as_str(),
                    e.title.as_str(),
                    e.calendar_name.as_str(),
                    e.color.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (
                    format!("src-1|1|b|{}", ms(2026, 10, 5, 8)).as_str(),
                    "Sitzung",
                    "Arbeit",
                    None
                ),
                (
                    format!("src-1|0|a|{}", ms(2026, 10, 7, 8)).as_str(),
                    "Zahnarzt",
                    "Privat",
                    Some("#FF2968")
                ),
            ]
        );
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0].2, "/1/calendars/home/");
        assert_eq!(seen[1].2, "/1/calendars/work/");
    }

    #[test]
    fn one_failing_calendar_fails_the_whole_sync_so_the_old_state_stays() {
        let _g = lock();
        let fake = Scripted::new(&[(207, &report("a", "Zahnarzt", 7)), (503, "")]);
        let result = sync_source(
            &fake,
            &icloud(true),
            "pw",
            window_around(ms(2026, 10, 2, 0)),
            0,
        );
        assert!(matches!(result, Err(CalError::Rejected(503))));
        let fake = Scripted::new(&[(401, "")]);
        assert!(matches!(
            sync_source(&fake, &icloud(true), "falsch", window_around(0), 0),
            Err(CalError::Auth)
        ));
    }

    #[test]
    fn google_sync_reads_the_secret_address_and_names_events_after_the_source() {
        let _g = lock();
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:g1\r\nSUMMARY:Elternabend\r\nDTSTART;VALUE=DATE:20261008\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let fake = Scripted::new(&[(200, ics)]);
        let source = Source {
            id: "src-g".to_owned(),
            kind: SourceKind::GoogleIcs,
            label: "Schule".to_owned(),
            account: String::new(),
            calendars: Vec::new(),
            last_sync_unix_ms: None,
            last_error: None,
            unsupported_rules: 0,
        };
        let url = "https://calendar.google.com/calendar/ical/x%40gmail.com/private-abc/basic.ics";
        let result =
            sync_source(&fake, &source, url, window_around(ms(2026, 10, 2, 0)), 60).unwrap();
        assert_eq!(result.events.len(), 1);
        let event = &result.events[0];
        assert_eq!(
            (event.calendar_name.as_str(), event.all_day),
            ("Schule", true)
        );
        assert_eq!(event.start_date.as_deref(), Some("2026-10-08"));
        assert!(matches!(
            sync_source(
                &Scripted::new(&[]),
                &source,
                "https://evil.example/x.ics",
                window_around(0),
                0
            ),
            Err(CalError::Invalid(_))
        ));
    }

    fn new_event() -> NewEvent {
        NewEvent {
            title: "Elterngespräch".to_owned(),
            all_day: false,
            start_unix_ms: ms(2026, 10, 9, 14),
            end_unix_ms: ms(2026, 10, 9, 15),
            start_date: None,
            end_date: None,
            location: Some("Raum 3".to_owned()),
            notes: None,
        }
    }

    #[test]
    fn creating_writes_to_the_chosen_calendar_and_returns_what_was_sent() {
        let _g = lock();
        let fake = Scripted::new(&[(201, "")]);
        let created = create_in_icloud(
            &fake,
            &icloud(true),
            "pw",
            &CreateRequest {
                calendar_href: "/1/calendars/work/",
                event: &new_event(),
                now_unix_ms: ms(2026, 10, 2, 9),
                local_offset_minutes: 60,
                entropy: [0xAB; 16],
            },
        )
        .unwrap();
        assert_eq!(created.title, "Elterngespräch");
        assert_eq!(created.calendar_name, "Arbeit");
        assert_eq!(created.start_unix_ms, ms(2026, 10, 9, 14));
        assert_eq!(
            created.id,
            format!("src-1|1|{}@iap|{}", "ab".repeat(16), ms(2026, 10, 9, 14))
        );
        let seen = fake.seen.lock().unwrap();
        let (host, method, path, _, body) = &seen[0];
        assert_eq!(host, "p12-caldav.icloud.com");
        assert_eq!(*method, Method::Put);
        assert_eq!(path, &format!("/1/calendars/work/{}.ics", "ab".repeat(16)));
        assert!(
            body.contains("SUMMARY:Elterngespräch")
                && body.contains(&format!("UID:{}@iap", "ab".repeat(16)))
        );
    }

    #[test]
    fn nothing_is_created_for_read_only_sources_calendars_or_bad_events() {
        let _g = lock();
        let fake = Scripted::new(&[]);
        let google = Source {
            kind: SourceKind::GoogleIcs,
            ..icloud(true)
        };
        for (source, href) in [
            (google, "/1/calendars/home/"),
            (icloud(false), "/1/calendars/home/"),
            (icloud(true), "/1/calendars/unknown/"),
        ] {
            let result = create_in_icloud(
                &fake,
                &source,
                "pw",
                &CreateRequest {
                    calendar_href: href,
                    event: &new_event(),
                    now_unix_ms: 0,
                    local_offset_minutes: 0,
                    entropy: [1; 16],
                },
            );
            assert!(matches!(result, Err(CalError::Invalid(_))), "{href}");
        }
        let mut bad = new_event();
        bad.title = "A\r\nBEGIN:VALARM".to_owned();
        assert!(matches!(
            create_in_icloud(
                &fake,
                &icloud(true),
                "pw",
                &CreateRequest {
                    calendar_href: "/1/calendars/work/",
                    event: &bad,
                    now_unix_ms: 0,
                    local_offset_minutes: 0,
                    entropy: [1; 16]
                }
            ),
            Err(CalError::Invalid(_))
        ));
        assert!(fake.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn a_collision_or_refusal_means_not_created() {
        let _g = lock();
        let fake = Scripted::new(&[(412, "")]);
        assert!(matches!(
            create_in_icloud(
                &fake,
                &icloud(true),
                "pw",
                &CreateRequest {
                    calendar_href: "/1/calendars/work/",
                    event: &new_event(),
                    now_unix_ms: 0,
                    local_offset_minutes: 0,
                    entropy: [2; 16]
                }
            ),
            Err(CalError::Exists)
        ));
        let fake = Scripted::new(&[(403, "")]);
        assert!(matches!(
            create_in_icloud(
                &fake,
                &icloud(true),
                "pw",
                &CreateRequest {
                    calendar_href: "/1/calendars/work/",
                    event: &new_event(),
                    now_unix_ms: 0,
                    local_offset_minutes: 0,
                    entropy: [2; 16]
                }
            ),
            Err(CalError::Rejected(403))
        ));
    }

    #[test]
    fn the_gate_sees_every_request_with_its_host_and_action_and_can_stop_it() {
        let _g = lock();
        let fake = Scripted::new(&[(201, "")]);
        let log: Mutex<Vec<(String, CapabilityAction)>> = Mutex::new(Vec::new());
        let allow = |host: &str, action: CapabilityAction| -> Result<(), String> {
            log.lock().unwrap().push((host.to_owned(), action));
            Ok(())
        };
        let gated = Gated {
            inner: &fake,
            gate: &allow,
        };
        create_in_icloud(
            &gated,
            &icloud(true),
            "pw",
            &CreateRequest {
                calendar_href: "/1/calendars/work/",
                event: &new_event(),
                now_unix_ms: 0,
                local_offset_minutes: 0,
                entropy: [3; 16],
            },
        )
        .unwrap();
        assert_eq!(
            *log.lock().unwrap(),
            [(
                "p12-caldav.icloud.com".to_owned(),
                CapabilityAction::CalendarWrite
            )]
        );
        // Ein Nein der Policy erreicht den Transport nie.
        let fake = Scripted::new(&[]);
        let deny = |_: &str, _: CapabilityAction| -> Result<(), String> {
            Err("Air Gap ist eingeschaltet".to_owned())
        };
        let gated = Gated {
            inner: &fake,
            gate: &deny,
        };
        let result = sync_source(&gated, &icloud(true), "pw", window_around(0), 0);
        assert!(matches!(result, Err(CalError::Blocked(m)) if m.contains("Air Gap")));
        assert!(fake.seen.lock().unwrap().is_empty());
    }
}
