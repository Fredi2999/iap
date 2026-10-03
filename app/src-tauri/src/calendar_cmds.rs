//! Kalender: Quellen verwalten, Termine abrufen, einen Termin anlegen.
//!
//! Zugangsdaten (Apple-Passwort, geheime Google-Adresse) liegen je als eigenes Setting im
//! Tresor und gehen nie an die Oberfläche zurück. Jeder Netzaufruf geht auf eine Handlung des
//! Nutzers zurück; es gibt keinen Takt. Unter jedem Aufruf sitzt der Policy-Wächter
//! (`pa_policy::egress::authorize_calendar`, Air Gap inklusive, mit Audit-Eintrag).

use std::sync::{Mutex, TryLockError};

use pa_policy::{egress::authorize_calendar, CapabilityAction, Decision};
use pa_scheduler::feed::NewEvent;
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::calendar::{
    caldav::{self, Credentials},
    sync::{self, Gated},
    CalError, CalendarRef, RemoteEvent, Source, SourceKind,
};
use crate::mail_cmds::{read_setting, write_setting};
use crate::{lifecycle, net, now_unix_ms, AppError, AppResult, AppState};

const SETTING_SOURCES: &str = "calendar.sources";
const MAX_SOURCES: usize = 8;
const MAX_LABEL_CHARS: usize = 60;

/// Alle Netzvorgänge des Kalenders laufen nacheinander. Das schützt den Zwischenspeicher vor
/// gleichzeitigen Schreibvorgängen (Abrufen und Anlegen) und vor doppelten Klicks.
static BUSY: Mutex<()> = Mutex::new(());

fn secret_key(id: &str) -> String {
    format!("calendar.secret.{id}")
}

fn cache_key(id: &str) -> String {
    format!("calendar.events.{id}")
}

/// Fehler des Kalenders als Text für die Oberfläche.
fn invalid(error: CalError) -> AppError {
    AppError::Invalid(error.to_string())
}

// ---------------------------------------------------------------------------------------------
// Prüfung von Eingaben (rein, ohne Tresor)
// ---------------------------------------------------------------------------------------------

/// Prüft die Apple-ID: eine E-Mail-Adresse ohne Leer- und Steuerzeichen und ohne `:` (Basic-Auth
/// trennt Name und Passwort an dieser Stelle).
fn clean_apple_id(raw: &str) -> Result<String, String> {
    let id = raw.trim();
    let valid = !id.is_empty()
        && id.chars().count() <= 254
        && id.matches('@').count() == 1
        && !id.starts_with('@')
        && !id.ends_with('@')
        && !id
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == ':');
    valid.then(|| id.to_owned()).ok_or_else(|| {
        "Gib deine Apple-ID ein (die E-Mail-Adresse, mit der du dich bei iCloud anmeldest)."
            .to_owned()
    })
}

/// Bringt ein app-spezifisches Apple-Passwort in Form (`xxxx-xxxx-xxxx-xxxx`, Leerraum wird entfernt).
fn clean_app_password(raw: &str) -> Result<String, String> {
    let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let valid = (8..=64).contains(&cleaned.len())
        && cleaned
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then_some(cleaned).ok_or_else(|| {
        "Das ist kein app-spezifisches Passwort. Erzeuge eines auf appleid.apple.com (Anmeldung und Sicherheit) und trage es hier ein; es sieht so aus: abcd-efgh-ijkl-mnop."
            .to_owned()
    })
}

fn clean_label(raw: &str, fallback: &str) -> Result<String, String> {
    let label = raw.trim();
    if label.chars().any(char::is_control) {
        return Err("Der Name enthält Steuerzeichen.".to_owned());
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        return Err(format!(
            "Der Name ist zu lang (höchstens {MAX_LABEL_CHARS} Zeichen)."
        ));
    }
    Ok(if label.is_empty() {
        fallback.to_owned()
    } else {
        label.to_owned()
    })
}

