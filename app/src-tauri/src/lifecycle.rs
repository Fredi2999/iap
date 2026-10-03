//! Gemeinsamer Laufzeitzustand für Flow Version, Sprache und Pet.
//!
//! Hier liegt, was Hauptseite, Chat, Pet und Flow gemeinsam brauchen:
//!
//! - die **aktive Unterhaltung** im Backend (nicht mehr lokal im Chat), damit
//!   ein Ansichts- oder Fensterwechsel dieselbe Unterhaltung behält;
//! - das sichtbare **Job-Register** mit der seriellen Modell-Queue;
//! - der **Fenstermodus** (Hauptfenster oder Pet);
//! - die **Persistenz der Konnektor-Einstellungen** im Tresor. Der Exa-Schlüssel
//!   liegt dadurch verschlüsselt im Tresor; der Air Gap startet trotzdem in
//!   jeder Sitzung EIN, damit Netz nie unbemerkt „aus dem Vorlauf“ aktiv ist.

use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        Mutex,
    },
    time::Duration,
};

use pa_core::jobs::JobQueue;
use pa_types::{
    avatar::{JobInfo, JobKind, WindowMode},
    ipc::ConnectorConfig,
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{lock, now_unix_ms, require_session, AppError, AppResult, AppState};

/// Nach so vielen Sekunden Ruhe im Pet-Modus wird das Sprachmodell entladen.
const IDLE_UNLOAD_SECONDS: i64 = 300;

/// Vault-Schlüssel der Konnektor-Einstellungen.
const SETTING_CONNECTOR_CONFIG: &str = "connector.config";

/// Arten von Arbeit, die beim Wechsel zum Pet kontrolliert stoppen. Chat und
/// Sprache gehören bewusst nicht dazu: das Pet darf weiter Gespräche führen.
const FLOW_KINDS: [JobKind; 2] = [JobKind::Workflow, JobKind::AgentFlow];

/// Geteilter Zustand; liegt als ein Feld in [`AppState`].
pub struct FlowRuntime {
    /// Serielle Modell-Queue und Job-Übersicht. Auf T0 gibt es einen Platz.
    pub jobs: JobQueue,
    active_conversation: Mutex<Option<String>>,
    window_mode: Mutex<WindowMode>,
    /// Pakete, deren Prüfsummen in dieser Sitzung schon geprüft wurden.
    pub verified_packs: Mutex<HashSet<String>>,
    /// Zeitpunkt (Unix-Millisekunden) der letzten Aktivität; steuert das Entladen im Leerlauf.
    last_activity_ms: AtomicI64,
    /// Das Sprachmodell wurde im Pet-Leerlauf beendet; die nächste Anfrage lädt es sichtbar neu.
    engine_unloaded: AtomicBool,
    /// Laufende Agent-Flow-Aufgaben (Abbruch-Flags, aktuelle Job-Nummern).
    pub agent_flow: crate::agent_flow::Runtime,
    /// Laufende Workflow-Läufe (Abbruch-Flags).
    pub workflows: crate::workflow_cmds::Runtime,
}

impl FlowRuntime {
    /// Neuer Zustand: Hauptfenster, keine aktive Unterhaltung, ein Queue-Platz.
    pub fn new() -> Self {
        Self {
            jobs: JobQueue::new(1),
            active_conversation: Mutex::new(None),
            window_mode: Mutex::new(WindowMode::Main),
            verified_packs: Mutex::new(HashSet::new()),
            last_activity_ms: AtomicI64::new(now_unix_ms()),
            engine_unloaded: AtomicBool::new(false),
            agent_flow: crate::agent_flow::Runtime::new(),
            workflows: crate::workflow_cmds::Runtime::new(),
        }
    }

    /// Leitet Job-Änderungen als Ereignis `jobs-changed` an die Oberfläche und
    /// zählt sie als Aktivität.
    pub fn attach_events(&self, app: &AppHandle) {
        let handle = app.clone();
        self.jobs.set_listener(move |jobs| {
            let _ = handle.emit("jobs-changed", jobs.to_vec());
            let state = handle.state::<AppState>();
            state.flow.touch_activity();
        });
    }

    /// Vermerkt Aktivität (Nutzeraktion, Auftrag, Sprache).
    pub fn touch_activity(&self) {
        self.last_activity_ms.store(now_unix_ms(), Ordering::SeqCst);
        self.engine_unloaded.store(false, Ordering::SeqCst);
    }

    /// Sekunden seit der letzten Aktivität.
    pub fn idle_seconds(&self) -> i64 {
        (now_unix_ms() - self.last_activity_ms.load(Ordering::SeqCst)).max(0) / 1000
    }

    /// Die aktive Unterhaltung.
    pub fn active_conversation_id(&self) -> Option<String> {
        self.active_conversation
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Setzt die aktive Unterhaltung (ohne Ereignis; Befehle melden sie selbst).
    pub fn set_active_conversation_id(&self, id: Option<String>) {
        if let Ok(mut guard) = self.active_conversation.lock() {
            *guard = id;
        }
    }

    /// Aktueller Fenstermodus.
    pub fn window_mode(&self) -> WindowMode {
        self.window_mode
            .lock()
            .map(|guard| *guard)
            .unwrap_or(WindowMode::Main)
    }

    /// Setzt den Fenstermodus.
    pub fn set_window_mode(&self, mode: WindowMode) {
        if let Ok(mut guard) = self.window_mode.lock() {
            *guard = mode;
        }
    }
}

impl Default for FlowRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Stoppt Workflows und Agent-Flow-Läufe samt ihren Prozessen kontrolliert.
///
/// Die Läufe sehen ihr Abbruch-Flag und beenden sich selbst; Ergebnisse und
/// Kandidaten bleiben erhalten. Es wird nichts automatisch fortgesetzt.
pub fn stop_flow_work(state: &AppState) -> usize {
    // Läufe zuerst: sie sehen ihr Flag auch zwischen zwei Modellaufrufen (etwa
    // während eines Exa-Aufrufs), wo es keinen Queue-Eintrag zum Abbrechen gibt.
    state.flow.workflows.cancel_all();
    state.flow.agent_flow.cancel_all();
    state.flow.jobs.cancel_kinds(&FLOW_KINDS)
}

/// Bricht wirklich alle Jobs ab (vollständiges Beenden).
pub fn stop_all_work(state: &AppState) {
    state.stream_cancel.store(true, Ordering::SeqCst);
    state.voice.shutdown();
    state.mail.stop();
    let ids: Vec<String> = state
        .flow
        .jobs
        .snapshot()
        .into_iter()
        .map(|job| job.id)
        .collect();
    for id in ids {
        state.flow.jobs.cancel(&id);
    }
}

/// Wird nach dem Entsperren aufgerufen: Konnektor-Einstellungen laden und
/// verwaiste Läufe eines abgestürzten Vorgängers als abgebrochen kennzeichnen.
pub fn on_session_opened(state: &AppState) -> AppResult<()> {
    // Jede Sitzung beginnt mit gesperrtem Netz, egal was zuvor eingestellt war.
    crate::net::set_air_gap(true);
    // Die Gmail-Auto-Antwort startet nach jedem Entsperren aus.
    state.mail.stop();
    let session = require_session(state)?;
    let mut vault = session.vault_runtime.lock()?;
    let stored = vault.repository().setting(SETTING_CONNECTOR_CONFIG)?;
    if let Some(text) = stored {
        if let Ok(mut config) = serde_json::from_str::<ConnectorConfig>(&text) {
            // Air Gap startet immer EIN.
            config.offline_mode = true;
            *lock(&state.connector_config)? = config;
        }
    }
    pa_vault::flow_store::mark_orphaned_runs_cancelled(
        vault.repository_mut().connection_mut(),
        now_unix_ms(),
    )?;
    drop(vault);
    crate::agent_flow::recover_interrupted(state)?;
    Ok(())
}

/// Speichert die Konnektor-Einstellungen im Tresor (ohne den Air-Gap-Zustand).
pub fn persist_connector_config(state: &AppState, config: &ConnectorConfig) -> AppResult<()> {
    let session = require_session(state)?;
    let mut to_store = config.clone();
    to_store.offline_mode = true;
    let text = serde_json::to_string(&to_store).map_err(|e| AppError::Internal(e.to_string()))?;
    let mut vault = session.vault_runtime.lock()?;
    vault
        .repository_mut()
        .set_setting(SETTING_CONNECTOR_CONFIG, &text)?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

/// Blendet alle sichtbaren IAP-Fenster aus (für eine Bildschirmaufnahme,
/// damit IAP sich nicht selbst aufnimmt) und liefert ihre Namen zurück.
pub fn hide_own_windows(app: &AppHandle) -> Vec<String> {
    use tauri::Manager;
    let mut hidden = Vec::new();
    for (label, window) in app.webview_windows() {
        if window.is_visible().unwrap_or(false) && window.hide().is_ok() {
            hidden.push(label);
        }
    }
    if !hidden.is_empty() {
        // Dem Fenstersystem einen Moment geben, die Fenster wirklich zu entfernen.
        std::thread::sleep(std::time::Duration::from_millis(350));
    }
    hidden
}

/// Blendet zuvor ausgeblendete Fenster wieder ein.
pub fn restore_windows(app: &AppHandle, labels: &[String]) {
    use tauri::Manager;
    for label in labels {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.show();
        }
    }
}

/// Schreibt eine Policy-Entscheidung ins Audit-Protokoll des Tresors.
///
/// Das Protokoll enthält nur Aktion, Ergebnis und Grund – nie Inhalte (kein
/// Audio, kein Bild, keine Suchbegriffe, keine Schlüssel).
pub fn audit_decision(
    state: &AppState,
    action: pa_policy::CapabilityAction,
    target: Option<String>,
    decision: &pa_policy::Decision,
    reason: &str,
) {
    use pa_launcher::vault_audit::VaultAuditSink;
    use pa_policy::{AuditLog, AuditOutcome, AuditSink, Decision, Mode};

    let Ok(session) = require_session(state) else {
        return;
    };
    let Ok(shared) = session.vault_runtime.shared() else {
        return;
    };
    let (outcome, why) = match decision {
        Decision::Allow(_) => (AuditOutcome::Allow, reason.to_owned()),
        Decision::Prompt(text) => (AuditOutcome::Prompt, text.clone()),
        Decision::Deny(text) => (AuditOutcome::Deny, text.clone()),
    };
    let mut sink = VaultAuditSink::new(shared);
    let _ = sink.append(
        AuditLog {
            mode: Mode::M1Workspace,
            action,
            target,
            outcome,
            reason: why,
        },
        now_unix_ms(),
    );
}

/// Übersicht aller Jobs.
#[tauri::command]
pub fn list_jobs(state: State<'_, AppState>) -> Vec<JobInfo> {
    state.flow.jobs.snapshot()
}

/// Bricht einen Job ab. `false`: nicht vorhanden oder nicht abbrechbar.
#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, job_id: String) -> bool {
    state.flow.jobs.cancel(&job_id)
}

/// Die aktive Unterhaltung, die Hauptseite, Chat und Pet teilen.
#[tauri::command]
pub fn get_active_conversation(state: State<'_, AppState>) -> AppResult<Option<String>> {
    Ok(lock(&state.flow.active_conversation)?.clone())
}

/// Setzt die aktive Unterhaltung und meldet sie allen Fenstern.
#[tauri::command]
pub fn set_active_conversation(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: Option<String>,
) -> AppResult<()> {
    *lock(&state.flow.active_conversation)? = conversation_id.clone();
    let _ = app.emit("active-conversation", conversation_id);
    Ok(())
}

/// Aktueller Fenstermodus.
#[tauri::command]
pub fn get_window_mode(state: State<'_, AppState>) -> WindowMode {
    state.flow.window_mode()
}

/// Wechselt bewusst zum kleinen Pet (Hauptfenster wird ausgeblendet).
///
/// Wird auch verwendet, wenn der Nutzer im ersten Hinweis „Als Pet weiterlaufen“ wählt.
#[tauri::command]
pub fn enter_pet_mode(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    state.close_prompted.store(false, Ordering::SeqCst);
    crate::pet::enter_pet_mode(&app).map_err(|error| AppError::Internal(error.to_string()))
}

/// Zeigt das Hauptfenster und blendet das Pet aus („IAP öffnen“).
#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    crate::pet::show_main(&app);
}

