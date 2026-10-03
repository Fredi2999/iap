//! Tauri-Backend der Desktop-Anwendung.
//!
//! Bündelt Bootstrap, Vault-Entsperrung, Modell-Start, Konversationsverwaltung
//! und Streaming hinter einer minimalen Menge typisierter IPC-Kommandos. Der
//! Frontendcode kennt Rust ausschließlich über die in `pa-types::ipc`
//! definierten Verträge.

mod agenda_text;
mod agent_flow;
mod audio;
mod calendar;
mod calendar_cmds;
mod code_agent;
mod code_cmds;
mod code_commands;
mod code_roots;
mod command_runner;
mod connectors;
mod documents;
mod error_code;
mod exa;
mod folder_dialog;
mod git_cmds;
mod host_traces;
mod instruction_skills;
mod library_cmds;
mod lifecycle;
mod mail;
mod mail_cmds;
mod mail_tool;
mod memory_cmds;
mod net;
mod pack_cmds;
mod packs;
mod pci_cmds;
mod perf;
mod pet;
mod pet_hotkey;
mod pet_style;
mod screen;
mod screen_cmds;
pub mod session;
#[cfg(test)]
mod skill_import_tests;
mod skill_tools;
mod tray;
mod ui_language;
mod ui_state;
mod vault_admin;
mod voice;
mod volume;
mod workflow_cmds;

use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pa_core::{
    budget::BudgetPolicy,
    conversation::Conversations,
    engine::{ChatEngine, EngineReady},
    orchestrator::{ChatOrchestrator, TurnCallbacks, TurnError, TurnRequest},
    prompt::PromptPrefix,
    CoreError,
};
use pa_inference::{
    adapter::AdapterKind, config::ServerConfig, loopback::LoopbackEndpoint, supervisor::Supervisor,
};
use pa_launcher::{
    bootstrap::{prepare, BootstrapOptions},
    cache::{CopyObserver, ModelCache},
    inference_backend::LlamaServerEngine,
    model_config::load_model_descriptor,
    paths::PackageRoot,
    resources::{compute_resource_plan, PlanRequest},
    runtime::{SharedConversations, VaultRuntime},
    tool_runtime::ToolRuntime,
    vault_audit::VaultAuditSink,
    LauncherError,
};
use pa_memory::{embedder::HashingEmbedder, IngestFact, MemoryStore};
use pa_types::{
    ipc::{
        AuditEntryView, AuditFilter, AvailableModel, BootstrapStatus, ConnectorConfig,
        ConversationDetail, MemoryExport, MemoryRetrieveResponse, MemoryUpsertRequest,
        ModelSettings, SendMessageRequest, SettingsSnapshot, SettingsUpdate, StreamEvent,
        ThemePreference, TierOverrideChange, UserProfile, VaultInfo, VaultSwitchRequest,
        WorkspaceEntry, WorkspaceListing,
    },
    memory::Fact,
    model::{HardwareTier, KvQuantization, ModelDescriptor, ResourcePlan},
};
use pa_vault::{
    hot_copy::{HotVault, RecoveryMode},
    key::derive_key,
    meta::{Argon2Parameters, VaultMeta},
    VaultError,
};
use rand_core::{OsRng, RngCore};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};
use zeroize::Zeroizing;

use crate::session::{Session, StoredBootstrap};

const READY_TIMEOUT: Duration = Duration::from_secs(120);
const SETTING_THEME: &str = "ui.theme";
const SETTING_USER_PROFILE_ENABLED: &str = "profile.enabled";
const SETTING_USER_NAME: &str = "profile.user_name";
const SETTING_USER_ABOUT: &str = "profile.user_about";
const SETTING_CUSTOM_SYSTEM_PROMPT: &str = "profile.custom_system_prompt";
const SETTING_ACTIVE_MODEL: &str = "model.active_id";
const SETTING_MODEL_PROFILE_PREFIX: &str = "model.profile.";

/// Vom Frontend gelesenes Wurzelverzeichnis; zusätzlich per Umgebungsvariable
/// `PORTABLE_AI_ROOT` überschreibbar, damit Entwicklungslauf und portabler
/// Start dieselbe Binary verwenden können.
fn default_root() -> PathBuf {
    if let Ok(value) = std::env::var("PORTABLE_AI_ROOT") {
        return PathBuf::from(value);
    }
    match std::env::current_exe() {
        Ok(path) => path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
        Err(_) => PathBuf::from("."),
    }
}

/// Startet die Tauri-App mit registrierten IPC-Kommandos und minimalen Rechten.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        // Globales Kürzel für das Pet: bleibt ungenutzt, bis `set_pet_hotkey` es registriert.
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let app_state = AppState::new(default_root());
            app.manage(app_state);
            app.state::<AppState>().flow.attach_events(app.handle());
            // Sichtbares Symbol im Infobereich und Entladen im Pet-Leerlauf.
            if let Err(error) = tray::install(app.handle()) {
                eprintln!("Tray-Symbol nicht verfügbar: {error}");
            }
            lifecycle::start_idle_unloader(app.handle());
            // Nur Debug-Builds: `IAP_DEBUG_PET=1` zeigt das Pet sofort und meldet seine
            // Geometrie, damit Platzierung und Transparenz ohne Tresor geprüft werden können.
            #[cfg(debug_assertions)]
            if std::env::var_os("IAP_DEBUG_PET").is_some() {
                let handle = app.handle().clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(3));
                    match pet::show(&handle) {
                        Ok(()) => {
                            thread::sleep(Duration::from_secs(2));
                            eprintln!("{}", pet::debug_report(&handle));
                        }
                        Err(error) => eprintln!("Pet-Fenster: {error}"),
                    }
                });
            }

            // Browserdaten der Oberfläche (Cache, Speicher von WebView2) liegen auf
            // dem Stick statt im Nutzerprofil des Hosts (Invariante 6). Deshalb wird
            // das Fenster hier erzeugt; in tauri.conf.json steht `create: false`.
            let webview_dir = default_root().join("AI").join("data").join("webview");
            if let Some(config) = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
            {
                tauri::WebviewWindowBuilder::from_config(app.handle(), config)?
                    .data_directory(webview_dir)
                    .build()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap_status,
            list_vaults,
            unlock_vault,
            get_user_profile,
            update_user_profile,
            list_conversations,
            open_conversation,
            create_conversation,
            rename_conversation,
            delete_conversation,
            installed_models,
            select_model,
            get_model_settings,
            save_model_settings,
            settings_snapshot,
            apply_settings,
            switch_vault,
            send_message,
            send_message_with_tools,
            cancel_stream,
            retrieve_memory,
            list_active_facts,
            upsert_fact,
            forget_fact,
            export_memory,
            list_audit,
            list_workspace,
            read_workspace_file,
            rubric_default,
            save_questionnaire,
            load_questionnaire,
            evaluate_rubric,
            run_rubric_monte_carlo,
            code_cmds::read_code_file,
            code_cmds::write_code_file,
            code_cmds::diff_code_file,
            code_cmds::apply_hunks,
            code_cmds::code_list,
            vault_admin::change_vault_password,
            vault_admin::delete_vault,
            code_agent::code_agent_send,
            code_agent::code_agent_cancel,
            code_agent::code_agent_reset,
            code_agent::code_agent_changes,
            code_agent::code_agent_change,
            code_agent::code_agent_discard,
            code_commands::code_agent_command_respond,
            code_roots::code_roots_status,
            code_roots::code_root_check,
            code_roots::code_root_open_host,
            code_roots::code_root_use_stick,
            code_roots::code_root_revoke,
            code_cmds::code_create,
            code_cmds::code_rename,
            code_cmds::code_delete,
            code_cmds::code_search,
            code_cmds::code_assist,
            code_cmds::code_assist_cancel,
            snapshot_create,
            snapshot_list,
            snapshot_restore,
            snapshot_discard,
            git_cmds::git_info,
            git_cmds::git_status,
            git_cmds::git_log,
            git_cmds::git_commit,
            respond_permission,
            pending_permissions,
            schedule_plan,
            import_ics_text,
            export_ics_text,
            list_installed_skills,
            import_skill_from_path,
            skill_dry_run,
            run_skill,
            verify_update_bundle,
            list_backups,
            create_full_export,
            get_connector_config,
            update_connector_config,
            workflow_cmds::test_exa_search,
            workflow_cmds::test_connector,
            workflow_cmds::connector_overview,
            memory_cmds::memory_graph,
            memory_cmds::update_fact,
            memory_cmds::learn_from_conversation,
            memory_cmds::learn_cancel,
            pet_style::get_pet_style,
            pet_style::set_pet_style,
            pet_style::system_stats,
            calendar_cmds::calendar_overview,
            calendar_cmds::calendar_events,
            calendar_cmds::calendar_icloud_discover,
            calendar_cmds::calendar_add_icloud,
            calendar_cmds::calendar_add_google,
            calendar_cmds::calendar_remove_source,
            calendar_cmds::calendar_sync,
            calendar_cmds::calendar_create_event,
            mail_cmds::mail_status,
            mail_cmds::mail_set_password,
            mail_cmds::mail_clear_password,
            mail_cmds::mail_test_connection,
            mail_cmds::mail_run_now,
            mail_cmds::mail_set_polling,
            mail_cmds::mail_emergency_stop,
            mail_cmds::mail_log,
            mail_cmds::mail_clear_log,
            pci_cmds::pci_status,
            pci_cmds::pci_import,
            pci_cmds::pci_list_computers,
            pci_cmds::pci_activity,
            pci_cmds::pci_delete_host,
            pci_cmds::pci_install_and_start,
            pci_cmds::pci_start,
            pci_cmds::pci_stop,
            pci_cmds::pci_remove_collector,
            workflow_cmds::exa_status,
            workflow_cmds::workflow_list,
            workflow_cmds::workflow_save,
            workflow_cmds::workflow_delete,
            workflow_cmds::workflow_validate,
            workflow_cmds::workflow_start_run,
            workflow_cmds::workflow_active_run,
            workflow_cmds::workflow_cancel_run,
            workflow_cmds::workflow_run_report,
            workflow_cmds::workflow_runs,
            load_ui_state,
            save_ui_state,
            get_ui_language,
            set_ui_language,
            host_traces,
            set_ask_on_exit,
            quit_app,
            cancel_quit,
            library_cmds::list_projects,
            library_cmds::save_project,
            library_cmds::delete_project,
            library_cmds::assign_conversation,
            library_cmds::import_document,
            library_cmds::list_documents,
            library_cmds::delete_document,
            library_cmds::set_document_attached,
            library_cmds::conversation_documents,
            library_cmds::conversation_sources,
            performance_report,
            run_performance_check,
            compare_answer,
            lifecycle::list_jobs,
            lifecycle::cancel_job,
            lifecycle::get_active_conversation,
            lifecycle::set_active_conversation,
            lifecycle::get_window_mode,
            lifecycle::enter_pet_mode,
            lifecycle::show_main_window,
            lifecycle::request_quit,
            pet::pet_expand,
            pet_hotkey::set_pet_hotkey,
            pack_cmds::list_packs,
            pack_cmds::set_pack_enabled,
            pack_cmds::verify_pack,
            voice::voice_status,
            voice::voice_block_reason,
            voice::voice_start_capture,
            voice::voice_stop_capture,
            voice::voice_discard,
            voice::voice_speak,
            voice::voice_stop_output,
            voice::voice_set_muted,
            voice::voice_run_benchmark,
            folder_dialog::pick_folder_dialog,
            instruction_skills::set_instruction_skill_active,
            instruction_skills::uninstall_skill,
            agent_flow::agent_flow_limits,
            agent_flow::agent_flow_preflight,
            agent_flow::agent_flow_projects,
            agent_flow::agent_flow_revoke_project,
            agent_flow::agent_flow_start,
            agent_flow::agent_flow_list,
            agent_flow::agent_flow_diff,
            agent_flow::agent_flow_cancel,
            agent_flow::agent_flow_adopt_preview,
            agent_flow::agent_flow_adopt,
            agent_flow::agent_flow_cleanup,
            screen_cmds::screen_status,
            screen_cmds::screen_sources,
            screen_cmds::screen_look,
            screen_cmds::screen_pick_region,
        ])
        .on_window_event(|window, event| {
            // Minimiert der Nutzer das Hauptfenster, bleibt IAP als Pet sichtbar und
            // ansprechbar; beim Wiederherstellen verschwindet es wieder.
            if window.label() == "main"
                && matches!(
                    event,
                    tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Focused(_)
                )
            {
                pet::sync_with_main_window(window.app_handle());
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Nur das Hauptfenster zählt; das Pet wird über sein Menü beendet.
                if window.label() != "main" {
                    return;
                }
                let state: State<'_, AppState> = window.state::<AppState>();
                if state.quitting.load(Ordering::SeqCst) {
                    return;
                }
                let has_session = state
                    .session
                    .lock()
                    .map(|guard| guard.is_some())
                    .unwrap_or(false);
                if !has_session {
                    // Kein offener Tresor: nichts zu sichern, das Modell läuft noch nicht.
                    return;
                }
                api.prevent_close();
                // Beim ersten Schließen erklärt ein Hinweis, dass das Fenster zum Pet wird
                // und IAP weiterläuft; erst „vollständig beenden“ beendet alles.
                if lifecycle::pet_hint_needed(&state)
                    && !state.close_prompted.swap(true, Ordering::SeqCst)
                    && window.emit("iap-close-requested", "choice").is_ok()
                {
                    return;
                }
                state.close_prompted.store(false, Ordering::SeqCst);
                let app = window.app_handle().clone();
                if let Err(error) = pet::enter_pet_mode(&app) {
                    // Ohne Pet-Fenster gäbe es keinen sichtbaren Rest von IAP: dann geordnet beenden.
                    eprintln!("Pet-Fenster nicht möglich, IAP wird beendet: {error}");
                    thread::spawn(move || {
                        let state = app.state::<AppState>();
                        let _ = shutdown_for_exit(&state, false);
                        app.exit(0);
                    });
                }
            }
        })
        .build(tauri::generate_context!())?
        .run(|app, event| {
            // Letzte Sicherung für jeden anderen Weg aus der App (z. B. Abmelden
            // von Windows): Modellprozess stoppen und den Tresor zurückschreiben.
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                let _ = shutdown_for_exit(&state, false);
            }
        });
    Ok(())
}

/// Zusammenfassung aller wechselnden Handles, die die App zur Laufzeit besitzt.
pub struct AppState {
    package_root: Mutex<PathBuf>,
    bootstrap: Mutex<Option<Arc<StoredBootstrap>>>,
    session: Mutex<Option<Session>>,
    stream_cancel: Arc<AtomicBool>,
    /// Ausstehende Werkzeug-Freigabedialoge (Konzept 10.3, Meilenstein 8).
    ///
    /// Der Tool-Runner registriert einen [`PendingPermission`], wenn die
    /// Policy `Decision::Prompt` liefert (z. B. bei `UntrustedContent`), und
    /// wartet auf die Antwort des Frontends via `respond_permission`. Aktuell
    /// wird der Store bereits per IPC gefüllt/gelesen; die vollständige
    /// Rendezvous-Verdrahtung mit dem `pa_launcher::tool_runtime` folgt in
    /// einem separaten Umbau.
    pending_permissions: Arc<Mutex<std::collections::HashMap<String, PendingPermission>>>,
    connector_config: Arc<Mutex<ConnectorConfig>>,
    /// Gesetzt, sobald das geordnete Beenden läuft; dann darf das Fenster schließen.
    quitting: AtomicBool,
    /// Die Nachfrage „Spuren entfernen?“ ist offen. Ein zweiter Klick auf
    /// Schließen beendet dann ohne weitere Nachfrage, damit nie ein Fenster
    /// hängen bleibt, falls die Oberfläche nicht antwortet.
    close_prompted: AtomicBool,
    /// Wie lange der letzte Start des Modells gedauert hat (für den Leistungs-Check).
    model_start_ms: Mutex<Option<u64>>,
    /// Aktive Unterhaltung, Job-Queue und Fenstermodus für Flow, Sprache und Pet.
    flow: lifecycle::FlowRuntime,
    /// Sprachsitzung: Aufnahme, Erkennung, Vorlesen.
    voice: voice::VoiceRuntime,
    /// Takt und Schalter der Gmail-Auto-Antwort (startet nach jedem Entsperren aus).
    mail: mail::poller::MailRuntime,
    /// Arbeitsordner des Code-Bereichs: Stick (Standard) oder freigegebener Ordner auf dem PC.
    code_root: code_roots::CodeRootRuntime,
    /// Code-Agent: Verlauf und offene Änderungsvorschläge der Sitzung.
    code_agent: code_agent::CodeAgentRuntime,
}

/// Ein wartender Freigabedialog. Antwort erreicht den Runner über den
/// [`std::sync::mpsc::Sender`].
pub struct PendingPermission {
    pub tool: String,
    pub arguments_json: String,
    pub reason: String,
    pub created_unix_ms: i64,
    pub responder: std::sync::mpsc::Sender<PermissionOutcome>,
}

/// Nutzerantwort auf einen Freigabedialog.
#[derive(Debug, Clone, Copy)]
pub struct PermissionOutcome {
    pub allow: bool,
    pub remember_for_session: bool,
}

