//! Gmail: Einstellungen prüfen, Zugangsdaten verwahren, Prüflauf und Takt starten.
//!
//! Das App-Passwort liegt als eigenes Setting im Tresor und wird nie an die Oberfläche
//! zurückgegeben. Der Takt startet nach jedem Entsperren AUS; es gibt keine stille Wiederaufnahme.
//! Alle Sperren der Auto-Antwort sitzen im Modul `mail` und werden hier nur verdrahtet.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use pa_policy::{egress::authorize_mail, CapabilityAction, Decision};
use pa_types::{
    avatar::JobKind,
    chat::{Message, MessageRole, MessageStatus, ThinkingLevel},
    ipc::{ConnectorConfig, MailReplyMode},
};
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::mail::{
    engine::{run_cycle, CycleReport, Deps, ReplyMode, Settings},
    guard::{LogEntry, LogKind, MailState},
    imap::{ImapClient, Search},
    message::single_address,
    poller::{
        interval_ms, next_action, Action, StopReason, MAX_INTERVAL_MINUTES, MIN_INTERVAL_MINUTES,
    },
    tls::SystemMailTransport,
    MailError, MailTransport, IMAP_ENDPOINT,
};
use crate::{lifecycle, net, now_unix_ms, require_session, AppError, AppResult, AppState};

const SETTING_PASSWORD: &str = "mail.password";
const SETTING_STATE: &str = "mail.state";
/// Längste Zusatzanweisung in Zeichen.
const MAX_INSTRUCTION_CHARS: usize = 1_000;
/// Längste Modellantwort in Zeichen (grobe Obergrenze für einen Mailentwurf).
const MAX_MODEL_CHARS: usize = 4_000;

/// Prüft die Gmail-Einstellungen. Ungültiges wird abgelehnt statt stillschweigend korrigiert.
pub fn validate_config(config: &ConnectorConfig) -> Result<(), String> {
    if !(MIN_INTERVAL_MINUTES..=MAX_INTERVAL_MINUTES).contains(&config.gmail_check_interval_minutes)
    {
        return Err(format!(
            "Der Abstand muss zwischen {MIN_INTERVAL_MINUTES} und {MAX_INTERVAL_MINUTES} Minuten liegen."
        ));
    }
    if !(1..=60).contains(&config.gmail_max_per_hour)
        || !(1..=200).contains(&config.gmail_max_per_day)
    {
        return Err(
            "Die Limits müssen zwischen 1 und 60 pro Stunde und 1 und 200 pro Tag liegen."
                .to_owned(),
        );
    }
    if config.gmail_max_per_day < config.gmail_max_per_hour {
        return Err("Das Tageslimit darf nicht unter dem Stundenlimit liegen.".to_owned());
    }
    if config.gmail_instruction.chars().count() > MAX_INSTRUCTION_CHARS {
        return Err(format!(
            "Die Anweisung ist zu lang (höchstens {MAX_INSTRUCTION_CHARS} Zeichen)."
        ));
    }
    let address = config.gmail_address.trim();
    if !address.is_empty() {
        let valid = single_address(address)
            .is_some_and(|a| a.ends_with("@gmail.com") || a.ends_with("@googlemail.com"));
        if !valid {
            return Err("Das Konto muss eine Gmail-Adresse sein (…@gmail.com).".to_owned());
        }
    }
    let target = config.gmail_target_email.trim();
    if !target.is_empty() && single_address(target).is_none() {
        return Err("Die Adresse für die Auto-Antwort ist ungültig.".to_owned());
    }
    if config.gmail_enabled && (address.is_empty() || target.is_empty()) {
        return Err(
            "Trage dein Gmail-Konto und die Adresse ein, der IAP antworten soll.".to_owned(),
        );
    }
    if config.gmail_reply_mode == MailReplyMode::Send && !config.gmail_send_acknowledged {
        return Err("Automatisches Senden braucht deine ausdrückliche Bestätigung.".to_owned());
    }
    Ok(())
}