/// „IAP vollständig beenden“ aus dem Pet oder dem Tray-Menü.
#[tauri::command]
pub fn request_quit(app: AppHandle) {
    request_full_quit(&app);
}

/// Ob beim Schließen erst der Hinweis „Schließen ≠ Beenden“ erscheint.
///
/// Der Hinweis erscheint genau einmal (Tresor-Schlüssel `ui.pet_hint_shown`).
pub fn pet_hint_needed(state: &AppState) -> bool {
    let Ok(session) = require_session(state) else {
        return false;
    };
    let Ok(key) = crate::ui_state::storage_key("ui.pet_hint_shown") else {
        return false;
    };
    let Ok(vault) = session.vault_runtime.lock() else {
        return false;
    };
    vault.repository().setting(&key).ok().flatten().is_none()
}

/// Beendet IAP vollständig. Mit offener Nachfrage („Spuren entfernen?“) zeigt
/// das Hauptfenster erst den Dialog; sonst wird sofort geordnet beendet.
pub fn request_full_quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let has_session = state
        .session
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false);
    let ask = has_session && crate::ask_on_exit(&state).unwrap_or(true);
    if ask {
        crate::pet::show_main(app);
        let _ = app.emit("iap-close-requested", "quit");
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let _ = crate::shutdown_for_exit(&state, false);
        app.exit(0);
    });
}