impl AppState {
    fn new(package_root: PathBuf) -> Self {
        Self {
            package_root: Mutex::new(package_root),
            bootstrap: Mutex::new(None),
            session: Mutex::new(None),
            stream_cancel: Arc::new(AtomicBool::new(false)),
            pending_permissions: Arc::new(Mutex::new(std::collections::HashMap::new())),
            connector_config: Arc::new(Mutex::new(ConnectorConfig::default())),
            quitting: AtomicBool::new(false),
            close_prompted: AtomicBool::new(false),
            model_start_ms: Mutex::new(None),
            flow: lifecycle::FlowRuntime::new(),
            voice: voice::VoiceRuntime::new(),
            mail: mail::poller::MailRuntime::default(),
            code_root: code_roots::CodeRootRuntime::new(),
            code_agent: code_agent::CodeAgentRuntime::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("Vault-Fehler: {0}")]
    Vault(String),
    #[error("Core-Fehler: {0}")]
    Core(String),
    #[error("Launcher-Fehler: {0}")]
    Launcher(String),
    #[error("Ungültige Eingabe: {0}")]
    Invalid(String),
    #[error("Internal: {0}")]
    Internal(String),
    /// Fehler mit stabilem Code (siehe `error_code`): Die Oberfläche übersetzt nach dem Code und
    /// muss den deutschen Text nicht mit Mustern erraten. `message` bleibt für Protokolle und
    /// für Stellen, die den Code noch nicht kennen.
    #[error("{message}")]
    Coded { code: &'static str, message: String },
}

impl AppError {
    /// Der Tresor ist nicht (mehr) entsperrt: gleicher Fehler an allen Stellen, die eine Sitzung brauchen.
    fn locked() -> Self {
        Self::Coded {
            code: error_code::VAULT_LOCKED,
            message: "Der Tresor ist gesperrt.".to_owned(),
        }
    }
}

/// Serialisiert `AppError` für die Oberfläche. Die meisten Fehler gehen weiter als reine
/// Zeichenkette (die UI zeigt den Text mit `String(error)`); Fehler mit Code gehen als Objekt
/// `{ code, message }`, das `errors.ts` (`friendlyError`, `errorText`) auspackt.
/// Mit `#[serde(tag = …)]` scheitert das an Newtype-Varianten (Serde-Fehler „cannot serialize
/// tagged newtype variant"), deshalb ist die Implementierung handgeschrieben.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Coded { code, message } => {
                use serde::ser::SerializeStruct;
                let mut object = serializer.serialize_struct("AppError", 2)?;
                object.serialize_field("code", code)?;
                object.serialize_field("message", message)?;
                object.end()
            }
            other => serializer.serialize_str(&other.to_string()),
        }
    }
}

impl From<VaultError> for AppError {
    fn from(error: VaultError) -> Self {
        let code = match &error {
            VaultError::Authentication => Some(error_code::WRONG_PASSPHRASE),
            VaultError::MediaUnavailable => Some(error_code::VAULT_MEDIA_UNAVAILABLE),
            VaultError::Integrity(_) => Some(error_code::VAULT_INTEGRITY),
            _ => None,
        };
        match code {
            Some(code) => Self::Coded {
                code,
                message: format!("Vault-Fehler: {error}"),
            },
            None => Self::Vault(error.to_string()),
        }
    }
}

impl From<CoreError> for AppError {
    fn from(error: CoreError) -> Self {
        Self::Core(error.to_string())
    }
}

impl From<LauncherError> for AppError {
    fn from(error: LauncherError) -> Self {
        Self::Launcher(error.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

type AppResult<T> = Result<T, AppError>;

fn ensure_bootstrap(state: &AppState) -> AppResult<Arc<StoredBootstrap>> {
    {
        let guard = lock(&state.bootstrap)?;
        if let Some(existing) = guard.as_ref() {
            return Ok(Arc::clone(existing));
        }
    }
    let package_root = lock(&state.package_root)?.clone();
    let mut progress = NoopProgress;
    let context = prepare(&package_root, BootstrapOptions::default(), &mut progress)?;
    let stored = Arc::new(StoredBootstrap::from_context(context));
    let mut guard = lock(&state.bootstrap)?;
    *guard = Some(Arc::clone(&stored));
    Ok(stored)
}

/// Führt Manifestprüfung, Hardwareprofil und Ressourcenplan aus (oder gibt das
/// gecachte Ergebnis zurück).
#[tauri::command]
fn bootstrap_status(state: State<'_, AppState>) -> AppResult<BootstrapStatus> {
    let stored = ensure_bootstrap(&state)?;
    Ok(stored.to_status())
}

/// Scannt das Datenverzeichnis nach vorhandenen Tresoren (*.db) und meldet sie für die UI.
#[tauri::command]
fn list_vaults(state: State<'_, AppState>) -> AppResult<Vec<VaultInfo>> {
    let bootstrap = ensure_bootstrap(&state)?;
    let data_dir = bootstrap.package_root.join("AI").join("data");
    let default_vault = bootstrap.default_vault_path();
    let mut results = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&data_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "db" {
                        let file_stem = path.file_name().unwrap_or_default().to_string_lossy();
                        if file_stem.contains(".partial") || file_stem.contains(".next") {
                            continue;
                        }
                        let name = file_stem.to_string();
                        let is_default = path == default_vault;
                        let initialized = path.with_extension("meta").exists();
                        let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                        results.push(VaultInfo {
                            name,
                            path: path.to_string_lossy().to_string(),
                            is_default,
                            initialized,
                            size_bytes,
                        });
                    }
                }
            }
        }
    }

    if results.is_empty() {
        let initialized = default_vault.with_extension("meta").exists();
        let size_bytes = std::fs::metadata(&default_vault)
            .map(|m| m.len())
            .unwrap_or(0);
        results.push(VaultInfo {
            name: "vault.db".to_owned(),
            path: default_vault.to_string_lossy().to_string(),
            is_default: true,
            initialized,
            size_bytes,
        });
    } else {
        results.sort_by_key(|item| std::cmp::Reverse(item.is_default));
    }

    Ok(results)
}

/// Öffnet den Vault mit der übermittelten Passphrase und startet den lokalen
/// Inferenzserver. Unterstützt die gezielte Auswahl eines alternativen Tresor-Pfads.
#[tauri::command]
async fn unlock_vault(
    app: AppHandle,
    passphrase: String,
    create_if_missing: bool,
    recover_from_host: bool,
    vault_path: Option<String>,
) -> AppResult<SettingsSnapshot> {
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        unlock_vault_inner(
            &worker_app.state::<AppState>(),
            passphrase,
            create_if_missing,
            recover_from_host,
            vault_path,
        )
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    settings_snapshot(app.state())
}

fn unlock_vault_inner(
    state: &AppState,
    passphrase: String,
    create_if_missing: bool,
    recover_from_host: bool,
    vault_path: Option<String>,
) -> AppResult<()> {
    let passphrase = Zeroizing::new(passphrase);
    let bootstrap = ensure_bootstrap(state)?;
    let portable_vault = match vault_path {
        Some(ref p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
        _ => bootstrap.default_vault_path(),
    };
    let meta_path = portable_vault.with_extension("meta");
    let meta = load_or_create_meta(&portable_vault, &meta_path, create_if_missing)?;
    let key = derive_key(&passphrase, &meta)?;
    let hot_directory = hot_vault_directory(&portable_vault, &bootstrap.host_identifier)?;
    let vault = match HotVault::start(&portable_vault, &hot_directory, key) {
        Ok(vault) => vault,
        Err(VaultError::RecoveryChoiceRequired { .. }) => {
            let retry_key = derive_key(&passphrase, &meta)?;
            let recovery = if recover_from_host {
                RecoveryMode::UseNewestHost
            } else {
                RecoveryMode::UsePortable
            };
            HotVault::start_with_recovery(&portable_vault, &hot_directory, retry_key, recovery)?
        }
        Err(other) => return Err(other.into()),
    };
    open_session(state, &bootstrap, vault, portable_vault)?;
    // Ein Fehler hier (z. B. beim Lesen der Konnektor-Einstellungen) darf das
    // Entsperren nicht verhindern; die Ursache landet im Protokoll.
    if let Err(error) = lifecycle::on_session_opened(state) {
        eprintln!("Sitzungsstart: Zusatzzustand nicht geladen: {error}");
    }
    Ok(())
}

fn open_session(
    state: &AppState,
    bootstrap: &StoredBootstrap,
    vault: HotVault,
    portable_vault: PathBuf,
) -> AppResult<()> {
    let preferred_model = vault.repository().setting(SETTING_ACTIVE_MODEL)?;
    let preferred_id = preferred_model
        .as_deref()
        .unwrap_or(&bootstrap.descriptor_id);
    let mut profile = stored_model_settings(&vault, preferred_id)?;
    let prepared = match preferred_model.as_deref() {
        Some(id) if id != bootstrap.descriptor_id => {
            match prepare_selected_model(
                bootstrap,
                id,
                None,
                profile.as_ref().and_then(|value| value.context_tokens),
            ) {
                Ok(model) => model,
                Err(error) => {
                    eprintln!(
                        "Gespeichertes Modell nicht verfügbar: {error}; verwende Standardmodell"
                    );
                    profile = stored_model_settings(&vault, &bootstrap.descriptor_id)?;
                    prepare_selected_model(
                        bootstrap,
                        &bootstrap.descriptor_id,
                        None,
                        profile.as_ref().and_then(|value| value.context_tokens),
                    )?
                }
            }
        }
        _ => prepare_selected_model(
            bootstrap,
            &bootstrap.descriptor_id,
            None,
            profile.as_ref().and_then(|value| value.context_tokens),
        )?,
    };
    let (config, endpoint) = server_configuration(
        bootstrap.server_executable.clone(),
        prepared.cached_path,
        &prepared.descriptor.id,
        &prepared.plan,
    )?;
    let mut supervisor = Supervisor::new(config.clone());
    let model_started = std::time::Instant::now();
    supervisor.start(READY_TIMEOUT).map_err(|error| {
        AppError::Launcher(format!(
            "Inferenzserver konnte nicht gestartet werden: {error}"
        ))
    })?;
    // Startzeit für den Leistungs-Check (Feature 3).
    *lock(&state.model_start_ms)? =
        Some(u64::try_from(model_started.elapsed().as_millis()).unwrap_or(u64::MAX));
    let adapter = AdapterKind::from_family(&prepared.descriptor.family, &prepared.descriptor.id);
    let mut running_engine = LlamaServerEngine::from_running(
        supervisor,
        endpoint,
        adapter,
        config.clone(),
        READY_TIMEOUT,
    );
    if let Some(profile) = &profile {
        running_engine.set_sampling(Some(pa_launcher::inference_backend::SamplingSettings {
            temperature: profile.temperature,
            top_p: profile.top_p,
        }));
    }
    let engine = Arc::new(Mutex::new(running_engine));
    let vault_runtime = Arc::new(VaultRuntime::start(vault, |_| {}));
    let vault_shared = vault_runtime
        .shared()
        .map_err(|error| AppError::Internal(error.to_string()))?;

    let workspace_dir = bootstrap
        .package_root
        .join("AI")
        .join("data")
        .join("workspace");
    std::fs::create_dir_all(&workspace_dir)?;

    // Werkzeug-Runtime mit Vault-persistentem Audit-Log.
    let sink: Box<dyn pa_policy::AuditSink + Send> =
        Box::new(VaultAuditSink::new(Arc::clone(&vault_shared)));
    let mut tool_runtime = ToolRuntime::with_sink(&workspace_dir, sink)
        .map_err(|error| AppError::Launcher(error.to_string()))?;
    // Installierte Skills stehen dem Modell als Werkzeuge zur Verfügung.
    for tool in skill_tools::installed_skill_tools(&bootstrap.package_root) {
        tool_runtime.register_tool(tool);
    }
    // Postfach lesen (nur lesend). Das Werkzeug prüft Freigabe, Air Gap und Herkunft bei jedem Aufruf selbst.
    {
        let password_vault = Arc::clone(&vault_shared);
        tool_runtime.register_tool(Arc::new(mail_tool::MailTool::new(
            Arc::clone(&state.connector_config),
            Arc::new(move || {
                password_vault
                    .lock()
                    .ok()
                    .and_then(|vault| vault.repository().setting("mail.password").ok().flatten())
            }),
            Arc::new(mail::tls::SystemMailTransport),
        )));
    }
    let tool_runtime = Arc::new(Mutex::new(tool_runtime));

    // Memory-System (Konzept 6, Meilenstein 10).
    //
    // Sobald das Manifest `embedding_model_id` gesetzt hat, wäre hier ein
    // zweiter `llama-server` mit `--embedding` zu starten und der
    // `pa_launcher::embedder_bridge::LoopbackEmbedder` einzuhängen. Solange
    // das Modell noch nicht im Bundle ist, bleibt der `HashingEmbedder`
    // aktiv — die Suche funktioniert damit als Prototyp weiter, aber ohne
    // semantische Nähe. Wir loggen die Konfiguration transparent (Warnung
    // in `plan.warnings`), damit die Startanzeige das ehrlich sichtbar
    // macht.
    if let Some(embedding_id) = bootstrap.manifest_summary.embedding_model_id.as_deref() {
        eprintln!(
            "manifest fordert Embedding-Modell `{embedding_id}`, \
             der zweite llama-server ist aber noch nicht verdrahtet; \
             HashingEmbedder-Fallback bleibt aktiv."
        );
    }
    let memory = Arc::new(Mutex::new(MemoryStore::new(
        Arc::clone(&vault_shared),
        HashingEmbedder,
    )));

    let mut session = Session::new(
        engine,
        prepared.descriptor.id,
        vault_runtime,
        portable_vault,
        prepared.plan.context_tokens,
        prepared.plan.kv_quantization,
        tool_runtime,
        workspace_dir,
        memory,
    );
    session.context_override = profile.and_then(|value| value.context_tokens);
    let mut guard = lock(&state.session)?;
    *guard = Some(session);
    // Eine neue Sitzung beginnt immer im Arbeitsordner des Sticks.
    state.code_root.reset();
    state.code_agent.reset();
    Ok(())
}

/// Listet alle Konversationen mit Titel und letzter Aktualisierung.
#[tauri::command]
fn list_conversations(state: State<'_, AppState>) -> AppResult<Vec<pa_types::chat::Conversation>> {
    let session = require_session(&state)?;
    let conversations = SharedConversations::new(session.vault_runtime.shared()?);
    Ok(conversations.list()?)
}

/// Liefert eine Konversation samt aller persistierten Nachrichten.
#[tauri::command]
fn open_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
) -> AppResult<ConversationDetail> {
    let session = require_session(&state)?;
    let shared = session.vault_runtime.shared()?;
    let conversations = SharedConversations::new(shared);
    let list = conversations.list()?;
    let conversation = list
        .into_iter()
        .find(|c| c.id == conversation_id)
        .ok_or_else(|| AppError::Invalid("Konversation nicht gefunden".to_owned()))?;
    let messages = conversations.messages(&conversation_id)?;
    Ok(ConversationDetail {
        conversation,
        messages,
    })
}

/// Legt eine leere Konversation an und liefert ihre neue ID zurück.
#[tauri::command]
fn create_conversation(state: State<'_, AppState>, title: String) -> AppResult<String> {
    let session = require_session(&state)?;
    let id = random_id("conversation");
    let now = now_unix_ms();
    let mut conversations = SharedConversations::new(session.vault_runtime.shared()?);
    conversations.create(&id, &title, now)?;
    if let Ok(mut vault) = session.vault_runtime.lock() {
        let mut no_fault = pa_vault::hot_copy::NoFault;
        let _ = vault.sync(&mut no_fault);
    }
    Ok(id)
}

/// Ändert den Titel einer Konversation.
#[tauri::command]
fn rename_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
    title: String,
) -> AppResult<()> {
    let session = require_session(&state)?;
    let mut conversations = SharedConversations::new(session.vault_runtime.shared()?);
    conversations.rename(&conversation_id, &title, now_unix_ms())?;
    if let Ok(mut vault) = session.vault_runtime.lock() {
        let mut no_fault = pa_vault::hot_copy::NoFault;
        let _ = vault.sync(&mut no_fault);
    }
    Ok(())
}

/// Löscht eine Konversation inklusive aller Nachrichten (Fremdschlüssel-Cascade).
#[tauri::command]
fn delete_conversation(state: State<'_, AppState>, conversation_id: String) -> AppResult<()> {
    let session = require_session(&state)?;
    let mut conversations = SharedConversations::new(session.vault_runtime.shared()?);
    conversations.delete(&conversation_id)?;
    if let Ok(mut vault) = session.vault_runtime.lock() {
        let mut no_fault = pa_vault::hot_copy::NoFault;
        let _ = vault.sync(&mut no_fault);
    }
    Ok(())
}

/// Liefert alle tatsächlich installierten Modelle.
///
/// Scannt `AI/models/*.model.toml` und bietet nur validierte Deskriptoren mit
/// vorhandener GGUF an, damit die Auswahl keine unstartbaren Modelle zeigt.
#[tauri::command]
fn installed_models(state: State<'_, AppState>) -> AppResult<Vec<AvailableModel>> {
    installed_models_list(&state)
}

fn installed_models_list(state: &AppState) -> AppResult<Vec<AvailableModel>> {
    let bootstrap = ensure_bootstrap(state)?;
    let package = PackageRoot::new(&bootstrap.package_root)?;
    let default_id = bootstrap.descriptor_id.clone();
    let models_dir = bootstrap.package_root.join("AI").join("models");
    let mut models: Vec<AvailableModel> = Vec::new();
    // Standardmodell aus dem Bootstrap kommt immer zuerst und mit `is_default=true`.
    models.push(bootstrap.to_available_model());
    if let Ok(entries) = std::fs::read_dir(&models_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext == "toml") {
                continue;
            }
            if !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".model.toml"))
            {
                continue;
            }
            let relative = Path::new("AI/models").join(entry.file_name());
            let Ok(descriptor) = load_model_descriptor(&package, &relative) else {
                continue;
            };
            let gguf = models_dir.join(&descriptor.gguf_file);
            if !gguf.exists() {
                continue;
            }
            if descriptor.id == default_id {
                continue;
            }
            models.push(AvailableModel {
                id: descriptor.id,
                display_name: descriptor.display_name,
                family: descriptor.family,
                gguf_bytes: descriptor.file_bytes,
                sha256: descriptor.sha256,
                max_context_tokens: descriptor.max_context_tokens,
                is_default: false,
            });
        }
    }
    Ok(models)
}

fn default_model_settings(model: &AvailableModel) -> ModelSettings {
    let (temperature, top_p) = match model.family.as_str() {
        family if family.starts_with("llama") => (0.4, 0.85),
        "qwen3" | "qwen-3" => (0.7, 0.8),
        "ministral3" | "ministral-3" => (0.05, 0.9),
        _ => (0.7, 1.0),
    };
    ModelSettings {
        model_id: model.id.clone(),
        context_tokens: None,
        temperature,
        top_p,
    }
}