/// Bringt Einstellungen in eine konsistente Form: Die Bestätigung gilt nur für den Sendemodus.
pub fn normalize_config(config: &mut ConnectorConfig) {
    config.gmail_address = config.gmail_address.trim().to_owned();
    config.gmail_target_email = config.gmail_target_email.trim().to_owned();
    if config.gmail_reply_mode != MailReplyMode::Send {
        config.gmail_send_acknowledged = false;
    }
}

pub(crate) fn read_setting(state: &AppState, key: &str) -> AppResult<Option<String>> {
    let session = require_session(state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(vault.repository().setting(key)?)
}

pub(crate) fn write_setting(state: &AppState, key: &str, value: &str) -> AppResult<()> {
    let session = require_session(state)?;
    let mut vault = session.vault_runtime.lock()?;
    vault.repository_mut().set_setting(key, value)?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

fn load_state(state: &AppState) -> MailState {
    read_setting(state, SETTING_STATE)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_state(state: &AppState, mail_state: &MailState) {
    if let Ok(text) = serde_json::to_string(mail_state) {
        let _ = write_setting(state, SETTING_STATE, &text);
    }
}

fn has_password(state: &AppState) -> bool {
    read_setting(state, SETTING_PASSWORD)
        .ok()
        .flatten()
        .is_some_and(|p| !p.is_empty())
}

/// Liest die Einstellungen für einen Lauf und prüft, ob alles da ist.
fn load_settings(state: &AppState) -> Result<(Settings, ConnectorConfig), MailError> {
    let config = crate::lock(&state.connector_config)
        .map_err(|e| MailError::Blocked(e.to_string()))?
        .clone();
    if !config.gmail_enabled {
        return Err(MailError::Blocked(
            "Gmail ist in den Einstellungen ausgeschaltet.".to_owned(),
        ));
    }
    validate_config(&config).map_err(MailError::Blocked)?;
    let password = read_setting(state, SETTING_PASSWORD)
        .map_err(|e| MailError::Blocked(e.to_string()))?
        .filter(|p| !p.is_empty())
        .ok_or_else(|| MailError::Blocked("Es ist kein App-Passwort hinterlegt.".to_owned()))?;
    let mode = match config.gmail_reply_mode {
        MailReplyMode::Draft => ReplyMode::Draft,
        MailReplyMode::Send => ReplyMode::Send,
    };
    Ok((
        Settings {
            address: config.gmail_address.trim().to_ascii_lowercase(),
            password,
            target: config.gmail_target_email.trim().to_ascii_lowercase(),
            mode,
            max_per_hour: config.gmail_max_per_hour,
            max_per_day: config.gmail_max_per_day,
            instruction: config.gmail_instruction.clone(),
        },
        config,
    ))
}

/// Verdrahtung des Prüflaufs mit Policy, Audit, Modell und Tresor.
struct AppDeps {
    app: AppHandle,
    job_cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl Deps for AppDeps {
    fn now_ms(&self) -> i64 {
        now_unix_ms()
    }

    fn authorize(&mut self, host: &str, action: CapabilityAction) -> Result<(), MailError> {
        let state = self.app.state::<AppState>();
        // Air Gap wird bei jedem Aufruf frisch gelesen.
        let decision = authorize_mail(net::air_gap_on(), host, action);
        lifecycle::audit_decision(
            &state,
            action,
            Some(host.to_owned()),
            &decision,
            "Verbindung zu Gmail",
        );
        match decision {
            Decision::Allow(_) => Ok(()),
            Decision::Prompt(reason) | Decision::Deny(reason) => Err(MailError::Blocked(reason)),
        }
    }

    fn generate(&mut self, system: &str, user: &str) -> Result<String, MailError> {
        let state = self.app.state::<AppState>();
        let session = require_session(&state).map_err(|e| MailError::Blocked(e.to_string()))?;
        let make = |position: i64, role: MessageRole, content: &str| Message {
            id: format!("mail-{position}"),
            conversation_id: "mail".to_owned(),
            position,
            role,
            content: content.to_owned(),
            status: MessageStatus::Complete,
            created_at_unix_ms: now_unix_ms(),
        };
        // Frischer Kontext: nur diese zwei Nachrichten und keine Werkzeuge.
        let messages = [
            make(0, MessageRole::System, system),
            make(1, MessageRole::User, user),
        ];
        let produced = std::cell::Cell::new(0_usize);
        let outcome = {
            let mut engine = session
                .engine
                .lock()
                .map_err(|_| MailError::Blocked("Modell-Sperre vergiftet".to_owned()))?;
            engine.set_thinking_level(ThinkingLevel::Kurz);
            let mut go =
                || !self.job_cancel.load(Ordering::SeqCst) && produced.get() < MAX_MODEL_CHARS;
            engine.stream_chat_with_grammar(&messages, None, &mut go, &mut |delta| {
                produced.set(produced.get() + delta.len());
                true
            })
        };
        if self.job_cancel.load(Ordering::SeqCst) {
            return Err(MailError::Blocked("Abgebrochen.".to_owned()));
        }
        outcome.map(|result| result.text).map_err(|error| {
            MailError::Blocked(format!(
                "Das Modell hat nicht geantwortet: {}",
                error.message
            ))
        })
    }

    fn persist(&mut self, mail_state: &MailState) {
        save_state(&self.app.state::<AppState>(), mail_state);
    }

    fn audit_reply(&mut self, action: CapabilityAction, to: &str, chars: usize) {
        let state = self.app.state::<AppState>();
        // Nur Adresse und Zeichenzahl, nie der Inhalt.
        lifecycle::audit_decision(
            &state,
            action,
            Some(format!("{to} ({chars} Zeichen)")),
            &Decision::Allow(pa_policy::Capability {
                action,
                canonical_path: None,
            }),
            "Auto-Antwort vorbereitet",
        );
    }

    fn cancelled(&self) -> bool {
        // Air Gap beendet den Lauf mitten in der Schleife, nicht erst beim nächsten Takt.
        self.job_cancel.load(Ordering::SeqCst) || net::air_gap_on()
    }

    fn unique(&mut self) -> u64 {
        OsRng.next_u64()
    }
}

fn set_status(state: &AppState, update: impl FnOnce(&mut crate::mail::poller::RunStatus)) {
    if let Ok(mut status) = state.mail.status.lock() {
        update(&mut status);
    }
}

fn notify_changed(app: &AppHandle) {
    let _ = app.emit("mail-changed", ());
}

/// Ein Prüflauf in der Job-Warteschlange (sichtbar und abbrechbar).
fn execute_cycle(app: &AppHandle) -> Result<CycleReport, MailError> {
    let state = app.state::<AppState>();
    let (settings, _) = load_settings(&state)?;
    if net::air_gap_on() {
        return Err(MailError::Blocked(
            "Air Gap ist eingeschaltet: externe Anfragen sind gesperrt".to_owned(),
        ));
    }
    let mut job = state
        .flow
        .jobs
        .submit(JobKind::Mail, "Gmail: Postfach prüfen", true);
    let job_cancel = job.cancel_flag();
    let mut slot = job
        .acquire()
        .map_err(|_| MailError::Blocked("Abgebrochen.".to_owned()))?;
    set_status(&state, |s| s.running = true);
    notify_changed(app);

    let mut mail_state = load_state(&state);
    let mut deferred: HashSet<String> = state
        .mail
        .deferred
        .lock()
        .map(|d| d.clone())
        .unwrap_or_default();
    let mut deps = AppDeps {
        app: app.clone(),
        job_cancel: Arc::clone(&job_cancel),
    };
    let result = run_cycle(
        &mut deps,
        &SystemMailTransport,
        &settings,
        &mut mail_state,
        &mut deferred,
    );
    if let Ok(mut shared) = state.mail.deferred.lock() {
        *shared = deferred;
    }
    let now = now_unix_ms();
    match &result {
        Ok(_) => {}
        Err(error) => {
            // Der Fehler gehört ins sichtbare Protokoll.
            mail_state.push_log(LogEntry {
                at_unix_ms: now,
                kind: LogKind::Error,
                subject: String::new(),
                detail: error.to_string().chars().take(160).collect(),
            });
            slot.fail();
        }
    }
    save_state(&state, &mail_state);
    set_status(&state, |s| {
        s.running = false;
        s.last_run_unix_ms = Some(now);
        s.last_error = result.as_ref().err().map(ToString::to_string);
    });
    notify_changed(app);
    result
}

/// Status für die Oberfläche (ohne Zugangsdaten).
#[derive(Debug, Clone, Serialize)]
pub struct MailStatus {
    /// Auf dieser Plattform gibt es eine geprüfte Verschlüsselung.
    pub supported: bool,
    pub enabled: bool,
    pub has_password: bool,
    pub polling: bool,
    pub running: bool,
    pub last_run_unix_ms: Option<i64>,
    pub next_run_unix_ms: Option<i64>,
    pub last_error: Option<String>,
    pub sent_last_hour: u32,
    pub sent_today: u32,
    /// Warum der Takt jetzt nicht starten kann (deutscher Text für `t()`), sonst `None`.
    pub blocked_reason: Option<String>,
}

fn blocked_reason(state: &AppState, config: &ConnectorConfig, password: bool) -> Option<String> {
    if net::air_gap_on() {
        Some("Air Gap ist eingeschaltet. Schalte ihn in der Seitenleiste aus.".to_owned())
    } else if !config.gmail_enabled {
        Some("Gmail ist ausgeschaltet.".to_owned())
    } else if !password {
        Some("Es ist kein App-Passwort hinterlegt.".to_owned())
    } else if validate_config(config).is_err() {
        Some("Die Einstellungen sind unvollständig.".to_owned())
    } else if !cfg!(windows) {
        Some("Mail ist auf diesem System noch nicht eingerichtet.".to_owned())
    } else {
        let _ = state;
        None
    }
}

/// Zustand der Gmail-Anbindung.
#[tauri::command]
pub fn mail_status(state: State<'_, AppState>) -> AppResult<MailStatus> {
    let config = crate::lock(&state.connector_config)?.clone();
    let password = has_password(&state);
    let mail_state = load_state(&state);
    let now = now_unix_ms();
    let run = state
        .mail
        .status
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    Ok(MailStatus {
        supported: cfg!(windows),
        enabled: config.gmail_enabled,
        has_password: password,
        polling: state.mail.is_polling(),
        running: run.running,
        last_run_unix_ms: run.last_run_unix_ms,
        next_run_unix_ms: run.next_run_unix_ms,
        last_error: run.last_error,
        sent_last_hour: mail_state.sent_within(now, 3_600_000),
        sent_today: mail_state.sent_within(now, 86_400_000),
        blocked_reason: blocked_reason(&state, &config, password),
    })
}

/// Speichert das App-Passwort im Tresor. Es wird nie zurückgegeben.
#[tauri::command]
pub fn mail_set_password(state: State<'_, AppState>, password: String) -> AppResult<()> {
    let cleaned: String = password.chars().filter(|c| !c.is_whitespace()).collect();
    // Google-App-Passwörter haben 16 Kleinbuchstaben; etwas Spielraum für Änderungen auf Googles Seite.
    if cleaned.len() < 8
        || cleaned.len() > 64
        || !cleaned.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(AppError::Invalid(
            "Das App-Passwort besteht aus Buchstaben und Ziffern (bei Google 16 Zeichen)."
                .to_owned(),
        ));
    }
    write_setting(&state, SETTING_PASSWORD, &cleaned)
}

/// Entfernt das App-Passwort und stoppt den Takt.
#[tauri::command]
pub fn mail_clear_password(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    state.mail.stop();
    write_setting(&state, SETTING_PASSWORD, "")?;
    notify_changed(&app);
    Ok(())
}

/// Testet Anmeldung und Postfachzugriff (nur lesend).
#[tauri::command]
pub async fn mail_test_connection(app: AppHandle) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || test_blocking(&app))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

fn test_blocking(app: &AppHandle) -> AppResult<String> {
    let state = app.state::<AppState>();
    let (settings, _) = load_settings(&state).map_err(|e| AppError::Invalid(e.to_string()))?;
    let mut deps = AppDeps {
        app: app.clone(),
        job_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let outcome = (|| -> Result<usize, MailError> {
        deps.authorize(IMAP_ENDPOINT.0, CapabilityAction::MailRead)?;
        let stream = SystemMailTransport.connect(
            IMAP_ENDPOINT.0,
            IMAP_ENDPOINT.1,
            Duration::from_secs(20),
        )?;
        let mut imap = ImapClient::new(stream, 64 * 1024)?;
        imap.login(&settings.address, &settings.password)?;
        imap.examine_inbox()?;
        let unread = imap.search(&Search {
            unseen: true,
            from: Some(settings.target.clone()),
            ..Search::default()
        })?;
        imap.logout();
        Ok(unread.len())
    })();
    match outcome {
        Ok(count) => Ok(format!("{count}")),
        Err(error) => Err(AppError::Invalid(error.to_string())),
    }
}

/// Einmal sofort prüfen, ohne den Takt zu starten.
#[tauri::command]
pub async fn mail_run_now(app: AppHandle) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        execute_cycle(&app)
            .map(|_| ())
            .map_err(|e| AppError::Invalid(e.to_string()))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

/// Schaltet den Takt ein oder aus. Er startet nur, wenn alles bereit ist.
#[tauri::command]
pub fn mail_set_polling(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<()> {
    if !enabled {
        state.mail.stop();
        notify_changed(&app);
        return Ok(());
    }
    let config = crate::lock(&state.connector_config)?.clone();
    if let Some(reason) = blocked_reason(&state, &config, has_password(&state)) {
        return Err(AppError::Invalid(reason));
    }
    let Some(generation) = state.mail.start() else {
        return Ok(());
    };
    let handle = app.clone();
    thread::spawn(move || poll_loop(&handle, generation));
    notify_changed(&app);
    Ok(())
}

/// Der Takt: schläft in kurzen Schritten, damit Schalter, Air Gap und Sperren sofort greifen.
fn poll_loop(app: &AppHandle, generation: u64) {
    let state = app.state::<AppState>();
    let mut due = now_unix_ms();
    set_status(&state, |s| s.next_run_unix_ms = Some(due));
    loop {
        let session_open = require_session(&state).is_ok();
        let now = now_unix_ms();
        match next_action(
            generation,
            state.mail.generation(),
            net::air_gap_on(),
            session_open,
            now,
            due,
        ) {
            Action::Stop(reason) => {
                finish_loop(app, &state, generation, reason);
                return;
            }
            Action::Wait => thread::sleep(Duration::from_millis(1000)),
            Action::Run => {
                let result = execute_cycle(app);
                let minutes = crate::lock(&state.connector_config)
                    .map(|c| c.gmail_check_interval_minutes)
                    .unwrap_or(5);
                due = now_unix_ms() + interval_ms(minutes);
                set_status(&state, |s| s.next_run_unix_ms = Some(due));
                notify_changed(app);
                // Eine falsche Anmeldung wird nicht alle fünf Minuten wiederholt.
                if matches!(result, Err(MailError::Auth)) && state.mail.generation() == generation {
                    state.mail.stop();
                    notify_changed(app);
                    return;
                }
            }
        }
    }
}

fn finish_loop(app: &AppHandle, state: &AppState, generation: u64, reason: StopReason) {
    // Nur das noch aktuelle Takt-Exemplar räumt auf; ein überholter hinterlässt nichts.
    if reason != StopReason::Switched && state.mail.generation() == generation {
        state.mail.stop();
        if let Ok(session) = require_session(state) {
            let _ = session;
            let mut mail_state = load_state(state);
            mail_state.push_log(LogEntry {
                at_unix_ms: now_unix_ms(),
                kind: LogKind::Info,
                subject: String::new(),
                detail: match reason {
                    StopReason::AirGap => "stopped_air_gap".to_owned(),
                    _ => "stopped_session".to_owned(),
                },
            });
            save_state(state, &mail_state);
        }
    }
    notify_changed(app);
}

/// Not-Aus: Takt aus, laufenden Prüflauf abbrechen, Sendemodus zurück auf Entwurf.
#[tauri::command]
pub fn mail_emergency_stop(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ConnectorConfig> {
    state.mail.stop();
    state.flow.jobs.cancel_kinds(&[JobKind::Mail]);
    let config = {
        let mut guard = crate::lock(&state.connector_config)?;
        guard.gmail_reply_mode = MailReplyMode::Draft;
        guard.gmail_send_acknowledged = false;
        guard.clone()
    };
    lifecycle::persist_connector_config(&state, &config)?;
    let mut mail_state = load_state(&state);
    mail_state.push_log(LogEntry {
        at_unix_ms: now_unix_ms(),
        kind: LogKind::Info,
        subject: String::new(),
        detail: "emergency_stop".to_owned(),
    });
    save_state(&state, &mail_state);
    notify_changed(&app);
    Ok(config)
}

/// Das sichtbare Protokoll, neueste zuerst.
#[tauri::command]
pub fn mail_log(state: State<'_, AppState>) -> AppResult<Vec<LogEntry>> {
    let mut log = load_state(&state).log;
    log.reverse();
    Ok(log)
}

/// Leert das Protokoll. Die Liste der beantworteten Mails bleibt erhalten, sonst käme es zu Doppelantworten.
#[tauri::command]
pub fn mail_clear_log(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let mut mail_state = load_state(&state);
    mail_state.log.clear();
    save_state(&state, &mail_state);
    notify_changed(&app);
    Ok(())
}

/// Stoppt den Takt (Sperren, Beenden, neuer Tresor).
pub fn stop_polling(state: &AppState) {
    state.mail.stop();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> ConnectorConfig {
        ConnectorConfig {
            gmail_enabled: true,
            gmail_address: "bot@gmail.com".into(),
            gmail_target_email: "anna@example.com".into(),
            ..ConnectorConfig::default()
        }
    }

    #[test]
    fn a_complete_default_config_is_valid() {
        assert!(validate_config(&valid()).is_ok());
        assert!(validate_config(&ConnectorConfig::default()).is_ok());
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        let bad = |change: fn(&mut ConnectorConfig)| {
            let mut config = valid();
            change(&mut config);
            validate_config(&config).is_err()
        };
        assert!(bad(|c| c.gmail_check_interval_minutes = 0));
        assert!(bad(|c| c.gmail_check_interval_minutes = 61));
        assert!(bad(|c| c.gmail_max_per_hour = 0));
        assert!(bad(|c| c.gmail_max_per_hour = 61));
        assert!(bad(|c| c.gmail_max_per_day = 201));
        assert!(bad(|c| {
            c.gmail_max_per_hour = 10;
            c.gmail_max_per_day = 5;
        }));
        assert!(bad(|c| c.gmail_instruction = "x".repeat(1_001)));
        assert!(bad(|c| c.gmail_address = "bot@firma.de".into()));
        assert!(bad(|c| c.gmail_address = "a@gmail.com, b@gmail.com".into()));
        assert!(bad(|c| c.gmail_target_email = "kein-at".into()));
        assert!(bad(|c| c.gmail_target_email = String::new()));
    }

    #[test]
    fn automatic_sending_needs_the_acknowledgement_in_the_backend() {
        let mut config = valid();
        config.gmail_reply_mode = MailReplyMode::Send;
        assert!(validate_config(&config).is_err());
        config.gmail_send_acknowledged = true;
        assert!(validate_config(&config).is_ok());
        // Zurück auf Entwurf löscht die Bestätigung.
        config.gmail_reply_mode = MailReplyMode::Draft;
        normalize_config(&mut config);
        assert!(!config.gmail_send_acknowledged);
    }

    #[test]
    fn config_is_normalised() {
        let mut config = valid();
        config.gmail_address = "  bot@gmail.com ".into();
        normalize_config(&mut config);
        assert_eq!(config.gmail_address, "bot@gmail.com");
    }
}