/// Entlädt im Pet-Leerlauf das Sprachmodell, damit der Arbeitsspeicher frei wird.
///
/// Die nächste Anfrage startet es wieder (der Chat meldet das als „Modell neu
/// gestartet“). Sprach- und Bildprozesse laufen ohnehin nur für die Dauer ihrer
/// Aufgabe. Ein gesperrter Tresor hat kein Modell, also nichts zu entladen.
pub fn start_idle_unloader(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        let state = app.state::<AppState>();
        if state.flow.window_mode() != WindowMode::Pet
            || state.flow.idle_seconds() < IDLE_UNLOAD_SECONDS
            || state.flow.engine_unloaded.load(Ordering::SeqCst)
        {
            continue;
        }
        let busy = state.flow.jobs.snapshot().iter().any(|job| {
            matches!(
                job.status,
                pa_types::avatar::JobStatus::Waiting | pa_types::avatar::JobStatus::Running
            )
        });
        if busy || state.voice.state() != pa_types::voice::VoiceState::Idle {
            continue;
        }
        let Ok(session) = require_session(&state) else {
            continue;
        };
        let stopped = match session.engine.lock() {
            Ok(mut engine) => engine.stop().is_ok(),
            Err(_) => false,
        };
        if stopped {
            state.flow.engine_unloaded.store(true, Ordering::SeqCst);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_runtime_starts_in_main_window_with_one_queue_slot() {
        let runtime = FlowRuntime::new();
        assert_eq!(runtime.window_mode(), WindowMode::Main);
        assert!(runtime.jobs.snapshot().is_empty());
    }

    #[test]
    fn window_mode_can_switch_and_back() {
        let runtime = FlowRuntime::new();
        runtime.set_window_mode(WindowMode::Pet);
        assert_eq!(runtime.window_mode(), WindowMode::Pet);
        runtime.set_window_mode(WindowMode::Main);
        assert_eq!(runtime.window_mode(), WindowMode::Main);
    }
}