#[cfg(test)]
mod model_settings_tests {
    use super::{default_model_settings, AvailableModel};

    fn model(family: &str) -> AvailableModel {
        AvailableModel {
            id: family.to_owned(),
            display_name: family.to_owned(),
            family: family.to_owned(),
            gguf_bytes: 1,
            sha256: "test".to_owned(),
            max_context_tokens: 8192,
            is_default: false,
        }
    }

    #[test]
    fn qwen3_defaults_follow_model_generation_config() {
        let settings = default_model_settings(&model("qwen3"));
        assert_eq!(settings.temperature, 0.7);
        assert_eq!(settings.top_p, 0.8);
    }

    #[test]
    fn ministral3_defaults_use_conservative_temperature() {
        let settings = default_model_settings(&model("ministral3"));
        assert!(settings.temperature < 0.1);
    }
}

fn stored_model_settings(vault: &HotVault, model_id: &str) -> AppResult<Option<ModelSettings>> {
    let key = format!("{SETTING_MODEL_PROFILE_PREFIX}{model_id}");
    let raw = vault.repository().setting(&key)?;
    let Some(raw) = raw else { return Ok(None) };
    let settings: ModelSettings = match serde_json::from_str(&raw) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("Modellprofil `{model_id}` beschädigt: {error}");
            return Ok(None);
        }
    };
    if settings.model_id != model_id
        || settings
            .context_tokens
            .is_some_and(|value| !(2048..=131_072).contains(&value))
        || !settings.temperature.is_finite()
        || !(0.0..=2.0).contains(&settings.temperature)
        || !settings.top_p.is_finite()
        || !(0.01..=1.0).contains(&settings.top_p)
    {
        eprintln!("Modellprofil `{model_id}` enthält ungültige Werte");
        return Ok(None);
    }
    Ok(Some(settings))
}

fn model_from_list(state: State<'_, AppState>, model_id: &str) -> AppResult<AvailableModel> {
    installed_models(state)?
        .into_iter()
        .find(|model| model.id == model_id)
        .ok_or_else(|| AppError::Invalid(format!("Modell `{model_id}` ist nicht installiert")))
}

fn model_profile(session: &SessionHandle, model: &AvailableModel) -> AppResult<ModelSettings> {
    let vault = session.vault_runtime.lock()?;
    Ok(stored_model_settings(&vault, &model.id)?.unwrap_or_else(|| default_model_settings(model)))
}

fn validate_model_settings(settings: &ModelSettings, model: &AvailableModel) -> AppResult<()> {
    if settings
        .context_tokens
        .is_some_and(|value| value < 2048 || value > model.max_context_tokens)
        || !settings.temperature.is_finite()
        || !(0.0..=2.0).contains(&settings.temperature)
        || !settings.top_p.is_finite()
        || !(0.01..=1.0).contains(&settings.top_p)
    {
        return Err(AppError::Invalid(
            "Modellwerte außerhalb der zulässigen Grenzen".to_owned(),
        ));
    }
    Ok(())
}

/// Liest nur die Einstellungen des gewählten installierten Modells aus dem Vault.
#[tauri::command]
fn get_model_settings(state: State<'_, AppState>, model_id: String) -> AppResult<ModelSettings> {
    let model = model_from_list(state.clone(), &model_id)?;
    let session = require_session(&state)?;
    model_profile(&session, &model)
}

/// Speichert Sampling und Kontext pro Modell. Änderungen am aktiven Kontext
/// führen erst nach erfolgreichem Server-Neustart zu einem neuen Profil.
#[tauri::command]
async fn save_model_settings(app: AppHandle, settings: ModelSettings) -> AppResult<ModelSettings> {
    let state = app.state::<AppState>();
    let model = model_from_list(state.clone(), &settings.model_id)?;
    validate_model_settings(&settings, &model)?;
    let session = require_session(&state)?;
    let active = session.model_id == settings.model_id;
    let previous_override = {
        let guard = lock(&state.session)?;
        guard.as_ref().and_then(|value| value.context_override)
    };
    if active && previous_override != settings.context_tokens {
        {
            let mut guard = lock(&state.session)?;
            if let Some(value) = guard.as_mut() {
                value.context_override = settings.context_tokens;
            }
        }
        let worker_app = app.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            reload_engine(&worker_app.state::<AppState>())
        })
        .await
        .map_err(|error| AppError::Internal(error.to_string()))
        .and_then(|result| result);
        if let Err(error) = result {
            let mut guard = lock(&state.session)?;
            if let Some(value) = guard.as_mut() {
                value.context_override = previous_override;
            }
            return Err(error);
        }
    }
    if active {
        let mut engine = session
            .engine
            .lock()
            .map_err(|_| AppError::Internal("Engine-Mutex vergiftet".to_owned()))?;
        engine.set_sampling(Some(pa_launcher::inference_backend::SamplingSettings {
            temperature: settings.temperature,
            top_p: settings.top_p,
        }));
    }
    let key = format!("{SETTING_MODEL_PROFILE_PREFIX}{}", model.id);
    let mut vault = session.vault_runtime.lock()?;
    vault.repository_mut().set_setting(
        &key,
        &serde_json::to_string(&settings).map_err(|error| AppError::Internal(error.to_string()))?,
    )?;
    Ok(settings)
}

struct PreparedModel {
    descriptor: ModelDescriptor,
    cached_path: PathBuf,
    plan: ResourcePlan,
}

fn prepare_selected_model(
    bootstrap: &StoredBootstrap,
    model_id: &str,
    tier_override: Option<HardwareTier>,
    context_override: Option<u32>,
) -> AppResult<PreparedModel> {
    if model_id.is_empty()
        || model_id.len() > 100
        || !model_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(AppError::Invalid("Ungültige Modell-ID".to_owned()));
    }
    let package = PackageRoot::new(&bootstrap.package_root)?;
    let models_dir = package.resolve_existing(Path::new("AI/models"))?;
    let mut selected = None;
    for entry in std::fs::read_dir(models_dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !name.ends_with(".model.toml") {
            continue;
        }
        let relative = Path::new("AI/models").join(name);
        let Ok(descriptor) = load_model_descriptor(&package, &relative) else {
            continue;
        };
        if descriptor.id == model_id && selected.replace(descriptor).is_some() {
            return Err(AppError::Invalid(
                "Modell-ID ist mehrfach installiert".to_owned(),
            ));
        }
    }
    let descriptor = selected
        .ok_or_else(|| AppError::Invalid(format!("Modell `{model_id}` ist nicht installiert")))?;
    let source = package.resolve_existing(&Path::new("AI/models").join(&descriptor.gguf_file))?;
    let source_bytes = std::fs::metadata(&source)?.len();
    if source_bytes != descriptor.file_bytes {
        return Err(AppError::Invalid(format!(
            "Modell `{model_id}` hat nicht die erwartete Dateigröße"
        )));
    }
    let plan = compute_resource_plan(
        &bootstrap.hardware,
        &descriptor,
        PlanRequest {
            tier_override,
            context_override,
            kv_quantization: bootstrap.kv_quantization,
        },
    )?;
    let cached_path = if model_id == bootstrap.descriptor_id {
        bootstrap.cached_model_path.clone()
    } else {
        let cache = ModelCache::for_current_host()?;
        cache
            .ensure_cached(
                &source,
                &descriptor.sha256,
                descriptor.file_bytes,
                &mut NoopProgress,
            )?
            .path()
            .to_path_buf()
    };
    Ok(PreparedModel {
        descriptor,
        cached_path,
        plan,
    })
}