/// Kalender, die die Oberfläche nach der Auswahl übergibt: nur Server der Tabelle und sichere Pfade.
fn check_calendars(calendars: &[CalendarRef]) -> Result<(), String> {
    if calendars.is_empty() {
        return Err("Wähle mindestens einen Kalender aus.".to_owned());
    }
    for calendar in calendars {
        let host_ok = pa_policy::egress::Connector::Calendar.allows_host(&calendar.host);
        let href_ok = calendar.href.starts_with('/')
            && calendar.href.ends_with('/')
            && !calendar.href.starts_with("//")
            && !calendar.href.contains("..")
            && !calendar
                .href
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '?' | '#'));
        if !host_ok || !href_ok {
            return Err("Ein Kalender hat eine ungültige Adresse.".to_owned());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Tresor
// ---------------------------------------------------------------------------------------------

fn load_sources(state: &AppState) -> Vec<Source> {
    read_setting(state, SETTING_SOURCES)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_sources(state: &AppState, sources: &[Source]) -> AppResult<()> {
    let text = serde_json::to_string(sources).map_err(|e| AppError::Internal(e.to_string()))?;
    write_setting(state, SETTING_SOURCES, &text)
}

fn load_cache(state: &AppState, id: &str) -> Vec<RemoteEvent> {
    read_setting(state, &cache_key(id))
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_cache(state: &AppState, id: &str, events: &[RemoteEvent]) -> AppResult<()> {
    let text = serde_json::to_string(events).map_err(|e| AppError::Internal(e.to_string()))?;
    write_setting(state, &cache_key(id), &text)
}

fn load_secret(state: &AppState, id: &str) -> Result<String, AppError> {
    read_setting(state, &secret_key(id))?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::Invalid(
                "Für diese Verbindung ist kein Zugang gespeichert. Verbinde den Kalender neu."
                    .to_owned(),
            )
        })
}

/// Alle zwischengespeicherten Termine aller Quellen, nach Beginn sortiert (kein Netz).
pub(crate) fn cached_events(state: &AppState) -> Vec<RemoteEvent> {
    let mut all: Vec<RemoteEvent> = load_sources(state)
        .iter()
        .flat_map(|source| load_cache(state, &source.id))
        .collect();
    all.sort_by(|a, b| (a.start_unix_ms, &a.title).cmp(&(b.start_unix_ms, &b.title)));
    all
}

/// Ein verbundener Termin als Termin des Planers (für Agenda-Text und Wochenplan).
///
/// Warum die Mindestlänge: Der Planer verlangt Ende nach Beginn; Termine ohne Dauer bekommen
/// 15 Minuten. Ganztagstermine behalten ihren Titel mit Hinweis, weil ihre Uhrzeit nur eine Näherung ist.
pub(crate) fn as_scheduler_event(event: &RemoteEvent) -> pa_scheduler::Event {
    let title = if event.all_day {
        format!("{} (ganztägig)", event.title)
    } else {
        event.title.clone()
    };
    pa_scheduler::Event {
        id: event.id.clone(),
        title,
        start_unix_ms: event.start_unix_ms,
        end_unix_ms: event.end_unix_ms.max(event.start_unix_ms + 15 * 60_000),
        location: event.location.clone(),
        project: Some(event.calendar_name.clone()),
        external_uid: None,
    }
}

fn notify_changed(app: &AppHandle) {
    let _ = app.emit("calendar-changed", ());
}

fn new_source_id() -> String {
    format!("src-{:08x}", OsRng.next_u32())
}

// ---------------------------------------------------------------------------------------------
// Policy-Wächter
// ---------------------------------------------------------------------------------------------

/// Baut den Wächter, der unter jeder Anfrage läuft: Policy (Air Gap, Host, Aktion) und Audit.
fn gate_for(app: &AppHandle) -> impl Fn(&str, CapabilityAction) -> Result<(), String> + Sync {
    let app = app.clone();
    move |host: &str, action: CapabilityAction| {
        let state = app.state::<AppState>();
        // Der Air Gap wird bei jeder Anfrage frisch gelesen.
        let decision = authorize_calendar(net::air_gap_on(), host, action);
        lifecycle::audit_decision(
            &state,
            action,
            Some(host.to_owned()),
            &decision,
            "Verbindung zum Kalender",
        );
        match decision {
            Decision::Allow(_) => Ok(()),
            Decision::Prompt(reason) | Decision::Deny(reason) => Err(reason),
        }
    }
}

fn busy_guard() -> AppResult<std::sync::MutexGuard<'static, ()>> {
    match BUSY.try_lock() {
        Ok(guard) => Ok(guard),
        Err(TryLockError::Poisoned(poisoned)) => Ok(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => Err(AppError::Invalid(
            "Es läuft schon ein Kalender-Vorgang. Warte kurz.".to_owned(),
        )),
    }
}

// ---------------------------------------------------------------------------------------------
// Befehle
// ---------------------------------------------------------------------------------------------

/// Übersicht für die Oberfläche: Air Gap und Quellen (ohne Zugangsdaten).
#[derive(Debug, Serialize)]
pub struct CalendarOverview {
    pub air_gap: bool,
    pub sources: Vec<Source>,
    /// Ob es auf dieser Plattform einen geprüften HTTPS-Client gibt.
    pub supported: bool,
}

#[tauri::command]
pub fn calendar_overview(state: State<'_, AppState>) -> AppResult<CalendarOverview> {
    Ok(CalendarOverview {
        air_gap: net::air_gap_on(),
        sources: load_sources(&state),
        supported: cfg!(windows),
    })
}

/// Alle Termine aus dem Zwischenspeicher. Verbindet sich nie.
#[tauri::command]
pub fn calendar_events(state: State<'_, AppState>) -> AppResult<Vec<RemoteEvent>> {
    Ok(cached_events(&state))
}

/// Meldet sich mit Apple-ID und app-spezifischem Passwort an und listet die Kalender.
/// Speichert nichts; die Auswahl folgt in `calendar_add_icloud`.
#[tauri::command]
pub async fn calendar_icloud_discover(
    app: AppHandle,
    apple_id: String,
    app_password: String,
) -> AppResult<Vec<CalendarRef>> {
    let user = clean_apple_id(&apple_id).map_err(AppError::Invalid)?;
    let password = clean_app_password(&app_password).map_err(AppError::Invalid)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy_guard()?;
        let gate = gate_for(&app);
        let transport = Gated {
            inner: &net::SystemTransport,
            gate: &gate,
        };
        let found = caldav::discover(
            &transport,
            &Credentials {
                user: &user,
                password: &password,
            },
        )
        .map_err(invalid)?;
        if found.is_empty() {
            return Err(AppError::Invalid(
                "In diesem Konto wurde kein Kalender für Termine gefunden.".to_owned(),
            ));
        }
        Ok(found
            .into_iter()
            .map(|f| CalendarRef {
                host: f.host,
                href: f.href,
                name: f.name,
                color: f.color,
                can_write: f.can_write,
            })
            .collect())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

fn add_source(
    state: &AppState,
    kind: SourceKind,
    label: String,
    account: String,
    calendars: Vec<CalendarRef>,
    secret: &str,
) -> AppResult<Source> {
    let mut sources = load_sources(state);
    if sources.len() >= MAX_SOURCES {
        return Err(AppError::Invalid(format!(
            "Es sind höchstens {MAX_SOURCES} Verbindungen möglich."
        )));
    }
    let source = Source {
        id: new_source_id(),
        kind,
        label,
        account,
        calendars,
        last_sync_unix_ms: None,
        last_error: None,
        unsupported_rules: 0,
    };
    // Erst das Geheimnis, dann die Liste: eine Verbindung ohne Zugang kann es nie geben.
    write_setting(state, &secret_key(&source.id), secret)?;
    sources.push(source.clone());
    save_sources(state, &sources)?;
    Ok(source)
}

/// Speichert die Verbindung zu iCloud mit den gewählten Kalendern. Ruft nichts ab.
#[tauri::command]
pub fn calendar_add_icloud(
    app: AppHandle,
    state: State<'_, AppState>,
    label: String,
    apple_id: String,
    app_password: String,
    calendars: Vec<CalendarRef>,
) -> AppResult<Source> {
    let user = clean_apple_id(&apple_id).map_err(AppError::Invalid)?;
    let password = clean_app_password(&app_password).map_err(AppError::Invalid)?;
    check_calendars(&calendars).map_err(AppError::Invalid)?;
    let label = clean_label(&label, "Apple Kalender").map_err(AppError::Invalid)?;
    let source = add_source(
        &state,
        SourceKind::Icloud,
        label,
        user,
        calendars,
        &password,
    )?;
    notify_changed(&app);
    Ok(source)
}

/// Speichert die geheime iCal-Adresse eines Google-Kalenders. Ruft nichts ab.
#[tauri::command]
pub fn calendar_add_google(
    app: AppHandle,
    state: State<'_, AppState>,
    label: String,
    url: String,
) -> AppResult<Source> {
    crate::calendar::ics_url::parse_url(&url).map_err(invalid)?;
    let label = clean_label(&label, "Google Kalender").map_err(AppError::Invalid)?;
    let calendars = vec![CalendarRef {
        host: crate::calendar::ics_url::GOOGLE_ICS_HOST.to_owned(),
        href: String::new(),
        name: label.clone(),
        color: None,
        can_write: false,
    }];
    let source = add_source(
        &state,
        SourceKind::GoogleIcs,
        label,
        String::new(),
        calendars,
        url.trim(),
    )?;
    notify_changed(&app);
    Ok(source)
}

/// Trennt eine Quelle: Zugang, Liste und Zwischenspeicher werden geleert.
#[tauri::command]
pub fn calendar_remove_source(
    app: AppHandle,
    state: State<'_, AppState>,
    source_id: String,
) -> AppResult<()> {
    let _busy = busy_guard()?;
    let mut sources = load_sources(&state);
    if !sources.iter().any(|s| s.id == source_id) {
        return Err(AppError::Invalid(
            "Diese Verbindung gibt es nicht mehr.".to_owned(),
        ));
    }
    // Zuerst Geheimnis und Zwischenspeicher leeren, zuletzt die Liste.
    write_setting(&state, &secret_key(&source_id), "")?;
    write_setting(&state, &cache_key(&source_id), "[]")?;
    sources.retain(|s| s.id != source_id);
    save_sources(&state, &sources)?;
    notify_changed(&app);
    Ok(())
}

/// Ergebnis des Abrufs einer Quelle.
#[derive(Debug, Serialize)]
pub struct SyncReport {
    pub source_id: String,
    pub ok: bool,
    pub count: usize,
    pub error: Option<String>,
}

/// Ruft eine oder alle Quellen ab. Schlägt ein Abruf fehl, bleibt der alte Stand sichtbar und der
/// Fehler steht an der Quelle.
#[tauri::command]
pub async fn calendar_sync(
    app: AppHandle,
    source_id: Option<String>,
    tz_offset_minutes: i32,
) -> AppResult<Vec<SyncReport>> {
    tauri::async_runtime::spawn_blocking(move || {
        sync_blocking(&app, source_id.as_deref(), tz_offset_minutes)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

fn sync_blocking(
    app: &AppHandle,
    only: Option<&str>,
    tz_offset_minutes: i32,
) -> AppResult<Vec<SyncReport>> {
    let _busy = busy_guard()?;
    let state = app.state::<AppState>();
    let sources: Vec<Source> = load_sources(&state)
        .into_iter()
        .filter(|s| only.is_none_or(|id| id == s.id))
        .collect();
    if sources.is_empty() {
        return Err(AppError::Invalid(
            "Es ist kein Kalender verbunden.".to_owned(),
        ));
    }
    let gate = gate_for(app);
    let transport = Gated {
        inner: &net::SystemTransport,
        gate: &gate,
    };
    let window = sync::window_around(now_unix_ms());
    let mut reports = Vec::new();
    for source in sources {
        let outcome = load_secret(&state, &source.id)
            .map_err(|e| e.to_string())
            .and_then(|secret| {
                sync::sync_source(&transport, &source, &secret, window, tz_offset_minutes)
                    .map_err(|e| e.to_string())
            });
        // Die Liste frisch laden: ein anderer Vorgang darf keine Änderung überschreiben.
        let mut current = load_sources(&state);
        let mut report = SyncReport {
            source_id: source.id.clone(),
            ok: false,
            count: 0,
            error: None,
        };
        match outcome {
            Ok(result) => {
                report.count = result.events.len();
                match save_cache(&state, &source.id, &result.events) {
                    Ok(()) => {
                        report.ok = true;
                        if let Some(entry) = current.iter_mut().find(|s| s.id == source.id) {
                            entry.last_sync_unix_ms = Some(now_unix_ms());
                            entry.last_error = None;
                            entry.unsupported_rules = result.unsupported_rules;
                        }
                    }
                    Err(error) => report.error = Some(error.to_string()),
                }
            }
            Err(message) => report.error = Some(message),
        }
        if let (Some(message), Some(entry)) = (
            &report.error,
            current.iter_mut().find(|s| s.id == source.id),
        ) {
            entry.last_error = Some(message.chars().take(200).collect());
        }
        let _ = save_sources(&state, &current);
        reports.push(report);
    }
    notify_changed(app);
    Ok(reports)
}

/// Legt einen Termin im gewählten Apple-Kalender an und merkt ihn im Zwischenspeicher.
#[tauri::command]
pub async fn calendar_create_event(
    app: AppHandle,
    source_id: String,
    calendar_href: String,
    event: NewEvent,
    tz_offset_minutes: i32,
) -> AppResult<RemoteEvent> {
    tauri::async_runtime::spawn_blocking(move || {
        let _busy = busy_guard()?;
        let state = app.state::<AppState>();
        let source = load_sources(&state)
            .into_iter()
            .find(|s| s.id == source_id)
            .ok_or_else(|| AppError::Invalid("Diese Verbindung gibt es nicht mehr.".to_owned()))?;
        let secret = load_secret(&state, &source.id)?;
        let gate = gate_for(&app);
        let transport = Gated {
            inner: &net::SystemTransport,
            gate: &gate,
        };
        let mut entropy = [0_u8; 16];
        OsRng.fill_bytes(&mut entropy);
        let created = sync::create_in_icloud(
            &transport,
            &source,
            &secret,
            &sync::CreateRequest {
                calendar_href: &calendar_href,
                event: &event,
                now_unix_ms: now_unix_ms(),
                local_offset_minutes: tz_offset_minutes,
                entropy,
            },
        )
        .map_err(invalid)?;
        // Der Termin liegt jetzt im Kalender; fehlt nur der Zwischenspeicher, holt der nächste Abruf ihn.
        let mut cache = load_cache(&state, &source.id);
        cache.retain(|e| e.id != created.id);
        cache.push(created.clone());
        let _ = save_cache(&state, &source.id, &cache);
        notify_changed(&app);
        Ok(created)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_ids_must_be_a_plain_email_without_colons_or_spaces() {
        assert_eq!(
            clean_apple_id("  anna@icloud.com ").unwrap(),
            "anna@icloud.com"
        );
        for bad in [
            "",
            "anna",
            "@icloud.com",
            "anna@",
            "a@b@c",
            "an na@icloud.com",
            "anna:x@icloud.com",
            "anna@icloud.com\r\nX: y",
        ] {
            assert!(clean_apple_id(bad).is_err(), "{bad:?}");
        }
        assert!(clean_apple_id(&format!("{}@x.at", "a".repeat(260))).is_err());
    }

    #[test]
    fn app_passwords_are_cleaned_and_normal_apple_passwords_are_not_pushed_through_blindly() {
        assert_eq!(
            clean_app_password(" abcd-efgh-ijkl-mnop\n").unwrap(),
            "abcd-efgh-ijkl-mnop"
        );
        assert_eq!(
            clean_app_password("abcd efgh ijkl mnop").unwrap(),
            "abcdefghijklmnop"
        );
        for bad in [
            "",
            "kurz",
            "mit:doppelpunkt1234",
            "ä-ö-ü-ä-ö-ü-ä-ö",
            &"a".repeat(65),
            "abcd-efgh-ijkl-mnop!",
        ] {
            assert!(clean_app_password(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn labels_default_and_stay_short_and_clean() {
        assert_eq!(
            clean_label("  ", "Apple Kalender").unwrap(),
            "Apple Kalender"
        );
        assert_eq!(clean_label(" Familie ", "x").unwrap(), "Familie");
        assert!(clean_label("a\nb", "x").is_err());
        assert!(clean_label(&"a".repeat(61), "x").is_err());
    }

    fn cal(host: &str, href: &str) -> CalendarRef {
        CalendarRef {
            host: host.to_owned(),
            href: href.to_owned(),
            name: "K".to_owned(),
            color: None,
            can_write: true,
        }
    }

    #[test]
    fn calendars_from_the_ui_must_stay_on_the_host_table_with_safe_paths() {
        assert!(check_calendars(&[cal("p12-caldav.icloud.com", "/1/calendars/home/")]).is_ok());
        assert!(check_calendars(&[]).is_err());
        for (host, href) in [
            ("evil.example", "/1/calendars/home/"),
            ("p12-caldav.icloud.com.evil.example", "/1/calendars/home/"),
            ("p12-caldav.icloud.com", "/1/calendars/home"),
            ("p12-caldav.icloud.com", "//evil/"),
            ("p12-caldav.icloud.com", "/1/../2/"),
            ("p12-caldav.icloud.com", "/1/?x=1/"),
            ("p12-caldav.icloud.com", "1/calendars/"),
        ] {
            assert!(check_calendars(&[cal(host, href)]).is_err(), "{host}{href}");
        }
    }

    #[test]
    fn remote_events_become_planner_events_with_a_minimum_length() {
        let mut event = RemoteEvent {
            id: "src-1|0|a|5".to_owned(),
            source_id: "src-1".to_owned(),
            source_label: "Apple".to_owned(),
            calendar_name: "Privat".to_owned(),
            color: None,
            title: "Anruf".to_owned(),
            start_unix_ms: 1_000_000,
            end_unix_ms: 1_000_000,
            all_day: false,
            start_date: None,
            end_date: None,
            location: Some("Büro".to_owned()),
            notes: None,
            recurring: false,
        };
        let planner = as_scheduler_event(&event);
        assert_eq!(
            (planner.title.as_str(), planner.end_unix_ms),
            ("Anruf", 1_000_000 + 900_000)
        );
        assert_eq!(planner.project.as_deref(), Some("Privat"));
        event.all_day = true;
        event.end_unix_ms = 1_000_000 + 86_400_000;
        let planner = as_scheduler_event(&event);
        assert_eq!(planner.title, "Anruf (ganztägig)");
        assert_eq!(planner.end_unix_ms, 1_000_000 + 86_400_000);
    }

    #[test]
    fn keys_for_secrets_and_caches_are_per_source() {
        assert_eq!(secret_key("src-1"), "calendar.secret.src-1");
        assert_eq!(cache_key("src-1"), "calendar.events.src-1");
        assert_ne!(new_source_id(), new_source_id());
    }
}