/// Wählt ein installiertes Modell erst nach Dateiprüfung und erfolgreichem
/// Server-Health-Check. Die mehrgigabytegroße Cache-Kopie und der Prozessstart
/// laufen außerhalb des UI-Threads.
#[tauri::command]
async fn select_model(app: AppHandle, model_id: String) -> AppResult<SettingsSnapshot> {
    let state = app.state::<AppState>();
    let bootstrap = ensure_bootstrap(&state)?;
    let server_executable = bootstrap.server_executable.clone();
    let selected_model = model_from_list(state.clone(), &model_id)?;
    let session = require_session(&state)?;
    let profile = model_profile(&session, &selected_model)?;
    validate_model_settings(&profile, &selected_model)?;
    let (current_id, tier_override) = {
        let guard = lock(&state.session)?;
        let session = guard.as_ref().ok_or_else(AppError::locked)?;
        (session.model_id.clone(), session.tier_override)
    };
    if current_id == model_id {
        return settings_snapshot(state);
    }

    let context_override = profile.context_tokens;
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        prepare_selected_model(&bootstrap, &model_id, tier_override, context_override)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    let selected_id = prepared.descriptor.id.clone();
    let (config, _) = server_configuration(
        server_executable,
        prepared.cached_path,
        &selected_id,
        &prepared.plan,
    )?;
    {
        let mut vault = session.vault_runtime.lock()?;
        vault
            .repository_mut()
            .set_setting(SETTING_ACTIVE_MODEL, &selected_id)?;
    }
    let engine = Arc::clone(&session.engine);
    let family = prepared.descriptor.family;
    let switch_result = tauri::async_runtime::spawn_blocking(move || -> AppResult<u64> {
        let mut engine = engine
            .lock()
            .map_err(|_| AppError::Internal("Engine-Mutex vergiftet".to_owned()))?;
        let started = std::time::Instant::now();
        engine
            .switch_to(config, AdapterKind::from_family(&family, &selected_id))
            .map_err(|error| AppError::Launcher(error.to_string()))?;
        let start_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        engine.set_sampling(Some(pa_launcher::inference_backend::SamplingSettings {
            temperature: profile.temperature,
            top_p: profile.top_p,
        }));
        Ok(start_ms)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?;
    if let Ok(start_ms) = &switch_result {
        // Startzeit für den Leistungs-Check (Feature 3).
        *lock(&state.model_start_ms)? = Some(*start_ms);
    }
    if let Err(error) = switch_result {
        let mut vault = session.vault_runtime.lock()?;
        vault
            .repository_mut()
            .set_setting(SETTING_ACTIVE_MODEL, &current_id)?;
        return Err(error);
    }
    {
        let mut guard = lock(&state.session)?;
        let active = guard.as_mut().ok_or_else(AppError::locked)?;
        active.model_id = prepared.descriptor.id;
        active.context_tokens = prepared.plan.context_tokens;
        active.context_override = context_override;
    }
    settings_snapshot(state)
}

/// Aktueller Stand aller wählbaren Einstellungen.
#[tauri::command]
fn settings_snapshot(state: State<'_, AppState>) -> AppResult<SettingsSnapshot> {
    let session = require_session(&state)?;
    let theme = read_theme(&session)?.unwrap_or_default();
    Ok(SettingsSnapshot {
        tier_override: session.tier_override,
        model_id: session.model_id.clone(),
        context_tokens: session.effective_context_tokens(),
        kv_quantization: session.kv_quantization,
        theme,
        vault_path: session.vault_path.to_string_lossy().into_owned(),
    })
}

/// Wendet einen Einstellungsupdate an und startet den Inferenzserver bei
/// modell- oder kontextrelevanten Änderungen neu.
#[tauri::command]
async fn apply_settings(app: AppHandle, update: SettingsUpdate) -> AppResult<SettingsSnapshot> {
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        apply_settings_inner(&worker_app.state::<AppState>(), update)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    settings_snapshot(app.state())
}

fn apply_settings_inner(state: &AppState, update: SettingsUpdate) -> AppResult<()> {
    let requires_engine_reload = {
        let mut guard = lock(&state.session)?;
        let session = guard.as_mut().ok_or_else(AppError::locked)?;
        if update
            .model_id
            .as_deref()
            .is_some_and(|id| id != session.model_id)
        {
            return Err(AppError::Invalid(
                "Modellwechsel über select_model ausführen".to_owned(),
            ));
        }
        let mut reload = false;
        if let Some(change) = update.tier_override {
            session.tier_override = match change {
                TierOverrideChange::Clear => None,
                TierOverrideChange::Set { tier } => Some(tier),
            };
            reload = true;
        }
        if let Some(context) = update.context_tokens {
            if context == 0 {
                return Err(AppError::Invalid(
                    "Kontextlänge muss größer als null sein".to_owned(),
                ));
            }
            session.context_override = Some(context);
            reload = true;
        }
        if let Some(theme) = update.theme {
            persist_theme(session, theme)?;
        }
        reload
    };
    if requires_engine_reload {
        reload_engine(state)?;
    }
    Ok(())
}

fn reload_engine(state: &AppState) -> AppResult<()> {
    let bootstrap = ensure_bootstrap(state)?;
    let (model_id, tier_override, context_override, engine) = {
        let guard = lock(&state.session)?;
        let session = guard.as_ref().ok_or_else(AppError::locked)?;
        (
            session.model_id.clone(),
            session.tier_override,
            session.context_override,
            Arc::clone(&session.engine),
        )
    };
    let prepared = prepare_selected_model(&bootstrap, &model_id, tier_override, context_override)?;
    let (config, _) = server_configuration(
        bootstrap.server_executable.clone(),
        prepared.cached_path,
        &prepared.descriptor.id,
        &prepared.plan,
    )?;
    let mut engine_guard = engine
        .lock()
        .map_err(|_| AppError::Internal("Engine-Mutex vergiftet".to_owned()))?;
    engine_guard
        .switch_to(
            config,
            AdapterKind::from_family(&prepared.descriptor.family, &model_id),
        )
        .map_err(|error| {
            AppError::Launcher(format!("Server erneut starten fehlgeschlagen: {error}"))
        })?;
    drop(engine_guard);
    let mut guard = lock(&state.session)?;
    if let Some(session) = guard.as_mut() {
        session.context_tokens = prepared.plan.context_tokens;
    }
    Ok(())
}

/// Schließt den aktuellen Vault, syncht ihn deterministisch zurück und öffnet
/// einen neuen Vault am angegebenen Pfad.
#[tauri::command]
async fn switch_vault(app: AppHandle, request: VaultSwitchRequest) -> AppResult<SettingsSnapshot> {
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        switch_vault_inner(&worker_app.state::<AppState>(), request)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    settings_snapshot(app.state())
}

fn switch_vault_inner(state: &AppState, request: VaultSwitchRequest) -> AppResult<()> {
    let passphrase = Zeroizing::new(request.passphrase);
    // Alte Session schließen. Die Gmail-Auto-Antwort gehört zur Sitzung und endet mit ihr.
    state.mail.stop();
    let previous = {
        let mut guard = lock(&state.session)?;
        guard.take()
    };
    if let Some(session) = previous {
        session.shutdown()?;
    }
    let new_vault = PathBuf::from(request.new_vault_path);
    let bootstrap = ensure_bootstrap(state)?;
    let meta_path = new_vault.with_extension("meta");
    let meta = load_or_create_meta(&new_vault, &meta_path, request.create_if_missing)?;
    let key = derive_key(&passphrase, &meta)?;
    let hot_directory = hot_vault_directory(&new_vault, &bootstrap.host_identifier)?;
    let vault = HotVault::start(&new_vault, &hot_directory, key)?;
    open_session(state, &bootstrap, vault, new_vault)
}

/// Startet einen Streaming-Turn im Hintergrund; Deltas kommen über den
/// `chat-stream`-Event.
#[tauri::command]
fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    request: SendMessageRequest,
) -> AppResult<String> {
    let session = require_session(&state)?;
    let thinking_level = request.thinking_level;
    let skill_id = request.skill_id;
    let content = request.content;
    if content.trim().is_empty() {
        return Err(AppError::Invalid(
            "Nachricht darf nicht leer sein".to_owned(),
        ));
    }
    let conversation_id = match request.conversation_id {
        Some(id) => id,
        None => {
            let mut conversations = SharedConversations::new(session.vault_runtime.shared()?);
            let title: String = content.chars().take(64).collect();
            let id = random_id("conversation");
            conversations.create(&id, &title, now_unix_ms())?;
            if let Ok(mut vault) = session.vault_runtime.lock() {
                let mut no_fault = pa_vault::hot_copy::NoFault;
                let _ = vault.sync(&mut no_fault);
            }
            id
        }
    };
    let user_id = random_id("message");
    let assistant_id = random_id("message");
    // Projekt-Grundanweisung und Dokument-Quellen für genau diese Frage.
    let package_root = ensure_bootstrap(&state)?.package_root.clone();
    let (turn, source_texts) = match session.vault_runtime.lock() {
        Ok(vault) => {
            let (mut turn, texts) = library_cmds::turn_context(
                vault.repository(),
                &conversation_id,
                &content,
                session.context_tokens,
            );
            if let Some(id) = skill_id.as_deref() {
                instruction_skills::attach_one(
                    &mut turn,
                    &package_root,
                    id,
                    session.context_tokens,
                )
                .map_err(AppError::Invalid)?;
            }
            instruction_skills::attach(
                &mut turn,
                vault.repository(),
                &package_root,
                session.context_tokens,
            );
            (turn, texts)
        }
        Err(_) => Default::default(),
    };
    if let Ok(sources) = library_cmds::store_sources(&state, &assistant_id, &turn.sources) {
        if !sources.is_empty() {
            let _ = app.emit("chat-sources", sources);
        }
    }
    // Cancel-Signal für diesen Turn reserviert.
    state.stream_cancel.store(false, Ordering::SeqCst);
    let cancel = Arc::clone(&state.stream_cancel);
    let vault = session.vault_runtime.shared()?;
    let engine = Arc::clone(&session.engine);
    let orchestrator = session.build_orchestrator(&turn, &source_texts);
    let task = StreamTask {
        app,
        engine,
        vault,
        orchestrator,
        cancel,
        job_cancel: Arc::new(AtomicBool::new(false)),
        conversation_id: conversation_id.clone(),
        user_id,
        assistant_id: assistant_id.clone(),
        content,
        thinking_level,
    };
    // Jede Antwort wartet in der seriellen Queue, damit Sprache, Bild und Sprachmodell
    // auf schwacher Hardware nie gleichzeitig rechnen.
    let mut queue_job =
        state
            .flow
            .jobs
            .submit(pa_types::avatar::JobKind::Chat, "Antwort erzeugen", true);
    let job_cancel = queue_job.cancel_flag();
    let mut task = task;
    task.job_cancel = job_cancel;
    thread::spawn(move || match queue_job.acquire() {
        Ok(slot) => {
            run_stream(task);
            drop(slot);
        }
        Err(_) => {
            let _ = task.app.emit(
                "chat-stream",
                StreamEvent::Failed {
                    assistant_message_id: Some(task.assistant_id.clone()),
                    error_kind: pa_types::chat::StreamErrorKind::ProcessExited,
                    message: "Der Auftrag wurde abgebrochen.".to_owned(),
                    partial_text: String::new(),
                    timings: pa_types::chat::ServerTimingsDto::default(),
                },
            );
        }
    });
    Ok(assistant_id)
}

/// Bündelt alle Argumente eines einzelnen Streaming-Turns; hält die
/// Übergabe an den Hintergrundthread übersichtlich.
struct StreamTask {
    app: AppHandle,
    engine: Arc<Mutex<LlamaServerEngine>>,
    vault: Arc<Mutex<HotVault>>,
    orchestrator: ChatOrchestrator,
    cancel: Arc<AtomicBool>,
    /// Abbruch über die Job-Übersicht (zusätzlich zum Stopp-Knopf).
    job_cancel: Arc<AtomicBool>,
    conversation_id: String,
    user_id: String,
    assistant_id: String,
    content: String,
    thinking_level: pa_types::chat::ThinkingLevel,
}

fn run_stream(task: StreamTask) {
    let StreamTask {
        app,
        engine,
        vault,
        orchestrator,
        cancel,
        job_cancel,
        conversation_id,
        user_id,
        assistant_id,
        content,
        thinking_level,
    } = task;
    let vault_sync = Arc::clone(&vault);
    let mut conversations = SharedConversations::new(vault);
    let mut should_continue =
        || !cancel.load(Ordering::SeqCst) && !job_cancel.load(Ordering::SeqCst);
    let app_for_delta = app.clone();
    let assistant_id_for_delta = assistant_id.clone();
    let mut on_delta = |delta: &str| {
        let _ = app_for_delta.emit(
            "chat-stream",
            StreamEvent::Delta {
                assistant_message_id: assistant_id_for_delta.clone(),
                text: delta.to_owned(),
            },
        );
    };
    let mut engine_status_notified = false;
    let mut on_engine_ready = |status: EngineReady| {
        engine_status_notified = true;
        let _ = app.emit(
            "chat-stream",
            StreamEvent::Started {
                conversation_id: conversation_id.clone(),
                user_message_id: user_id.clone(),
                assistant_message_id: assistant_id.clone(),
                engine_restarted: matches!(status, EngineReady::Restarted),
            },
        );
    };
    let request = TurnRequest {
        conversation_id: &conversation_id,
        user_message_id: &user_id,
        assistant_message_id: &assistant_id,
        user_input: &content,
        now_unix_ms: now_unix_ms(),
    };
    let mut engine_guard = match engine.lock() {
        Ok(guard) => guard,
        Err(_) => {
            let _ = app.emit(
                "chat-stream",
                StreamEvent::Failed {
                    assistant_message_id: Some(assistant_id.clone()),
                    error_kind: pa_types::chat::StreamErrorKind::ProcessExited,
                    message: "Engine-Mutex vergiftet".to_owned(),
                    partial_text: String::new(),
                    timings: pa_types::chat::ServerTimingsDto::default(),
                },
            );
            return;
        }
    };
    engine_guard.set_thinking_level(thinking_level);
    let mut callbacks = TurnCallbacks {
        should_continue: &mut should_continue,
        on_delta: &mut on_delta,
        on_engine_ready: &mut on_engine_ready,
    };
    let engine_ref: &mut dyn ChatEngine = &mut *engine_guard;
    let result = orchestrator.run_turn(&mut conversations, engine_ref, request, &mut callbacks);
    match result {
        Ok(outcome) => {
            let _ = app.emit(
                "chat-stream",
                StreamEvent::Finished {
                    assistant_message_id: assistant_id.clone(),
                    outcome: outcome.outcome,
                    dropped_older_turns: u32::try_from(outcome.dropped_older_turns)
                        .unwrap_or(u32::MAX),
                },
            );
        }
        Err(TurnError::Stream(failure)) => {
            let _ = app.emit(
                "chat-stream",
                StreamEvent::Failed {
                    assistant_message_id: Some(failure.assistant_message_id),
                    error_kind: failure.source.kind,
                    message: failure.source.message,
                    partial_text: failure.source.partial_text,
                    timings: failure.source.timings,
                },
            );
        }
        Err(TurnError::Core(error)) => {
            let assistant = if engine_status_notified {
                Some(assistant_id.clone())
            } else {
                None
            };
            let _ = app.emit(
                "chat-stream",
                StreamEvent::Failed {
                    assistant_message_id: assistant,
                    error_kind: pa_types::chat::StreamErrorKind::Adapter,
                    message: error.to_string(),
                    partial_text: String::new(),
                    timings: pa_types::chat::ServerTimingsDto::default(),
                },
            );
        }
    }
    drop(conversations);
    if let Ok(mut vault_guard) = vault_sync.lock() {
        let mut no_fault = pa_vault::hot_copy::NoFault;
        let _ = vault_guard.sync(&mut no_fault);
    };
}

/// Bricht einen laufenden Stream ab; der bereits sichtbare Teiltext bleibt erhalten.
#[tauri::command]
fn cancel_stream(state: State<'_, AppState>) -> AppResult<()> {
    state.stream_cancel.store(true, Ordering::SeqCst);
    // Auch wartende Antworten in der Queue beenden.
    state
        .flow
        .jobs
        .cancel_kinds(&[pa_types::avatar::JobKind::Chat]);
    Ok(())
}

/// Werkzeugmodus für den Chat.
///
/// Führt den Turn durch die pa-core-Werkzeugschleife statt reinen Streams:
/// Nutzernachricht und finale Assistentenantwort landen in der Konversation,
/// jeder Werkzeugaufruf und jedes Werkzeug-Ergebnis wird zusätzlich als
/// [`ToolStreamEvent`] auf dem `tool-stream`-Event emittiert, damit die UI
/// den Verlauf inline zeigen kann.
///
/// Rückgabe: die persistierte Assistant-Message-ID. Bei Fehler wird die
/// Nachricht als `Aborted` mit Fehlertext gespeichert und der Fehler
/// zusätzlich als `ToolStreamEvent::ToolError` emittiert.
#[tauri::command]
fn send_message_with_tools(
    app: AppHandle,
    state: State<'_, AppState>,
    request: SendMessageRequest,
) -> AppResult<String> {
    use pa_core::tool_loop::{ToolEvent, ToolLoopError};
    use pa_types::ipc::ToolStreamEvent;

    let session_guard = lock(&state.session)?;
    let session = session_guard.as_ref().ok_or_else(AppError::locked)?;
    let vault_shared = session.vault_runtime.shared()?;
    let engine = Arc::clone(&session.engine);
    let tool_runtime = Arc::clone(&session.tool_runtime);
    let context_tokens = session.context_tokens;
    drop(session_guard);

    let thinking_level = request.thinking_level;
    let skill_id = request.skill_id;
    let content = request.content;
    if content.trim().is_empty() {
        return Err(AppError::Invalid(
            "Nachricht darf nicht leer sein".to_owned(),
        ));
    }
    let conversation_id = match request.conversation_id {
        Some(id) => id,
        None => {
            let mut conversations = SharedConversations::new(Arc::clone(&vault_shared));
            let title: String = content.chars().take(64).collect();
            let id = random_id("conversation");
            conversations.create(&id, &title, now_unix_ms())?;
            if let Ok(mut vault) = vault_shared.lock() {
                let mut no_fault = pa_vault::hot_copy::NoFault;
                let _ = vault.sync(&mut no_fault);
            }
            id
        }
    };
    let user_id = random_id("message");
    let assistant_id = random_id("message");
    let now = now_unix_ms();
    {
        let mut conversations = SharedConversations::new(Arc::clone(&vault_shared));
        conversations.append(
            &user_id,
            &conversation_id,
            pa_types::chat::MessageRole::User,
            &content,
            pa_types::chat::MessageStatus::Complete,
            now,
        )?;
        conversations.append(
            &assistant_id,
            &conversation_id,
            pa_types::chat::MessageRole::Assistant,
            "",
            pa_types::chat::MessageStatus::Streaming,
            now,
        )?;
    }
    if let Ok(mut vault) = vault_shared.lock() {
        let mut no_fault = pa_vault::hot_copy::NoFault;
        let _ = vault.sync(&mut no_fault);
    }

    // Projekt-Grundanweisung und Dokument-Quellen wie im Chat ohne Werkzeuge.
    let skills_root = ensure_bootstrap(&state)?.package_root.clone();
    let (turn, source_texts) = match vault_shared.lock() {
        Ok(vault) => {
            let (mut turn, texts) = library_cmds::turn_context(
                vault.repository(),
                &conversation_id,
                &content,
                context_tokens,
            );
            if let Some(id) = skill_id.as_deref() {
                let applied =
                    instruction_skills::attach_one(&mut turn, &skills_root, id, context_tokens)
                        .map_err(AppError::Invalid)?;
                if !applied {
                    instruction_skills::wasm_hint(&mut turn, id);
                }
            }
            instruction_skills::attach(&mut turn, vault.repository(), &skills_root, context_tokens);
            (turn, texts)
        }
        Err(_) => Default::default(),
    };
    if let Ok(sources) = library_cmds::store_sources(&state, &assistant_id, &turn.sources) {
        if !sources.is_empty() {
            let _ = app.emit("chat-sources", sources);
        }
    }
    let project_prompt = turn.project_prompt.clone();
    let sources_prompt = turn.sources_prompt(&source_texts);

    let app_clone = app.clone();
    let assistant_for_thread = assistant_id.clone();
    let vault_for_thread = Arc::clone(&vault_shared);
    let cancel = Arc::clone(&state.stream_cancel);
    let pending_store = Arc::clone(&state.pending_permissions);
    cancel.store(false, Ordering::SeqCst);
    // Auch der Werkzeug-Chat wartet in der seriellen Queue (sichtbar in der Job-Übersicht).
    let mut queue_job = state.flow.jobs.submit(
        pa_types::avatar::JobKind::Chat,
        "Antwort mit Werkzeugen",
        true,
    );
    std::thread::spawn(move || {
        let Ok(_queue_slot) = queue_job.acquire() else {
            let _ = app_clone.emit(
                "chat-stream",
                StreamEvent::Failed {
                    assistant_message_id: Some(assistant_for_thread.clone()),
                    error_kind: pa_types::chat::StreamErrorKind::ProcessExited,
                    message: "Der Auftrag wurde abgebrochen.".to_owned(),
                    partial_text: String::new(),
                    timings: pa_types::chat::ServerTimingsDto::default(),
                },
            );
            return;
        };
        let mut runtime = match tool_runtime.lock() {
            Ok(runtime) => runtime,
            Err(_) => {
                let _ = app_clone.emit(
                    "tool-stream",
                    ToolStreamEvent::ToolError {
                        assistant_message_id: assistant_for_thread.clone(),
                        tool: "tool_runtime".to_owned(),
                        message: "Werkzeug-Runtime vergiftet".to_owned(),
                    },
                );
                return;
            }
        };
        let assistant_for_events = assistant_for_thread.clone();
        let app_for_events = app_clone.clone();
        let mut hook = TauriPermissionHook {
            app: app_clone.clone(),
            pending_store: Arc::clone(&pending_store),
            assistant_message_id: assistant_for_thread.clone(),
        };
        let mut custom_prefix = Vec::new();
        if let Ok(vault) = vault_for_thread.lock() {
            let repo = vault.repository();
            let is_enabled = repo
                .setting(SETTING_USER_PROFILE_ENABLED)
                .ok()
                .flatten()
                .map(|v| v == "true")
                .unwrap_or(false);

            let mut parts = Vec::new();
            parts.extend(project_prompt.clone());
            if is_enabled {
                if let Ok(Some(name)) = repo.setting(SETTING_USER_NAME) {
                    if !name.trim().is_empty() {
                        parts.push(format!("Name des Nutzers: {}", name.trim()));
                    }
                }
                if let Ok(Some(about)) = repo.setting(SETTING_USER_ABOUT) {
                    if !about.trim().is_empty() {
                        parts.push(format!("Über den Nutzer: {}", about.trim()));
                    }
                }
                if let Ok(Some(sys)) = repo.setting(SETTING_CUSTOM_SYSTEM_PROMPT) {
                    if !sys.trim().is_empty() {
                        parts.push(format!("System-Instruktion: {}", sys.trim()));
                    }
                }
            }
            // Antwortsprache gilt unabhängig vom persönlichen Profil.
            parts.extend(
                ui_language::response_instruction(ui_language::current()).map(str::to_owned),
            );
            if !parts.is_empty() {
                custom_prefix.push(pa_types::chat::Message {
                    id: "custom-profile".to_owned(),
                    conversation_id: String::new(),
                    position: 0,
                    role: pa_types::chat::MessageRole::System,
                    content: parts.join("\n"),
                    status: pa_types::chat::MessageStatus::Complete,
                    created_at_unix_ms: now,
                });
            }
        }
        if let Some(sources) = sources_prompt.clone() {
            custom_prefix.push(pa_types::chat::Message {
                id: "document-sources".to_owned(),
                conversation_id: String::new(),
                position: 0,
                role: pa_types::chat::MessageRole::System,
                content: sources,
                status: pa_types::chat::MessageStatus::Complete,
                created_at_unix_ms: now,
            });
        }

        let result = runtime.run_turn_with_hook(
            &content,
            now,
            Some(&mut hook),
            |messages| {
                let mut collected = String::new();
                let cancel = Arc::clone(&cancel);
                let mut should_continue = || !cancel.load(Ordering::SeqCst);
                let mut on_delta = |delta: &str| -> bool {
                    collected.push_str(delta);
                    true
                };
                let mut engine_guard = match engine.lock() {
                    Ok(guard) => guard,
                    Err(_) => {
                        return Err(ToolLoopError::Engine("Engine-Mutex vergiftet".to_owned()))
                    }
                };
                engine_guard.set_thinking_level(thinking_level);
                let mut full_messages = custom_prefix.clone();
                full_messages.extend_from_slice(messages);
                let outcome =
                    engine_guard.stream_chat(&full_messages, &mut should_continue, &mut on_delta);
                drop(engine_guard);
                match outcome {
                    Ok(dto) => {
                        if dto.aborted {
                            Err(ToolLoopError::Engine(
                                "Stream durch Nutzer abgebrochen".to_owned(),
                            ))
                        } else {
                            Ok(collected)
                        }
                    }
                    Err(error) => Err(ToolLoopError::Engine(error.message)),
                }
            },
            |event| match event {
                ToolEvent::ModelText { .. } => {}
                ToolEvent::ToolCall { tool, arguments } => {
                    let _ = app_for_events.emit(
                        "tool-stream",
                        ToolStreamEvent::ToolCall {
                            assistant_message_id: assistant_for_events.clone(),
                            tool,
                            arguments_json: arguments.to_string(),
                        },
                    );
                }
                ToolEvent::ToolResult {
                    tool,
                    content,
                    is_untrusted,
                } => {
                    let _ = app_for_events.emit(
                        "tool-stream",
                        ToolStreamEvent::ToolResult {
                            assistant_message_id: assistant_for_events.clone(),
                            tool,
                            content,
                            is_untrusted,
                        },
                    );
                }
                ToolEvent::ToolError { tool, message } => {
                    let _ = app_for_events.emit(
                        "tool-stream",
                        ToolStreamEvent::ToolError {
                            assistant_message_id: assistant_for_events.clone(),
                            tool,
                            message,
                        },
                    );
                }
            },
        );

        let vault_sync = Arc::clone(&vault_for_thread);
        let mut conversations = SharedConversations::new(vault_for_thread);
        match result {
            Ok(final_text) => {
                let _ = conversations.finish(
                    &assistant_for_thread,
                    &final_text,
                    pa_types::chat::MessageStatus::Complete,
                );
                let _ = app_clone.emit(
                    "chat-stream",
                    StreamEvent::Finished {
                        assistant_message_id: assistant_for_thread.clone(),
                        outcome: pa_types::chat::StreamOutcomeDto {
                            text: final_text,
                            aborted: false,
                            timings: pa_types::chat::ServerTimingsDto::default(),
                            prompt_tokens: None,
                            completion_tokens: None,
                        },
                        dropped_older_turns: 0,
                    },
                );
            }
            Err(error) => {
                let message = error.to_string();
                let _ = conversations.finish(
                    &assistant_for_thread,
                    &format!("[abgebrochen] {message}"),
                    pa_types::chat::MessageStatus::Aborted,
                );
                let _ = app_clone.emit(
                    "chat-stream",
                    StreamEvent::Failed {
                        assistant_message_id: Some(assistant_for_thread.clone()),
                        error_kind: pa_types::chat::StreamErrorKind::Adapter,
                        message,
                        partial_text: String::new(),
                        timings: pa_types::chat::ServerTimingsDto::default(),
                    },
                );
            }
        }
        drop(conversations);
        if let Ok(mut vault_guard) = vault_sync.lock() {
            let mut no_fault = pa_vault::hot_copy::NoFault;
            let _ = vault_guard.sync(&mut no_fault);
        };
    });
    Ok(assistant_id)
}

// -------------------------------------------------------------
// Phase 2: Memory-, Logs- und Dateien-Commands
// -------------------------------------------------------------

pub(crate) fn session_memory(state: &AppState) -> AppResult<Arc<Mutex<MemoryStore>>> {
    let guard = lock(&state.session)?;
    let session = guard.as_ref().ok_or_else(AppError::locked)?;
    Ok(Arc::clone(&session.memory))
}

pub(crate) fn session_workspace(state: &AppState) -> AppResult<PathBuf> {
    let guard = lock(&state.session)?;
    let session = guard.as_ref().ok_or_else(AppError::locked)?;
    Ok(session.workspace_dir.clone())
}

/// Hybride Suche über Fakten und Dokumentchunks (BM25 + Vektor + RRF).
#[tauri::command]
fn retrieve_memory(state: State<'_, AppState>, query: String) -> AppResult<MemoryRetrieveResponse> {
    if query.trim().is_empty() {
        return Err(AppError::Invalid("query darf nicht leer sein".to_owned()));
    }
    let memory = session_memory(&state)?;
    let store = memory
        .lock()
        .map_err(|_| AppError::Internal("Memory-Mutex vergiftet".to_owned()))?;
    let hits = store
        .retrieve(&query)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(MemoryRetrieveResponse { hits, query })
}

/// Alle nicht-abgelösten Fakten, wie sie der Memory-Bereich der UI zeigt.
#[tauri::command]
fn list_active_facts(state: State<'_, AppState>) -> AppResult<Vec<Fact>> {
    let memory = session_memory(&state)?;
    let store = memory
        .lock()
        .map_err(|_| AppError::Internal("Memory-Mutex vergiftet".to_owned()))?;
    store
        .active_facts()
        .map_err(|error| AppError::Internal(error.to_string()))
}

/// Nutzer-getriebenes Anlegen oder Aktualisieren eines Faktes (Konzept 6.5).
#[tauri::command]
fn upsert_fact(state: State<'_, AppState>, request: MemoryUpsertRequest) -> AppResult<Fact> {
    if request.text.trim().is_empty() {
        return Err(AppError::Invalid(
            "Fakt-Text darf nicht leer sein".to_owned(),
        ));
    }
    let memory = session_memory(&state)?;
    let mut store = memory
        .lock()
        .map_err(|_| AppError::Internal("Memory-Mutex vergiftet".to_owned()))?;
    let now = now_unix_ms();
    let id = store
        .ingest(IngestFact {
            id: request.id,
            text: request.text,
            category: request.category,
            confidence: if request.user_verified { 1.0 } else { 0.7 },
            source_message_id: None,
            valid_from_unix_ms: now,
            user_verified: request.user_verified,
        })
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let facts = store
        .active_facts()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    drop(store);
    if let Ok(session) = require_session(&state) {
        if let Ok(mut vault) = session.vault_runtime.lock() {
            let mut no_fault = pa_vault::hot_copy::NoFault;
            let _ = vault.sync(&mut no_fault);
        }
    }
    facts
        .into_iter()
        .find(|fact| fact.id == id)
        .ok_or_else(|| AppError::Internal("Fakt nach Ingest nicht auffindbar".to_owned()))
}

/// Löscht einen Fakt endgültig.
#[tauri::command]
fn forget_fact(state: State<'_, AppState>, fact_id: String) -> AppResult<()> {
    let memory = session_memory(&state)?;
    let mut store = memory
        .lock()
        .map_err(|_| AppError::Internal("Memory-Mutex vergiftet".to_owned()))?;
    store
        .forget(&fact_id)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    drop(store);
    if let Ok(session) = require_session(&state) {
        if let Ok(mut vault) = session.vault_runtime.lock() {
            let mut no_fault = pa_vault::hot_copy::NoFault;
            let _ = vault.sync(&mut no_fault);
        }
    }
    Ok(())
}

/// Exportiert alle aktiven Fakten als JSON-freundliche Struktur.
#[tauri::command]
fn export_memory(state: State<'_, AppState>) -> AppResult<MemoryExport> {
    let memory = session_memory(&state)?;
    let store = memory
        .lock()
        .map_err(|_| AppError::Internal("Memory-Mutex vergiftet".to_owned()))?;
    let facts = store
        .active_facts()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(MemoryExport {
        exported_at_unix_ms: now_unix_ms(),
        facts,
    })
}

/// Liefert eine gefilterte Sicht auf das Audit-Log (Konzept 10.2 + UI).
#[tauri::command]
fn list_audit(state: State<'_, AppState>, filter: AuditFilter) -> AppResult<Vec<AuditEntryView>> {
    let session_guard = lock(&state.session)?;
    let session = session_guard.as_ref().ok_or_else(AppError::locked)?;
    let vault = session
        .vault_runtime
        .lock()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let limit = filter.limit.unwrap_or(500);
    let rows = vault
        .repository()
        .audit_entries(Some(limit))
        .map_err(|error| AppError::Internal(error.to_string()))?;
    drop(vault);
    drop(session_guard);
    let filtered: Vec<AuditEntryView> = rows
        .into_iter()
        .filter(|row| filter.actions.is_empty() || filter.actions.iter().any(|a| a == &row.action))
        .filter(|row| {
            filter.outcomes.is_empty() || filter.outcomes.iter().any(|o| o == &row.outcome)
        })
        .filter(|row| match filter.since_unix_ms {
            Some(since) => row.created_unix_ms >= since,
            None => true,
        })
        .filter(|row| match &filter.contains {
            Some(needle) => {
                let haystack = format!("{} {}", row.reason, row.target.as_deref().unwrap_or(""));
                haystack.to_lowercase().contains(&needle.to_lowercase())
            }
            None => true,
        })
        .map(|row| AuditEntryView {
            id: row.id,
            created_unix_ms: row.created_unix_ms,
            mode: row.mode,
            action: row.action,
            target: row.target,
            outcome: row.outcome,
            reason: row.reason,
            hash_hex: hex::encode(&row.hash),
            prev_hash_hex: hex::encode(&row.prev_hash),
        })
        .collect();
    Ok(filtered)
}

/// Listet ein Verzeichnis relativ zum Werkzeug-Workspace (Dateien-Bereich).
///
/// Nutzt bewusst `pa-policy::safe_join` nicht, weil hier reines Lesen im
/// M0-M1-Standardmodus okay ist; ausbrechen kann man nicht, weil der
/// Pfad relativ ist und außerhalb der Workspace-Wurzel abgelehnt wird.
#[tauri::command]
fn list_workspace(
    state: State<'_, AppState>,
    relative_path: String,
) -> AppResult<WorkspaceListing> {
    let workspace_dir = session_workspace(&state)?;
    let root = std::fs::canonicalize(&workspace_dir)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let target = if relative_path.trim().is_empty() || relative_path == "." {
        root.clone()
    } else {
        root.join(&relative_path)
    };
    let canonical = std::fs::canonicalize(&target)
        .map_err(|error| AppError::Invalid(format!("Pfad {relative_path}: {error}")))?;
    if !canonical.starts_with(&root) {
        return Err(AppError::Invalid(
            "Pfad liegt außerhalb des Workspace".to_owned(),
        ));
    }
    let mut entries: Vec<WorkspaceEntry> = std::fs::read_dir(&canonical)
        .map_err(|error| AppError::Internal(error.to_string()))?
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok();
            let relative = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let modified_unix_ms = metadata.as_ref().and_then(|meta| {
                meta.modified().ok().and_then(|time| {
                    time.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|dur| dur.as_millis().min(i64::MAX as u128) as i64)
                })
            });
            WorkspaceEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                relative_path: relative,
                is_directory: metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                bytes: metadata
                    .as_ref()
                    .map(|m| if m.is_file() { m.len() } else { 0 })
                    .unwrap_or(0),
                modified_unix_ms,
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(WorkspaceListing {
        root: root.to_string_lossy().into_owned(),
        relative_path,
        entries,
    })
}

/// Liest bis zu 64 KiB einer Datei; für den Dateien-Bereich der UI reicht das
/// als Vorschau und verhindert, dass die WebView bei sehr großen Dateien
/// hängt. Binärinhalte werden mit `String::from_utf8_lossy` dargestellt.
#[tauri::command]
fn read_workspace_file(state: State<'_, AppState>, relative_path: String) -> AppResult<String> {
    let workspace_dir = session_workspace(&state)?;
    let root = std::fs::canonicalize(&workspace_dir)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let candidate = root.join(&relative_path);
    let canonical = std::fs::canonicalize(&candidate)
        .map_err(|error| AppError::Invalid(format!("Pfad {relative_path}: {error}")))?;
    if !canonical.starts_with(&root) {
        return Err(AppError::Invalid(
            "Pfad liegt außerhalb des Workspace".to_owned(),
        ));
    }
    let bytes = std::fs::read(&canonical)?;
    const MAX_BYTES: usize = 64 * 1024;
    let truncated = &bytes[..bytes.len().min(MAX_BYTES)];
    let mut text = String::from_utf8_lossy(truncated).into_owned();
    if bytes.len() > MAX_BYTES {
        text.push_str("\n… (gekürzt bei 64 KiB Vorschaugrenze)");
    }
    Ok(text)
}

fn require_session(state: &AppState) -> AppResult<SessionHandle> {
    // Lockt kurz, gibt sofort die Handles frei (Arcs sind Clone).
    let guard = lock(&state.session)?;
    let session = guard.as_ref().ok_or_else(AppError::locked)?;
    Ok(SessionHandle {
        vault_runtime: Arc::clone(&session.vault_runtime),
        engine: Arc::clone(&session.engine),
        model_id: session.model_id.clone(),
        tier_override: session.tier_override,
        context_tokens: session.context_tokens,
        kv_quantization: session.kv_quantization,
        vault_path: session.vault_path.clone(),
    })
}

/// Momentaufnahme der wichtigsten Session-Felder für IPC-Kommandos.
struct SessionHandle {
    vault_runtime: Arc<VaultRuntime>,
    engine: Arc<Mutex<LlamaServerEngine>>,
    model_id: String,
    tier_override: Option<HardwareTier>,
    context_tokens: u32,
    kv_quantization: KvQuantization,
    vault_path: PathBuf,
}

impl SessionHandle {
    fn effective_context_tokens(&self) -> u32 {
        self.context_tokens
    }

    fn build_orchestrator(
        &self,
        turn: &library_cmds::TurnContext,
        source_texts: &std::collections::HashMap<u32, String>,
    ) -> ChatOrchestrator {
        let mut prefix = PromptPrefix::empty();
        if let Ok(vault) = self.vault_runtime.lock() {
            let repo = vault.repository();
            let is_enabled = repo
                .setting(SETTING_USER_PROFILE_ENABLED)
                .ok()
                .flatten()
                .map(|v| v == "true")
                .unwrap_or(false);

            let mut identity_parts = Vec::new();
            // Die Projekt-Grundanweisung steht vorne (Spezifikation C5).
            if let Some(project_prompt) = turn.project_prompt.as_deref() {
                identity_parts.push(project_prompt.trim().to_owned());
            }
            if is_enabled {
                if let Ok(Some(custom_sys)) = repo.setting(SETTING_CUSTOM_SYSTEM_PROMPT) {
                    if !custom_sys.trim().is_empty() {
                        identity_parts.push(custom_sys.trim().to_owned());
                    }
                }
            }
            // Antwortsprache gilt unabhängig vom persönlichen Profil.
            if let Some(instruction) = ui_language::response_instruction(ui_language::current()) {
                identity_parts.push(instruction.to_owned());
            }
            if !identity_parts.is_empty() {
                prefix = prefix.with_system_identity(identity_parts.join("\n\n"));
            }

            if is_enabled {
                let mut profile_parts = Vec::new();
                if let Ok(Some(name)) = repo.setting(SETTING_USER_NAME) {
                    if !name.trim().is_empty() {
                        profile_parts.push(format!("Name des Nutzers: {}", name.trim()));
                    }
                }
                if let Ok(Some(about)) = repo.setting(SETTING_USER_ABOUT) {
                    if !about.trim().is_empty() {
                        profile_parts.push(format!("Über den Nutzer: {}", about.trim()));
                    }
                }
                if !profile_parts.is_empty() {
                    prefix = prefix.with_user_profile(profile_parts.join("\n"));
                }
            }
        }
        if let Some(sources) = turn.sources_prompt(source_texts) {
            prefix = prefix.with_variable_contexts([sources]);
        }

        ChatOrchestrator::new(prefix, BudgetPolicy::new(self.effective_context_tokens()))
    }
}

/// Liest das im Tresor hinterlegte Profil und benutzerdefinierte System-Instruktionen aus.
#[tauri::command]
fn get_user_profile(state: State<'_, AppState>) -> AppResult<UserProfile> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    let repo = vault.repository();
    let enabled = repo
        .setting(SETTING_USER_PROFILE_ENABLED)?
        .map(|v| v == "true")
        .unwrap_or(false);
    let user_name = repo.setting(SETTING_USER_NAME)?.unwrap_or_default();
    let user_about = repo.setting(SETTING_USER_ABOUT)?.unwrap_or_default();
    let custom_system_prompt = repo
        .setting(SETTING_CUSTOM_SYSTEM_PROMPT)?
        .unwrap_or_default();
    Ok(UserProfile {
        enabled,
        user_name,
        user_about,
        custom_system_prompt,
    })
}

/// Aktualisiert das Nutzerprofil und synchronisiert die Änderung sofort persistent auf das Medium.
#[tauri::command]
fn update_user_profile(state: State<'_, AppState>, profile: UserProfile) -> AppResult<UserProfile> {
    let session = require_session(&state)?;
    {
        let mut vault = session.vault_runtime.lock()?;
        let repo = vault.repository_mut();
        repo.set_setting(
            SETTING_USER_PROFILE_ENABLED,
            if profile.enabled { "true" } else { "false" },
        )?;
        repo.set_setting(SETTING_USER_NAME, &profile.user_name)?;
        repo.set_setting(SETTING_USER_ABOUT, &profile.user_about)?;
        repo.set_setting(SETTING_CUSTOM_SYSTEM_PROMPT, &profile.custom_system_prompt)?;
        let mut no_fault = pa_vault::hot_copy::NoFault;
        let _ = vault.sync(&mut no_fault);
    }
    Ok(profile)
}

/// Liest einen Oberflächenwert aus dem Vault. Die Oberfläche speichert dort
/// Zustand, der eine Sitzung überdauern soll (Effekt-Stufe, Aufgaben), statt
/// ihn im Profil des Host-Rechners abzulegen.
#[tauri::command]
fn load_ui_state(state: State<'_, AppState>, key: String) -> AppResult<Option<String>> {
    let storage_key = ui_state::storage_key(&key).map_err(AppError::Invalid)?;
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(vault.repository().setting(&storage_key)?)
}

/// Schreibt einen Oberflächenwert in den Vault und synchronisiert ihn sofort
/// auf den Stick, damit ein Abziehen des Mediums nichts verliert.
#[tauri::command]
fn save_ui_state(state: State<'_, AppState>, key: String, value: String) -> AppResult<()> {
    let storage_key = ui_state::storage_key(&key).map_err(AppError::Invalid)?;
    ui_state::check_value(&value).map_err(AppError::Invalid)?;
    let session = require_session(&state)?;
    let mut vault = session.vault_runtime.lock()?;
    vault.repository_mut().set_setting(&storage_key, &value)?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

/// Liefert die Sprache der Oberfläche. Funktioniert schon vor dem Entsperren,
/// damit der Entsperr-Bildschirm in der gewählten Sprache erscheint.
#[tauri::command]
fn get_ui_language(state: State<'_, AppState>) -> AppResult<String> {
    let package_root = lock(&state.package_root)?.clone();
    let code = ui_language::read(&package_root);
    ui_language::set_current(code);
    Ok(code.to_owned())
}

// -------------------------------------------------------------
// Spurlos beenden (Feature 4)
// -------------------------------------------------------------

/// Kennung der App; unter diesem Namen legten ältere Versionen ihre
/// Browserdaten in `%LOCALAPPDATA%` ab.
const WEBVIEW_IDENTIFIER: &str = "at.iap.desktop";

fn trace_locations() -> Vec<host_traces::TraceLocation> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    host_traces::known_locations(local.as_deref(), &std::env::temp_dir(), WEBVIEW_IDENTIFIER)
}

/// Ob beim Beenden nach den Spuren gefragt wird. Standard ist „ja“, weil ein
/// Stick meist auf fremden Rechnern läuft; abwählbar je PC.
fn ask_on_exit(state: &AppState) -> AppResult<bool> {
    let bootstrap = ensure_bootstrap(state)?;
    let session = require_session(state)?;
    let vault = session.vault_runtime.lock()?;
    let value = vault
        .repository()
        .setting(&host_traces::ask_setting_key(&bootstrap.host_identifier))?;
    Ok(value.as_deref() != Some("false"))
}

/// Listet, was IAP auf diesem PC hinterlässt.
#[tauri::command]
fn host_traces(state: State<'_, AppState>) -> AppResult<pa_types::ipc::HostTraceReport> {
    let traces = host_traces::collect(&trace_locations());
    let total_bytes = traces.iter().map(|trace| trace.size_bytes).sum();
    let ask_on_exit = ask_on_exit(&state).unwrap_or(true);
    Ok(pa_types::ipc::HostTraceReport {
        traces,
        total_bytes,
        ask_on_exit,
    })
}

/// Merkt sich im Vault, ob auf diesem PC beim Beenden gefragt wird.
#[tauri::command]
fn set_ask_on_exit(state: State<'_, AppState>, ask: bool) -> AppResult<()> {
    let bootstrap = ensure_bootstrap(&state)?;
    let session = require_session(&state)?;
    let mut vault = session.vault_runtime.lock()?;
    vault.repository_mut().set_setting(
        &host_traces::ask_setting_key(&bootstrap.host_identifier),
        if ask { "true" } else { "false" },
    )?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

/// Die Nachfrage wurde abgebrochen; das nächste Schließen fragt wieder.
#[tauri::command]
fn cancel_quit(state: State<'_, AppState>) {
    state.close_prompted.store(false, Ordering::SeqCst);
}

/// Beendet IAP geordnet. Mit `purge` werden danach die Spuren auf dem PC
/// entfernt. Scheitert das Entfernen, bleibt die App offen und meldet den Grund,
/// damit niemand fälschlich glaubt, der PC sei sauber.
#[tauri::command]
async fn quit_app(app: AppHandle, purge: bool) -> AppResult<()> {
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        shutdown_for_exit(&worker.state::<AppState>(), purge)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    app.exit(0);
    Ok(())
}

/// Stoppt das Modell, schreibt den Tresor zurück und entfernt auf Wunsch die
/// Spuren. Mehrfachaufrufe sind harmlos: die Session wird nur einmal entnommen.
fn shutdown_for_exit(state: &AppState, purge: bool) -> AppResult<()> {
    state.quitting.store(true, Ordering::SeqCst);
    lifecycle::stop_all_work(state);
    let session = lock(&state.session)?.take();

    // Löschziele vor dem Schließen des Tresors prüfen, damit jede Entscheidung
    // von pa-policy noch im Audit-Log landet.
    let mut targets = Vec::new();
    if purge {
        if let Some(session) = session.as_ref() {
            let shared = session
                .vault_runtime
                .shared()
                .map_err(|error| AppError::Internal(error.to_string()))?;
            let mut audit = VaultAuditSink::new(shared);
            for location in trace_locations() {
                if !location.full_path().exists() {
                    continue;
                }
                let scope = pa_policy::PathScope::new(&location.scope)
                    .map_err(|error| AppError::Internal(error.to_string()))?;
                let path = checked_policy_path(
                    &scope,
                    &location.relative,
                    pa_policy::CapabilityAction::FileWrite,
                    pa_policy::Mode::M1Workspace,
                    "Spuren auf diesem PC entfernen",
                    &mut audit,
                )?;
                targets.push(path);
            }
        }
    }

    if let Some(session) = session {
        close_session(session);
    }

    let mut failed = Vec::new();
    for target in targets {
        if let Err(error) = std::fs::remove_dir_all(&target) {
            failed.push(format!("{}: {error}", target.display()));
            continue;
        }
        // Leeren Elternordner `%LOCALAPPDATA%\IAP` mitnehmen; remove_dir
        // löscht nur leere Ordner und kann damit nichts Fremdes treffen.
        if let Some(parent) = target
            .parent()
            .filter(|parent| parent.file_name().is_some_and(|name| name == "IAP"))
        {
            let _ = std::fs::remove_dir(parent);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        state.quitting.store(false, Ordering::SeqCst);
        Err(AppError::Invalid(format!(
            "Einige Spuren konnten nicht entfernt werden: {}",
            failed.join("; ")
        )))
    }
}

/// Löst die Session in ihre Teile auf: erst das Modell stoppen, dann alle
/// weiteren Halter des Tresors freigeben, damit der Tresor sauber zurück-
/// geschrieben und seine Arbeitskopie sicher gelöscht werden kann.
fn close_session(session: Session) {
    let Session {
        engine,
        vault_runtime,
        tool_runtime,
        memory,
        ..
    } = session;
    if let Ok(mut engine) = engine.lock() {
        let _ = engine.stop();
    }
    drop(engine);
    drop(tool_runtime);
    drop(memory);
    match Arc::try_unwrap(vault_runtime) {
        Ok(runtime) => {
            let _ = runtime.shutdown();
        }
        Err(shared) => {
            // Ein Hintergrundauftrag hält den Tresor noch: wenigstens zurückschreiben.
            if let Ok(mut vault) = shared.lock() {
                let mut no_fault = pa_vault::hot_copy::NoFault;
                let _ = vault.sync(&mut no_fault);
            }
        }
    }
}

// -------------------------------------------------------------
// Modelle vergleichen (Feature 7, optional)
// -------------------------------------------------------------

/// Antwort eines anderen Modells auf dieselbe Frage.
#[derive(Debug, Clone, Serialize)]
struct CompareAnswer {
    model_id: String,
    model_name: String,
    answer: String,
    tokens_per_second: Option<f64>,
}

/// Beantwortet eine Frage mit einem anderen installierten Modell und wechselt
/// danach zurück. Es ist immer nur ein Modell im Speicher (8-GB-Rechner); die
/// Vergleichsantwort wird nicht gespeichert, sie ist ein flüchtiger Blick daneben.
/// Der Rückwechsel läuft auch dann, wenn die Vergleichsantwort scheitert.
#[tauri::command]
async fn compare_answer(
    app: AppHandle,
    question: String,
    model_id: String,
    thinking_level: pa_types::chat::ThinkingLevel,
) -> AppResult<CompareAnswer> {
    if question.trim().is_empty() {
        return Err(AppError::Invalid(
            "Es gibt keine Frage zum Vergleichen.".to_owned(),
        ));
    }
    let original_id = {
        let state = app.state::<AppState>();
        require_session(&state)?.model_id
    };
    if original_id == model_id {
        return Err(AppError::Invalid(
            "Bitte ein anderes Modell wählen.".to_owned(),
        ));
    }
    let model_name = {
        let state = app.state::<AppState>();
        installed_models_list(&state)?
            .into_iter()
            .find(|model| model.id == model_id)
            .map(|model| model.display_name)
            .ok_or_else(|| AppError::Invalid("Dieses Modell ist nicht installiert.".to_owned()))?
    };

    select_model(app.clone(), model_id.clone()).await?;
    let worker = app.clone();
    let answer =
        tauri::async_runtime::spawn_blocking(move || -> AppResult<(String, Option<f64>)> {
            let state = worker.state::<AppState>();
            let session = require_session(&state)?;
            let mut messages = Vec::new();
            if let Some(instruction) = ui_language::response_instruction(ui_language::current()) {
                messages.push(pa_types::chat::Message {
                    id: "compare-language".to_owned(),
                    conversation_id: String::new(),
                    position: 0,
                    role: pa_types::chat::MessageRole::System,
                    content: instruction.to_owned(),
                    status: pa_types::chat::MessageStatus::Complete,
                    created_at_unix_ms: now_unix_ms(),
                });
            }
            messages.push(pa_types::chat::Message {
                id: "compare-question".to_owned(),
                conversation_id: String::new(),
                position: 1,
                role: pa_types::chat::MessageRole::User,
                content: question,
                status: pa_types::chat::MessageStatus::Complete,
                created_at_unix_ms: now_unix_ms(),
            });
            let mut engine = lock(&session.engine)?;
            engine.set_thinking_level(thinking_level);
            let started = std::time::Instant::now();
            let mut should_continue = || started.elapsed() < Duration::from_secs(300);
            let mut on_delta = |_: &str| true;
            let outcome = engine
                .stream_chat(&messages, &mut should_continue, &mut on_delta)
                .map_err(|error| {
                    AppError::Launcher(format!(
                        "Die Vergleichsantwort ist fehlgeschlagen: {}",
                        error.message
                    ))
                })?;
            Ok((outcome.text, outcome.timings.predicted_per_second))
        })
        .await
        .map_err(|error| AppError::Internal(error.to_string()));

    // Rückwechsel immer versuchen; ein Fehler dabei hat Vorrang vor dem Ergebnis,
    // damit niemand unbemerkt mit dem falschen Modell weiterarbeitet.
    select_model(app, original_id).await?;
    let (answer, tokens_per_second) = answer??;
    Ok(CompareAnswer {
        model_id,
        model_name,
        answer,
        tokens_per_second,
    })
}

// -------------------------------------------------------------
// Leistungs-Check (Feature 3)
// -------------------------------------------------------------

/// Letztes Messergebnis dieses PCs; `None`, wenn hier noch nie gemessen wurde.
#[tauri::command]
fn performance_report(state: State<'_, AppState>) -> AppResult<Option<perf::PerfReport>> {
    let bootstrap = ensure_bootstrap(&state)?;
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    let raw = vault
        .repository()
        .setting(&perf::setting_key(&bootstrap.host_identifier))?;
    Ok(raw.and_then(|text| serde_json::from_str(&text).ok()))
}

/// Misst Geschwindigkeit und Speicherbedarf des aktiven Modells auf diesem PC.
/// Läuft im Hintergrund, weil die Probeantwort einige Sekunden dauert.
#[tauri::command]
async fn run_performance_check(app: AppHandle) -> AppResult<perf::PerfReport> {
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        run_performance_check_inner(&worker.state::<AppState>())
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

fn run_performance_check_inner(state: &AppState) -> AppResult<perf::PerfReport> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

    let bootstrap = ensure_bootstrap(state)?;
    let session = require_session(state)?;
    let model_start_ms = *lock(&state.model_start_ms)?;
    let models = installed_models_list(state).unwrap_or_default();

    // Speicher-Wächter: misst alle 200 ms den eigenen llama-server (Pfad im Paket).
    let stop = Arc::new(AtomicBool::new(false));
    let sampler_stop = Arc::clone(&stop);
    let package_root = bootstrap.package_root.clone();
    let sampler = thread::spawn(move || {
        let mut system = System::new();
        let mut peak = 0u64;
        while !sampler_stop.load(Ordering::SeqCst) {
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::everything(),
            );
            for process in system.processes().values() {
                let ours = process.name().to_string_lossy().starts_with("llama-server")
                    && process
                        .exe()
                        .is_some_and(|exe| exe.starts_with(&package_root));
                if ours {
                    peak = peak.max(process.memory());
                }
            }
            thread::sleep(Duration::from_millis(200));
        }
        peak
    });

    // Feste, kurze Probeaufgabe ohne Denkphase, damit Messungen vergleichbar sind.
    let probe = pa_types::chat::Message {
        id: "perf-probe".to_owned(),
        conversation_id: String::new(),
        position: 0,
        role: pa_types::chat::MessageRole::User,
        content: "Zähle von 1 bis 40, durch Kommas getrennt. Antworte nur mit den Zahlen."
            .to_owned(),
        status: pa_types::chat::MessageStatus::Complete,
        created_at_unix_ms: now_unix_ms(),
    };
    let outcome = {
        let mut engine = lock(&session.engine)?;
        engine.set_thinking_level(pa_types::chat::ThinkingLevel::Kurz);
        let started = std::time::Instant::now();
        let mut should_continue = || started.elapsed() < Duration::from_secs(90);
        let mut on_delta = |_: &str| true;
        engine.stream_chat(&[probe], &mut should_continue, &mut on_delta)
    };
    stop.store(true, Ordering::SeqCst);
    let ram_peak = sampler.join().unwrap_or(0);

    let timings = outcome
        .map_err(|error| {
            AppError::Launcher(format!(
                "Die Probeantwort ist fehlgeschlagen: {}",
                error.message
            ))
        })?
        .timings;
    let current = models.iter().find(|model| model.id == session.model_id);
    let sizes: Vec<(String, u64)> = models
        .iter()
        .map(|model| (model.id.clone(), model.gguf_bytes))
        .collect();
    let report = perf::PerfReport {
        measured_at_unix_ms: now_unix_ms(),
        model_id: session.model_id.clone(),
        model_name: current
            .map(|model| model.display_name.clone())
            .unwrap_or_else(|| session.model_id.clone()),
        model_start_ms,
        prompt_per_second: timings.prompt_per_second,
        tokens_per_second: timings.predicted_per_second,
        ram_peak_bytes: (ram_peak > 0).then_some(ram_peak),
        total_ram_bytes: bootstrap.hardware.total_ram_bytes,
        recommended_thinking: perf::recommend_thinking(timings.predicted_per_second)
            .map(str::to_owned),
        recommended_model_id: perf::recommend_model(
            timings.predicted_per_second,
            &session.model_id,
            &sizes,
        ),
    };
    let text =
        serde_json::to_string(&report).map_err(|error| AppError::Internal(error.to_string()))?;
    let mut vault = session.vault_runtime.lock()?;
    vault
        .repository_mut()
        .set_setting(&perf::setting_key(&bootstrap.host_identifier), &text)?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(report)
}

/// Speichert die Sprache auf dem Stick und stellt die Modellantworten sofort um.
#[tauri::command]
fn set_ui_language(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    code: String,
) -> AppResult<String> {
    let package_root = lock(&state.package_root)?.clone();
    let code = ui_language::write(&package_root, &code).map_err(AppError::Invalid)?;
    ui_language::set_current(code);
    // Das Pet ist ein eigenes Fenster mit eigenem Sprachzustand und das Tray-Symbol
    // ein nativer Eintrag; beide erfahren die Änderung nur über dieses Ereignis.
    if let Err(error) = tray::relabel(&app, code) {
        eprintln!("Tray-Beschriftung nicht aktualisiert: {error}");
    }
    if let Err(error) = tauri::Emitter::emit(&app, "ui-language-changed", code) {
        eprintln!("Sprachwechsel nicht gemeldet: {error}");
    }
    Ok(code.to_owned())
}

fn read_theme(session: &SessionHandle) -> AppResult<Option<ThemePreference>> {
    let vault = session.vault_runtime.lock()?;
    match vault.repository().setting(SETTING_THEME)? {
        Some(raw) => {
            let theme = match raw.as_str() {
                "dark" => ThemePreference::Dark,
                "light" => ThemePreference::Light,
                _ => ThemePreference::System,
            };
            Ok(Some(theme))
        }
        None => Ok(None),
    }
}

fn persist_theme(session: &Session, theme: ThemePreference) -> AppResult<()> {
    let value = match theme {
        ThemePreference::Dark => "dark",
        ThemePreference::Light => "light",
        ThemePreference::System => "system",
    };
    let mut vault = session.vault_runtime.lock()?;
    vault.repository_mut().set_setting(SETTING_THEME, value)?;
    Ok(())
}

fn load_or_create_meta(
    vault_path: &Path,
    meta_path: &Path,
    create_if_missing: bool,
) -> AppResult<VaultMeta> {
    if meta_path.exists() {
        return Ok(VaultMeta::load(meta_path)?);
    }
    if vault_path.exists() {
        return Err(AppError::Invalid(
            "Zu einem vorhandenen Vault fehlt die vault.meta-Datei".to_owned(),
        ));
    }
    if !create_if_missing {
        return Err(AppError::Invalid(
            "Vault existiert nicht; setze create_if_missing=true, um ihn neu anzulegen".to_owned(),
        ));
    }
    if let Some(parent) = vault_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let meta = VaultMeta::random(Argon2Parameters::default());
    meta.save_atomic(meta_path)?;
    Ok(meta)
}

fn hot_vault_directory(vault: &Path, host_identifier: &str) -> std::io::Result<PathBuf> {
    let absolute = if vault.is_absolute() {
        vault.to_path_buf()
    } else {
        std::env::current_dir()?.join(vault)
    };
    let vault_id = hex::encode(Sha256::digest(absolute.to_string_lossy().as_bytes()));
    let host_prefix = &host_identifier[..16.min(host_identifier.len())];
    let vault_prefix = &vault_id[..16.min(vault_id.len())];
    let new_dir = std::env::temp_dir()
        .join("PortableAI")
        .join("vault-hot")
        .join(format!("{host_prefix}_{vault_prefix}"));

    let legacy_dir = std::env::temp_dir()
        .join("PortableAI")
        .join("vault-hot")
        .join(host_identifier)
        .join(&vault_id);
    if legacy_dir.exists() && !new_dir.exists() {
        let _ = std::fs::create_dir_all(&new_dir);
        let legacy_hot = legacy_dir.join("hot.db");
        if legacy_hot.exists() {
            let _ = std::fs::copy(&legacy_hot, new_dir.join("hot.db"));
        }
        let _ = std::fs::remove_dir_all(&legacy_dir);
    }

    Ok(new_dir)
}

fn server_configuration(
    executable: PathBuf,
    model: PathBuf,
    model_alias: &str,
    plan: &ResourcePlan,
) -> AppResult<(ServerConfig, LoopbackEndpoint)> {
    let (port, token) = ServerConfig::random_endpoint()
        .map_err(|error| AppError::Launcher(format!("Loopback-Port nicht verfügbar: {error}")))?;
    let config = ServerConfig {
        executable,
        model,
        model_alias: model_alias.to_owned(),
        port,
        api_key: token.clone(),
        context_tokens: plan.context_tokens,
        threads: plan.threads,
        gpu_layers: 0,
        kv_quantization: plan.kv_quantization,
        mmproj: None,
    };
    let endpoint = LoopbackEndpoint::new(port, token)
        .map_err(|error| AppError::Launcher(format!("Loopback-Endpoint ungültig: {error}")))?;
    Ok((config, endpoint))
}

// ============================================================================
// Rubrik-Engine (Konzept 4.2)
// ============================================================================

const QUESTIONNAIRE_SETTING_KEY: &str = "rubric.questionnaire.v1";

/// Ergebnis von [`save_questionnaire`]: welche Pflichtfelder noch fehlen.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SaveQuestionnaireResult {
    pub missing_fields: Vec<String>,
    pub saved_unix_ms: i64,
}

/// Anfrage-DTO für [`run_rubric_monte_carlo`].
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RunMonteCarloRequest {
    pub input: pa_tools::rubric::MonteCarloInput,
    #[serde(default = "default_mc_runs")]
    pub runs: u32,
    #[serde(default = "default_mc_seed")]
    pub seed: u64,
}

fn default_mc_runs() -> u32 {
    10_000
}
fn default_mc_seed() -> u64 {
    // Deterministische Seed-Default, damit dieselben Eingaben immer dieselben
    // P10/P50/P90 liefern (Konzept 4.2: „reproduzierbare Verteilung statt
    // erfundener Einzelzahl").
    0x50_6f_72_74_41_49_00_01
}

/// Liefert die im MVP eingebaute Business-Rubrik.
#[tauri::command]
fn rubric_default() -> AppResult<pa_tools::rubric::Rubric> {
    let rubric = pa_tools::rubric::Rubric::business_idea_default();
    rubric
        .validate()
        .map_err(|error| AppError::Internal(format!("Rubrik ungültig: {error}")))?;
    Ok(rubric)
}

/// Speichert den Fragebogen im Vault (`settings.rubric.questionnaire.v1`) und
/// meldet fehlende Pflichtfelder zurück, damit die UI Nutzer gezielt fragt.
#[tauri::command]
fn save_questionnaire(
    state: State<'_, AppState>,
    responses: pa_tools::rubric::QuestionnaireResponses,
) -> AppResult<SaveQuestionnaireResult> {
    let missing = responses
        .missing_fields()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let payload =
        serde_json::to_string(&responses).map_err(|error| AppError::Internal(error.to_string()))?;
    let session_guard = lock(&state.session)?;
    let session = session_guard.as_ref().ok_or_else(AppError::locked)?;
    let vault_runtime = Arc::clone(&session.vault_runtime);
    drop(session_guard);
    {
        let mut vault = vault_runtime
            .lock()
            .map_err(|_| AppError::Internal("Vault-Mutex vergiftet".to_owned()))?;
        vault
            .repository_mut()
            .set_setting(QUESTIONNAIRE_SETTING_KEY, &payload)?;
    }
    Ok(SaveQuestionnaireResult {
        missing_fields: missing,
        saved_unix_ms: now_unix_ms(),
    })
}

/// Lädt den gespeicherten Fragebogen; leer, wenn noch nichts persistiert.
#[tauri::command]
fn load_questionnaire(
    state: State<'_, AppState>,
) -> AppResult<pa_tools::rubric::QuestionnaireResponses> {
    let session_guard = lock(&state.session)?;
    let session = session_guard.as_ref().ok_or_else(AppError::locked)?;
    let vault_runtime = Arc::clone(&session.vault_runtime);
    drop(session_guard);
    let vault = vault_runtime
        .lock()
        .map_err(|_| AppError::Internal("Vault-Mutex vergiftet".to_owned()))?;
    let raw = vault.repository().setting(QUESTIONNAIRE_SETTING_KEY)?;
    match raw {
        Some(json) => serde_json::from_str(&json).map_err(|error| {
            AppError::Internal(format!("gespeicherter Fragebogen defekt: {error}"))
        }),
        None => Ok(pa_tools::rubric::QuestionnaireResponses::default()),
    }
}

/// Nimmt ein LLM-erzeugtes Score-Sheet, validiert es gegen die Standardrubrik
/// und liefert das gewichtete Ergebnis samt Beiträgen (Konzept 4.2 Stufe 3).
#[tauri::command]
fn evaluate_rubric(
    sheet: pa_tools::rubric::RubricScoreSheet,
) -> AppResult<pa_tools::rubric::WeightedResult> {
    let rubric = pa_tools::rubric::Rubric::business_idea_default();
    rubric
        .validate()
        .map_err(|error| AppError::Internal(format!("Rubrik ungültig: {error}")))?;
    sheet.validate(&rubric).map_err(AppError::Invalid)?;
    Ok(pa_tools::rubric::WeightedResult::from(&rubric, &sheet))
}

/// Führt die Monte-Carlo-Simulation deterministisch aus. `seed` und `runs`
/// haben Defaults; der Aufrufer kann sie explizit setzen.
#[tauri::command]
fn run_rubric_monte_carlo(
    request: RunMonteCarloRequest,
) -> AppResult<pa_tools::rubric::MonteCarloResult> {
    if request.runs == 0 {
        return Err(AppError::Invalid("runs muss größer als 0 sein".to_owned()));
    }
    Ok(pa_tools::rubric::run_monte_carlo(
        &request.input,
        request.runs,
        request.seed,
    ))
}

fn random_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    format!("{prefix}-{}", hex::encode(bytes))
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(i64::MAX as u128) as i64
        })
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> AppResult<std::sync::MutexGuard<'_, T>> {
    mutex
        .lock()
        .map_err(|_| AppError::Internal("Mutex vergiftet".to_owned()))
}

struct NoopProgress;

impl CopyObserver for NoopProgress {
    fn copied(&mut self, _bytes: u64, _total: u64) -> Result<(), LauncherError> {
        Ok(())
    }
}

// ============================================================================
// Werkzeug-Freigabe-Rendezvous (Konzept 10.3, Meilenstein 8)
// ============================================================================

/// Sicht auf einen wartenden Freigabedialog (ohne den `Sender`).
#[derive(Debug, Clone, Serialize)]
pub struct PendingPermissionView {
    pub pending_id: String,
    pub tool: String,
    pub arguments_json: String,
    pub reason: String,
    pub created_unix_ms: i64,
}

/// Timeout, nach dem eine unbeantwortete Freigabe automatisch als Deny gilt.
/// Wählt bewusst großzügig: die UI muss den Nutzer echt fragen können, aber
/// ein vergessener Dialog blockiert keinen Turn dauerhaft.
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(300);

/// Hook-Adapter, den `pa_launcher::tool_runtime` synchron aufruft, wenn die
/// Policy `Decision::Prompt` liefert. Emittiert `PermissionRequested` in den
/// Tauri-Event-Bus und wartet blockierend auf die Antwort aus
/// `respond_permission`.
struct TauriPermissionHook {
    app: AppHandle,
    pending_store: Arc<Mutex<std::collections::HashMap<String, PendingPermission>>>,
    assistant_message_id: String,
}

impl pa_tools::PermissionHook for TauriPermissionHook {
    fn ask(
        &mut self,
        tool: &str,
        request: &pa_policy::CapabilityRequest,
        reason: &str,
    ) -> Result<pa_tools::PermissionAnswer, pa_tools::ToolError> {
        let pending_id = random_id("perm");
        let arguments_json = serde_json::to_string(request).unwrap_or_else(|_| "{}".to_owned());
        let (sender, receiver) = std::sync::mpsc::channel::<PermissionOutcome>();
        {
            let mut pending =
                self.pending_store
                    .lock()
                    .map_err(|_| pa_tools::ToolError::Denied {
                        tool: tool.to_owned(),
                        reason: "Pending-Permissions-Mutex vergiftet".to_owned(),
                    })?;
            pending.insert(
                pending_id.clone(),
                PendingPermission {
                    tool: tool.to_owned(),
                    arguments_json: arguments_json.clone(),
                    reason: reason.to_owned(),
                    created_unix_ms: now_unix_ms(),
                    responder: sender,
                },
            );
        }
        let _ = self.app.emit(
            "tool-stream",
            pa_types::ipc::ToolStreamEvent::PermissionRequested {
                pending_id: pending_id.clone(),
                assistant_message_id: self.assistant_message_id.clone(),
                tool: tool.to_owned(),
                arguments_json,
                reason: reason.to_owned(),
            },
        );
        match receiver.recv_timeout(PERMISSION_TIMEOUT) {
            Ok(outcome) => Ok(pa_tools::PermissionAnswer {
                allow: outcome.allow,
                remember_for_session: outcome.remember_for_session,
            }),
            Err(_) => {
                // Timeout: Store bereinigen, damit ein späterer respond_permission
                // nicht auf einen toten Sender zeigt.
                if let Ok(mut pending) = self.pending_store.lock() {
                    pending.remove(&pending_id);
                }
                Ok(pa_tools::PermissionAnswer {
                    allow: false,
                    remember_for_session: false,
                })
            }
        }
    }
}

/// Beantwortet einen ausstehenden Freigabedialog. Wenn der Runner bereits
/// weitergelaufen ist (Timeout/Session-Ende), landet die Antwort im Leeren
/// und der Command meldet ehrlich, dass keine passende Anfrage mehr existiert.
#[tauri::command]
fn respond_permission(
    state: State<'_, AppState>,
    response: pa_types::ipc::ToolPromptResponse,
) -> AppResult<bool> {
    let mut pending = state
        .pending_permissions
        .lock()
        .map_err(|_| AppError::Internal("Pending-Permissions-Mutex vergiftet".to_owned()))?;
    match pending.remove(&response.pending_id) {
        Some(entry) => {
            let outcome = PermissionOutcome {
                allow: response.allow,
                remember_for_session: response.remember_for_session,
            };
            // Der Empfänger könnte bereits weg sein (Kanal geschlossen); dann
            // war die Freigabe hinfällig — für den Nutzer trotzdem quittieren.
            let _ = entry.responder.send(outcome);
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Liste aller aktuell wartenden Freigabedialoge.
#[tauri::command]
fn pending_permissions(state: State<'_, AppState>) -> AppResult<Vec<PendingPermissionView>> {
    let pending = state
        .pending_permissions
        .lock()
        .map_err(|_| AppError::Internal("Pending-Permissions-Mutex vergiftet".to_owned()))?;
    let mut view: Vec<PendingPermissionView> = pending
        .iter()
        .map(|(pending_id, entry)| PendingPermissionView {
            pending_id: pending_id.clone(),
            tool: entry.tool.clone(),
            arguments_json: entry.arguments_json.clone(),
            reason: entry.reason.clone(),
            created_unix_ms: entry.created_unix_ms,
        })
        .collect();
    view.sort_by_key(|entry| entry.created_unix_ms);
    Ok(view)
}

// ============================================================================
// Code-Bereich (Konzept 9): Editor, Diff-Ansicht, Snapshot, Git
// ============================================================================

const SNAPSHOT_SUBDIR: &str = ".snapshots";

/// Snapshot-Kopf für die UI: dieselben Felder wie `pa_code::Snapshot`, aber
/// Pfade als String für Serialisierung.
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotView {
    pub id: String,
    pub source_root: String,
    pub snapshot_root: String,
    pub kind: pa_code::SnapshotKind,
    pub file_count: u32,
    pub bytes_referenced: u64,
    pub created_unix_ms: i64,
}

impl From<pa_code::Snapshot> for SnapshotView {
    fn from(snapshot: pa_code::Snapshot) -> Self {
        Self {
            id: snapshot.id,
            source_root: snapshot.source_root.display().to_string(),
            snapshot_root: snapshot.snapshot_root.display().to_string(),
            kind: snapshot.kind,
            file_count: snapshot.file_count,
            bytes_referenced: snapshot.bytes_referenced,
            created_unix_ms: snapshot.created_unix_ms,
        }
    }
}

/// Sicherungspunkte gibt es nur im Arbeitsordner auf dem Stick. Im freigegebenen PC-Ordner würde
/// ein `.snapshots`-Ordner eine versteckte Spur im Projekt hinterlassen (Invariante 6).
fn stick_workspace_for_snapshots(state: &AppState) -> AppResult<PathBuf> {
    let active = code_roots::active_root(state)?;
    if active.host {
        return Err(AppError::Invalid(
            "Sicherungspunkte gibt es nur im Arbeitsordner auf dem Stick, nicht in einem PC-Ordner."
                .to_owned(),
        ));
    }
    Ok(active.path)
}

fn snapshot_manager(workspace: &Path) -> AppResult<pa_code::SnapshotManager> {
    pa_code::SnapshotManager::open(workspace.join(SNAPSHOT_SUBDIR))
        .map_err(|error| AppError::Internal(error.to_string()))
}

fn read_snapshot_index(workspace: &Path) -> AppResult<Vec<SnapshotView>> {
    let base = workspace.join(SNAPSHOT_SUBDIR);
    if !base.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in std::fs::read_dir(&base)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        let id = entry.file_name().to_string_lossy().into_owned();
        // Metadaten aus Verzeichnis-Timestamp; Byte-/Datei-Zählung durch Traversierung.
        let created_unix_ms = entry
            .metadata()
            .and_then(|m| m.created().or_else(|_| m.modified()))
            .ok()
            .and_then(|time| {
                time.duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
            })
            .unwrap_or(0);
        let (file_count, bytes) = walk_snapshot(&path);
        result.push(SnapshotView {
            id,
            source_root: workspace.display().to_string(),
            snapshot_root: path.display().to_string(),
            kind: pa_code::SnapshotKind::Copy,
            file_count,
            bytes_referenced: bytes,
            created_unix_ms,
        });
    }
    result.sort_by_key(|a| std::cmp::Reverse(a.created_unix_ms));
    Ok(result)
}

fn walk_snapshot(root: &Path) -> (u32, u64) {
    let mut files = 0_u32;
    let mut bytes = 0_u64;
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    let (sub_files, sub_bytes) = walk_snapshot(&path);
                    files = files.saturating_add(sub_files);
                    bytes = bytes.saturating_add(sub_bytes);
                } else if ft.is_file() {
                    files = files.saturating_add(1);
                    if let Ok(meta) = entry.metadata() {
                        bytes = bytes.saturating_add(meta.len());
                    }
                }
            }
        }
    }
    (files, bytes)
}

/// Legt einen Snapshot des kompletten Workspace-Verzeichnisses an. Die
/// `.snapshots/`-Wurzel selbst wird von der Klon-Traversierung nicht erfasst
/// (siehe `pa_code::snapshot`).
#[tauri::command]
fn snapshot_create(state: State<'_, AppState>) -> AppResult<SnapshotView> {
    let workspace = stick_workspace_for_snapshots(&state)?;
    let manager = snapshot_manager(&workspace)?;
    let snapshot = manager
        .create(&workspace, now_unix_ms())
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(snapshot.into())
}

/// Liste aller vorhandenen Snapshots, neueste zuerst.
#[tauri::command]
fn snapshot_list(state: State<'_, AppState>) -> AppResult<Vec<SnapshotView>> {
    let workspace = stick_workspace_for_snapshots(&state)?;
    read_snapshot_index(&workspace)
}

/// Stellt den Workspace aus einem Snapshot wieder her.
#[tauri::command]
fn snapshot_restore(state: State<'_, AppState>, snapshot_id: String) -> AppResult<()> {
    let workspace = stick_workspace_for_snapshots(&state)?;
    let manager = snapshot_manager(&workspace)?;
    let snapshot_root = workspace.join(SNAPSHOT_SUBDIR).join(&snapshot_id);
    if !snapshot_root.exists() {
        return Err(AppError::Invalid(format!(
            "Snapshot {snapshot_id} existiert nicht"
        )));
    }
    let snapshot = pa_code::Snapshot {
        id: snapshot_id,
        source_root: workspace.clone(),
        snapshot_root,
        kind: pa_code::SnapshotKind::Copy,
        file_count: 0,
        bytes_referenced: 0,
        created_unix_ms: 0,
    };
    manager
        .restore(&snapshot)
        .map_err(|error| AppError::Internal(error.to_string()))
}

/// Löscht einen Snapshot dauerhaft.
#[tauri::command]
fn snapshot_discard(state: State<'_, AppState>, snapshot_id: String) -> AppResult<()> {
    let workspace = stick_workspace_for_snapshots(&state)?;
    let manager = snapshot_manager(&workspace)?;
    let snapshot_root = workspace.join(SNAPSHOT_SUBDIR).join(&snapshot_id);
    let snapshot = pa_code::Snapshot {
        id: snapshot_id,
        source_root: workspace,
        snapshot_root,
        kind: pa_code::SnapshotKind::Copy,
        file_count: 0,
        bytes_referenced: 0,
        created_unix_ms: 0,
    };
    manager
        .discard(&snapshot)
        .map_err(|error| AppError::Internal(error.to_string()))
}

// ============================================================================
// Phase 4: Scheduler / Skills / Update / Export
// ============================================================================

/// Führt den deterministischen Wochen-Scheduler aus (Konzept 12).
#[tauri::command]
fn schedule_plan(request: pa_scheduler::PlanRequest) -> AppResult<pa_scheduler::SolvedPlan> {
    pa_scheduler::schedule(&request).map_err(|error| AppError::Invalid(error.to_string()))
}

/// Parst einen ICS-Text (`.ics`-Dateiinhalt) in Termine + Aufgaben (Konzept 12).
#[tauri::command]
fn import_ics_text(text: String) -> AppResult<pa_scheduler::ics::IcsImport> {
    pa_scheduler::import_ics(&text).map_err(|error| AppError::Invalid(error.to_string()))
}

/// Serialisiert Termine + Aufgaben als ICS-Text.
#[tauri::command]
fn export_ics_text(
    events: Vec<pa_scheduler::Event>,
    tasks: Vec<pa_scheduler::Task>,
) -> AppResult<String> {
    Ok(pa_scheduler::export_ics(&events, &tasks))
}

/// Listet alle installierten WASM-Skills unter `AI/skills/user/` (Konzept 8.2).
#[tauri::command]
fn list_installed_skills(state: State<'_, AppState>) -> AppResult<Vec<InstalledSkillView>> {
    let workspace = session_workspace(&state)?;
    // Skills-Wurzel liegt neben data/workspace: <package_root>/AI/skills
    let bootstrap = ensure_bootstrap(&state)?;
    let skills_root = bootstrap
        .package_root
        .join("AI")
        .join("skills")
        .join("user");
    let registry = pa_skills::SkillRegistry::new(skills_root);
    let items = registry
        .scan()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let _ = workspace; // Workspace-Wurzel wird für den Trockenlauf genutzt.
    let mut views: Vec<InstalledSkillView> = items
        .into_iter()
        .map(|s| InstalledSkillView {
            id: s.manifest.skill.id.clone(),
            name: s.manifest.skill.name.clone(),
            version: s.manifest.skill.version.clone(),
            permissions: s.manifest.permission_summary(),
            tools: s
                .manifest
                .tools
                .iter()
                .map(|t| SkillToolView {
                    name: t.name.clone(),
                    description: t.description.clone(),
                })
                .collect(),
            kind: "wasm".to_owned(),
            description: String::new(),
            active: true,
            body_preview: String::new(),
            files: Vec::new(),
            skipped: Vec::new(),
        })
        .collect();
    views.extend(instruction_skills::list(&state)?);
    Ok(views)
}

/// Leitet jeden Dateizugriff beim Import durch pa-policy und das Vault-Audit.
fn checked_skill_path(
    scope: &pa_policy::PathScope,
    relative: &Path,
    action: pa_policy::CapabilityAction,
    mode: pa_policy::Mode,
    audit: &mut VaultAuditSink,
) -> AppResult<PathBuf> {
    checked_policy_path(
        scope,
        relative,
        action,
        mode,
        "Skill per Drag-and-drop importieren",
        audit,
    )
}

/// Prüft einen vom Nutzer ausgelösten Dateizugriff mit pa-policy, schreibt das
/// Ergebnis ins Vault-Audit und liefert den kanonischen Pfad (Invariante 2 und 3).
fn checked_policy_path(
    scope: &pa_policy::PathScope,
    relative: &Path,
    action: pa_policy::CapabilityAction,
    mode: pa_policy::Mode,
    reason: &str,
    audit: &mut VaultAuditSink,
) -> AppResult<PathBuf> {
    use pa_policy::{
        AuditLog, AuditOutcome, AuditSink, CapabilityRequest, Decision, DerivationSource,
        GrantStore,
    };
    let request = CapabilityRequest {
        action,
        relative_path: Some(relative.to_path_buf()),
        source: DerivationSource::UserIntent,
        reason: reason.to_owned(),
    };
    let decision =
        match pa_policy::capability::evaluate(&request, mode, scope, &GrantStore::default()) {
            Ok(decision) => decision,
            Err(error) => {
                audit
                    .append(
                        AuditLog {
                            mode,
                            action,
                            target: Some(relative.display().to_string()),
                            outcome: AuditOutcome::Deny,
                            reason: error.to_string(),
                        },
                        now_unix_ms(),
                    )
                    .map_err(|audit_error| AppError::Internal(audit_error.to_string()))?;
                return Err(AppError::Invalid(error.to_string()));
            }
        };
    let outcome = match &decision {
        Decision::Allow(_) => AuditOutcome::Allow,
        Decision::Prompt(_) => AuditOutcome::Prompt,
        Decision::Deny(_) => AuditOutcome::Deny,
    };
    audit
        .append(
            AuditLog {
                mode,
                action,
                target: Some(relative.display().to_string()),
                outcome,
                reason: request.reason,
            },
            now_unix_ms(),
        )
        .map_err(|error| AppError::Internal(error.to_string()))?;
    match decision {
        Decision::Allow(capability) => capability
            .canonical_path
            .ok_or_else(|| AppError::Internal("Policy gab keinen Pfad zurück".to_owned())),
        Decision::Prompt(reason) | Decision::Deny(reason) => Err(AppError::Invalid(reason)),
    }
}

/// Installiert einen vom Nutzer abgelegten WASM-Skill nach Manifest- und Policy-Prüfung.
#[tauri::command]
async fn import_skill_from_path(
    state: State<'_, AppState>,
    source_path: String,
) -> AppResult<InstalledSkillView> {
    let package_root = ensure_bootstrap(&state)?.package_root.clone();
    let shared = require_session(&state)?
        .vault_runtime
        .shared()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let root = package_root.clone();
    let view = tauri::async_runtime::spawn_blocking(move || {
        import_skill_from_path_inner(source_path, root, shared)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))??;
    if view.kind == "instructions" {
        // Wer einen Anleitungs-Skill installiert, will ihn nutzen: er startet aktiv.
        instruction_skills::activate_after_import(&state, &view.id)?;
        return Ok(view);
    }
    // Neuen Skill sofort auch dem Modell anbieten, ohne die Sitzung neu zu starten.
    let tool_runtime = lock(&state.session)?
        .as_ref()
        .map(|session| Arc::clone(&session.tool_runtime));
    if let Some(tool_runtime) = tool_runtime {
        let registry =
            pa_skills::SkillRegistry::new(package_root.join("AI").join("skills").join("user"));
        let installed = registry.scan().unwrap_or_default();
        if let Some(skill) = installed
            .iter()
            .find(|skill| skill.manifest.skill.id == view.id)
        {
            let mut runtime = lock(&tool_runtime)?;
            for tool in skill_tools::SkillTool::from_skill(skill) {
                runtime.register_tool(Arc::new(tool));
            }
        }
    }
    Ok(view)
}

/// Findet den eigentlichen Skill-Ordner zu dem, was abgelegt wurde.
///
/// Warum: Wer einen Ordner zieht, meint oft den umgebenden Ordner (der Skill
/// liegt eine Ebene tiefer) oder hat einen Anleitungs-Skill (`SKILL.md`). Beides
/// soll eine verständliche Antwort bekommen statt eines Betriebssystemfehlers.
/// Ob im Ordner eine `SKILL.md` (in beliebiger Groß-/Kleinschreibung) liegt.
fn has_skill_md(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("SKILL.md")
                    && entry.file_type().is_ok_and(|t| t.is_file())
            })
        })
        .unwrap_or(false)
}

fn locate_skill_folder(dropped: PathBuf) -> AppResult<PathBuf> {
    let is_skill = |path: &Path| path.join("manifest.toml").is_file() || has_skill_md(path);
    if is_skill(&dropped) {
        return Ok(dropped);
    }
    // Genau ein Unterordner mit manifest.toml oder SKILL.md: den nehmen.
    let inner: Vec<PathBuf> = std::fs::read_dir(&dropped)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir() && is_skill(path))
                .collect()
        })
        .unwrap_or_default();
    if let [only] = inner.as_slice() {
        return Ok(only.clone());
    }
    if inner.len() > 1 {
        return Err(AppError::Invalid(
            "In diesem Ordner liegen mehrere Skills. Bitte einen einzelnen Skill-Ordner ablegen."
                .to_owned(),
        ));
    }
    Err(AppError::Invalid(
        "In diesem Ordner liegt weder eine SKILL.md noch eine manifest.toml. Ein Skill ist entweder eine Anleitung (SKILL.md) oder ein WASM-Paket (manifest.toml plus .wasm-Datei)."
            .to_owned(),
    ))
}

fn import_skill_from_path_inner(
    source_path: String,
    package_root: PathBuf,
    shared: Arc<Mutex<pa_vault::hot_copy::HotVault>>,
) -> AppResult<InstalledSkillView> {
    if source_path.starts_with("\\\\") {
        return Err(AppError::Invalid(
            "Netzwerkpfade sind im Offline-Modus nicht zulässig".to_owned(),
        ));
    }
    let source = PathBuf::from(source_path);
    let source_dir = if source.is_dir() {
        source.clone()
    } else {
        if !matches!(
            source.file_name().and_then(|name| name.to_str()),
            Some("manifest.toml")
        ) {
            return Err(AppError::Invalid(
                "Bitte einen Skill-Ordner oder dessen manifest.toml ablegen".to_owned(),
            ));
        }
        source
            .parent()
            .ok_or_else(|| AppError::Invalid("Skill-Ordner fehlt".to_owned()))?
            .to_path_buf()
    };
    let source_dir = locate_skill_folder(source_dir)?;
    if !source_dir.join("manifest.toml").is_file() {
        // Anleitungs-Skill: reiner Text, keine Ausführung.
        let mut audit = VaultAuditSink::new(shared);
        let (view, _id) = instruction_skills::import(&source_dir, &package_root, &mut audit)?;
        return Ok(view);
    }
    let source_scope = pa_policy::PathScope::new(&source_dir)
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    let mut audit = VaultAuditSink::new(shared);
    let manifest_path = checked_skill_path(
        &source_scope,
        Path::new("manifest.toml"),
        pa_policy::CapabilityAction::FileRead,
        pa_policy::Mode::M0Observe,
        &mut audit,
    )?;
    let manifest_text = std::fs::read_to_string(manifest_path)?;
    let manifest = pa_skills::Manifest::parse(&manifest_text)
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    let id = &manifest.skill.id;
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(AppError::Invalid(
            "Skill-ID darf nur Buchstaben, Zahlen, '-' und '_' enthalten (maximal 64 Zeichen)"
                .to_owned(),
        ));
    }
    if manifest.skill.runtime != "wasm"
        || manifest.capabilities.network
        || manifest.capabilities.exec
    {
        return Err(AppError::Invalid(
            "Nur lokale WASM-Skills ohne Netzwerk- oder Prozessfreigabe sind zulässig".to_owned(),
        ));
    }
    let entry = Path::new(&manifest.skill.entry);
    if entry.components().count() != 1 || entry.extension().is_none_or(|ext| ext != "wasm") {
        return Err(AppError::Invalid(
            "Der WASM-Eintrag muss eine einzelne .wasm-Datei sein".to_owned(),
        ));
    }
    if !source_dir.join(entry).is_file() {
        return Err(AppError::Invalid(format!(
            "Die Datei {} aus der manifest.toml fehlt im Ordner.",
            entry.display()
        )));
    }
    let wasm_path = checked_skill_path(
        &source_scope,
        entry,
        pa_policy::CapabilityAction::FileRead,
        pa_policy::Mode::M0Observe,
        &mut audit,
    )?;
    if std::fs::metadata(&wasm_path)?.len() > 16 * 1024 * 1024 {
        return Err(AppError::Invalid(
            "Skill-Datei ist größer als 16 MB".to_owned(),
        ));
    }
    let wasm = std::fs::read(wasm_path)?;
    if !wasm.starts_with(b"\0asm\x01\0\0\0") {
        return Err(AppError::Invalid(
            "Die Skill-Datei ist kein gültiges WASM-Modul".to_owned(),
        ));
    }
    let actual = hex::encode(Sha256::digest(&wasm));
    if !actual.eq_ignore_ascii_case(&manifest.skill.sha256) {
        return Err(AppError::Invalid(
            "SHA-256 des WASM-Moduls stimmt nicht mit manifest.toml überein".to_owned(),
        ));
    }
    let package_scope = pa_policy::PathScope::new(&package_root)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let base = PathBuf::from("AI").join("skills").join("user");
    for folder in [
        PathBuf::from("AI"),
        PathBuf::from("AI").join("skills"),
        base.clone(),
    ] {
        let path = checked_skill_path(
            &package_scope,
            &folder,
            pa_policy::CapabilityAction::FileWrite,
            pa_policy::Mode::M1Workspace,
            &mut audit,
        )?;
        std::fs::create_dir_all(path)?;
    }
    let target = checked_skill_path(
        &package_scope,
        &base.join(id),
        pa_policy::CapabilityAction::FileWrite,
        pa_policy::Mode::M1Workspace,
        &mut audit,
    )?;
    if target.exists() {
        return Err(AppError::Invalid(format!(
            "Skill `{id}` ist bereits installiert"
        )));
    }
    let stage_relative = PathBuf::from("AI")
        .join("skills")
        .join(format!(".import-{}", random_id("skill")));
    let stage = checked_skill_path(
        &package_scope,
        &stage_relative,
        pa_policy::CapabilityAction::FileWrite,
        pa_policy::Mode::M1Workspace,
        &mut audit,
    )?;
    std::fs::create_dir(&stage)?;
    let install_result = (|| -> AppResult<()> {
        let stage_manifest = checked_skill_path(
            &package_scope,
            &stage_relative.join("manifest.toml"),
            pa_policy::CapabilityAction::FileWrite,
            pa_policy::Mode::M1Workspace,
            &mut audit,
        )?;
        std::fs::write(stage_manifest, manifest_text.as_bytes())?;
        let stage_wasm = checked_skill_path(
            &package_scope,
            &stage_relative.join(entry),
            pa_policy::CapabilityAction::FileWrite,
            pa_policy::Mode::M1Workspace,
            &mut audit,
        )?;
        std::fs::write(stage_wasm, wasm)?;
        std::fs::rename(&stage, &target)?;
        Ok(())
    })();
    if install_result.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    install_result?;
    let permissions = manifest.permission_summary();
    Ok(InstalledSkillView {
        id: id.clone(),
        name: manifest.skill.name,
        version: manifest.skill.version,
        permissions,
        tools: manifest
            .tools
            .into_iter()
            .map(|tool| SkillToolView {
                name: tool.name,
                description: tool.description,
            })
            .collect(),
        kind: "wasm".to_owned(),
        description: String::new(),
        active: true,
        body_preview: String::new(),
        files: Vec::new(),
        skipped: Vec::new(),
    })
}

/// Trockenlauf eines Skill-Werkzeugs.
#[tauri::command]
fn skill_dry_run(
    state: State<'_, AppState>,
    skill_id: String,
    tool: String,
    arguments_json: String,
) -> AppResult<pa_skills::DryRunOutcome> {
    let bootstrap = ensure_bootstrap(&state)?;
    let skills_root = bootstrap
        .package_root
        .join("AI")
        .join("skills")
        .join("user");
    let registry = pa_skills::SkillRegistry::new(skills_root);
    let items = registry
        .scan()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let skill = items
        .into_iter()
        .find(|s| s.manifest.skill.id == skill_id)
        .ok_or_else(|| AppError::Invalid(format!("Skill `{skill_id}` nicht gefunden")))?;
    let args: std::collections::BTreeMap<String, serde_json::Value> =
        serde_json::from_str(&arguments_json).map_err(|error| {
            AppError::Invalid(format!("arguments_json ist kein Objekt: {error}"))
        })?;
    pa_skills::dry_run(&skill, &tool, args).map_err(|error| AppError::Invalid(error.to_string()))
}

/// Führt ein Skill-Werkzeug wirklich aus (Wasmtime-Sandbox), über denselben
/// Policy- und Audit-Pfad wie ein Werkzeugaufruf des Modells.
#[tauri::command]
async fn run_skill(
    state: State<'_, AppState>,
    skill_id: String,
    tool: String,
    arguments_json: String,
) -> AppResult<String> {
    let bootstrap = ensure_bootstrap(&state)?;
    let session = require_session(&state)?;
    let workspace_dir = lock(&state.session)?
        .as_ref()
        .map(|session| session.workspace_dir.clone())
        .ok_or_else(AppError::locked)?;
    let shared = session
        .vault_runtime
        .shared()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let package_root = bootstrap.package_root.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<String> {
        use pa_tools::Tool;
        let arguments: std::collections::BTreeMap<String, serde_json::Value> =
            serde_json::from_str(&arguments_json).map_err(|error| {
                AppError::Invalid(format!("Die Eingabe ist kein JSON-Objekt: {error}"))
            })?;
        let name = skill_tools::SkillTool::tool_name(&skill_id, &tool);
        let registry =
            pa_skills::SkillRegistry::new(package_root.join("AI").join("skills").join("user"));
        let skill = registry
            .scan()
            .map_err(|error| AppError::Invalid(error.to_string()))?
            .into_iter()
            .find(|skill| skill.manifest.skill.id == skill_id)
            .ok_or_else(|| AppError::Invalid(format!("Skill `{skill_id}` nicht gefunden")))?;
        let skill_tool = skill_tools::SkillTool::from_skill(&skill)
            .into_iter()
            .find(|candidate| candidate.spec().name == name)
            .ok_or_else(|| {
                AppError::Invalid(format!("Werkzeug `{tool}` ist nicht Teil des Skills"))
            })?;
        let workspace = pa_policy::PathScope::new(&workspace_dir)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        let grants = pa_policy::GrantStore::default();
        let mut audit = VaultAuditSink::new(shared);
        let mut context = pa_tools::ToolContext {
            workspace: &workspace,
            mode: pa_policy::Mode::M0Observe,
            grants: &grants,
            audit: &mut audit,
            now_unix_ms: now_unix_ms(),
            permission_hook: None,
        };
        let invocation = pa_tools::ToolInvocation {
            name,
            arguments,
            source: pa_policy::DerivationSource::UserIntent,
        };
        let output = skill_tool
            .invoke(&invocation, &mut context)
            .map_err(|error| AppError::Invalid(error.to_string()))?;
        Ok(output.content)
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Prüft ein bereits entpacktes Update-Bundle (SHA-256 + Ed25519-Skeleton).
#[tauri::command]
fn verify_update_bundle(bundle_path: String) -> AppResult<pa_update::BundleVerification> {
    let path = std::path::PathBuf::from(bundle_path);
    pa_update::verify_bundle(&path, &[]).map_err(|error| AppError::Invalid(error.to_string()))
}

/// Listet vorhandene rollierende Backups unter `AI/backup/auto/`.
#[tauri::command]
fn list_backups(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    let bootstrap = ensure_bootstrap(&state)?;
    let root = bootstrap
        .package_root
        .join("AI")
        .join("backup")
        .join("auto");
    let set = pa_update::BackupSet::new(root, 7);
    let paths = set
        .list()
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(paths.into_iter().map(|p| p.display().to_string()).collect())
}

/// Legt ein Vollexport-Verzeichnis (`export-<ms>/`) mit README + Grundstruktur an.
#[tauri::command]
fn create_full_export(state: State<'_, AppState>) -> AppResult<String> {
    let bootstrap = ensure_bootstrap(&state)?;
    let base = bootstrap.package_root.join("AI").join("export");
    std::fs::create_dir_all(&base)?;
    let path = pa_export::create_export_root(&base, now_unix_ms())
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(path.display().to_string())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstalledSkillView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub permissions: Vec<pa_skills::PermissionEntry>,
    pub tools: Vec<SkillToolView>,
    /// `wasm` (Code in der Sandbox) oder `instructions` (Anleitung aus einer SKILL.md).
    pub kind: String,
    pub description: String,
    /// Nur bei Anleitungen: wird dem Modell mitgegeben.
    pub active: bool,
    pub body_preview: String,
    pub files: Vec<String>,
    /// Beim Import bewusst übersprungene Dateien (Skripte, zu groß, …).
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillToolView {
    pub name: String,
    pub description: String,
}

// ============================================================================
// Konnektoren: Exa und der Air Gap (Master-Schalter für jeden ausgehenden Verkehr)
// ============================================================================

#[tauri::command]
fn get_connector_config(state: State<'_, AppState>) -> AppResult<ConnectorConfig> {
    let guard = lock(&state.connector_config)?;
    Ok(guard.clone())
}

#[tauri::command]
fn update_connector_config(
    state: State<'_, AppState>,
    config: ConnectorConfig,
) -> AppResult<ConnectorConfig> {
    let mut config = config;
    mail_cmds::validate_config(&config).map_err(AppError::Invalid)?;
    mail_cmds::normalize_config(&mut config);
    *lock(&state.connector_config)? = config.clone();
    // Harter Schalter im HTTPS-Client: unabhängig davon, wer Exa aufruft.
    net::set_air_gap(config.offline_mode);
    // Air Gap an oder Gmail aus beendet den Takt sofort; er startet nie von selbst wieder.
    if config.offline_mode || !config.gmail_enabled {
        mail_cmds::stop_polling(&state);
    }
    lifecycle::persist_connector_config(&state, &config)?;
    Ok(config)
}
