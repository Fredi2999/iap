//! Agent Flow: mehrere Coding-Kandidaten, jeder in einem eigenen Git-Worktree.
//!
//! Eine Aufgabe wird einmal eingegeben und auf getrennte Kandidaten verteilt.
//! Jeder Kandidat hat Worktree, Branch, Kontext, Modell und Budget. Die lokalen
//! Modellaufrufe laufen **nacheinander** durch die serielle Job-Queue und sind
//! dort sichtbar. Das Modell schreibt nie selbst: Es liefert Text, daraus wird
//! ein geprüfter Änderungsvorschlag (`pa-code::patch`), der ausschließlich im
//! Worktree des Kandidaten angewendet und dort committet wird. Die Übernahme
//! des Gewinners ins Zielprojekt braucht eine weitere ausdrückliche
//! Bestätigung nach der vollständigen Diff-Vorschau und eine Konfliktprüfung.
//!
//! Native Projekt-Tests laufen nicht: ohne nachgewiesene Netz- und Pfad-
//! Sandbox bleiben sie deaktiviert und erscheinen als „nicht ausgeführt“.

use std::{
    cell::Cell,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use pa_code::{
    git::{cli::AuditHook, flow::FlowProject},
    patch::ChangeProposal,
};
use pa_policy::{resolve_absolute_in_scope, Capability, CapabilityAction, Decision, PathScope};
use pa_types::{
    agent_flow::{
        AdoptPreview, AdoptResult, AgentFlowTask, Candidate, CandidateSpec, CandidateStatus,
        ProjectLocation, ProjectPreflight, ResourceUsage, StartAgentFlowRequest, TestStatus,
        MAX_CANDIDATES, T0_DEFAULT_CANDIDATES,
    },
    avatar::JobKind,
    chat::{Message, MessageRole, MessageStatus, ThinkingLevel},
    model::HardwareTier,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    ensure_bootstrap, installed_models_list, lifecycle, lock, random_id, require_session,
    select_model, volume, AppError, AppResult, AppState,
};

/// Verschiedene Herangehensweisen, damit die Kandidaten wirklich Alternativen liefern.
const HINTS: [&str; 5] = [
    "Ändere so wenig wie möglich und bleibe im vorhandenen Stil.",
    "Baue eine robuste Lösung mit sauberer Fehlerbehandlung.",
    "Stelle Lesbarkeit und Wartbarkeit an die erste Stelle.",
    "Wähle bewusst einen anderen Lösungsweg als die naheliegende Variante.",
    "Sei vorsichtig: ändere nur, was zwingend nötig ist, und erkläre kurz warum.",
];

const TESTS_NOT_RUN: &str =
    "Nicht ausgeführt: Native Projekt-Tests bleiben ohne nachgewiesene Netz- und Pfad-Sandbox deaktiviert.";

/// Höchstlänge eines Kontext-Auszugs pro Datei und insgesamt (Zeichen).
const FILE_CONTEXT_CHARS: usize = 6_000;
const TOTAL_CONTEXT_CHARS: usize = 14_000;
const MAX_CONTEXT_FILES: usize = 6;

/// Laufzeitzustand der Aufgaben (Abbruch-Flags und aktuelle Job-Nummern).
#[derive(Default)]
pub struct Runtime {
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    jobs: Mutex<HashMap<String, String>>,
}

impl Runtime {
    /// Leerer Zustand.
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, task_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut map) = self.cancels.lock() {
            map.insert(task_id.to_owned(), Arc::clone(&flag));
        }
        flag
    }

    fn running(&self, task_id: &str) -> bool {
        self.cancels
            .lock()
            .is_ok_and(|map| map.contains_key(task_id))
    }

    /// Bricht alle laufenden Aufgaben ab (Wechsel zum Pet, Beenden).
    pub fn cancel_all(&self) {
        if let Ok(map) = self.cancels.lock() {
            for flag in map.values() {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }

    fn finish(&self, task_id: &str) {
        if let Ok(mut map) = self.cancels.lock() {
            map.remove(task_id);
        }
        if let Ok(mut map) = self.jobs.lock() {
            map.remove(task_id);
        }
    }
}

// ---------------------------------------------------------------------------
// Hilfen
// ---------------------------------------------------------------------------

fn invalid(error: impl std::fmt::Display) -> AppError {
    AppError::Invalid(error.to_string())
}

pub(crate) fn git_program(state: &AppState) -> AppResult<PathBuf> {
    let pack = crate::pack_cmds::open_usable(state, crate::packs::PACK_GIT).map_err(|_| {
        AppError::Invalid(
            "Git-Paket nicht eingerichtet: Agent Flow ist nicht verfügbar.".to_owned(),
        )
    })?;
    let program = pack.file("cmd/git.exe").map_err(invalid)?;
    // Git bekommt gewöhnliche Laufwerkspfade; `\\?\`-Pfade sind für Prozessstarts unzuverlässig.
    Ok(pa_code::git::cli::simplify_path(&program))
}

fn audit_hook(app: &AppHandle) -> AuditHook {
    let handle = app.clone();
    Arc::new(move |subcommand: &str, decision: &Decision| {
        let state = handle.state::<AppState>();
        lifecycle::audit_decision(
            &state,
            CapabilityAction::GitOp,
            Some(subcommand.to_owned()),
            decision,
            "Git-Aufruf für Agent Flow",
        );
    })
}

fn open_project(app: &AppHandle, path: &str) -> AppResult<FlowProject> {
    let state = app.state::<AppState>();
    FlowProject::open(
        &git_program(&state)?,
        Path::new(path),
        Some(audit_hook(app)),
    )
    .map_err(invalid)
}

/// Öffnet das Projekt nur zum Prüfen; legt nichts beim Nutzer an.
fn open_project_for_check(app: &AppHandle, path: &str) -> AppResult<FlowProject> {
    let state = app.state::<AppState>();
    FlowProject::open_for_check(
        &git_program(&state)?,
        Path::new(path),
        Some(audit_hook(app)),
    )
    .map_err(invalid)
}

/// Stick oder Host: liegt das Projekt unter der Paketwurzel, ist es ein Stick-Projekt.
fn classify(state: &AppState, path: &Path) -> AppResult<ProjectLocation> {
    let root = lock(&state.package_root)?.clone();
    let scope = PathScope::new(&root).map_err(invalid)?;
    Ok(if resolve_absolute_in_scope(&scope, path).is_ok() {
        ProjectLocation::Stick
    } else {
        ProjectLocation::Host
    })
}

fn is_approved(state: &AppState, path: &str) -> AppResult<bool> {
    let session = require_session(state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(pa_vault::agent_flow_store::is_project_approved(
        vault.repository().connection(),
        path,
    )?)
}

fn approve(state: &AppState, path: &str, location: ProjectLocation) -> AppResult<()> {
    let session = require_session(state)?;
    let mut vault = session.vault_runtime.lock()?;
    pa_vault::agent_flow_store::approve_project(
        vault.repository_mut().connection_mut(),
        path,
        location,
        crate::now_unix_ms(),
    )?;
    Ok(())
}

fn store_task(state: &AppState, task: &AgentFlowTask) -> AppResult<()> {
    let session = require_session(state)?;
    let mut vault = session.vault_runtime.lock()?;
    pa_vault::agent_flow_store::upsert_task(vault.repository_mut().connection_mut(), task)?;
    Ok(())
}

fn sync_vault(state: &AppState) {
    if let Ok(session) = require_session(state) {
        if let Ok(mut vault) = session.vault_runtime.lock() {
            let mut no_fault = pa_vault::hot_copy::NoFault;
            let _ = vault.sync(&mut no_fault);
        }
    }
}

fn load_task(state: &AppState, task_id: &str) -> AppResult<AgentFlowTask> {
    let session = require_session(state)?;
    let vault = session.vault_runtime.lock()?;
    pa_vault::agent_flow_store::get_task(vault.repository().connection(), task_id)?
        .ok_or_else(|| invalid("Aufgabe nicht gefunden"))
}

fn publish(app: &AppHandle, task: &AgentFlowTask) {
    let state = app.state::<AppState>();
    if let Err(error) = store_task(&state, task) {
        eprintln!("Agent Flow: Aufgabe nicht gespeichert: {error}");
    }
    let _ = app.emit("agent-flow-changed", task);
}

/// Wählt die Dateien für den Kontext: ausdrücklich genannte, sonst solche, deren
/// Name in der Aufgabe vorkommt. Sensible Dateien sind nie dabei.
pub fn select_context(tracked: &[String], prompt: &str, explicit: &[String]) -> Vec<String> {
    let lower = prompt.to_lowercase();
    let mut chosen: Vec<String> = Vec::new();
    for path in explicit {
        if tracked.contains(path)
            && !pa_code::git::flow::is_sensitive(path)
            && !chosen.contains(path)
        {
            chosen.push(path.clone());
        }
    }
    if chosen.is_empty() {
        for path in tracked {
            let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
            let stem = name
                .rsplit_once('.')
                .map_or(name.as_str(), |(stem, _)| stem);
            let mentioned = lower.contains(&name) || (stem.len() >= 4 && lower.contains(stem));
            if mentioned && !pa_code::git::flow::is_sensitive(path) {
                chosen.push(path.clone());
            }
        }
    }
    chosen.truncate(MAX_CONTEXT_FILES);
    chosen
}

fn read_in_worktree(scope: &PathScope, relative: &str) -> Option<String> {
    let target = resolve_absolute_in_scope(scope, &scope.root().join(relative)).ok()?;
    std::fs::read_to_string(target).ok()
}

/// Baut den Auftragstext für ein Modell.
pub fn build_prompt(
    task: &str,
    hint: &str,
    files: &[(String, String)],
    too_large: &[String],
    all_files: &[String],
) -> String {
    let mut text = String::new();
    text.push_str("Du bist ein sorgfältiger Programmierassistent. Erledige die folgende Aufgabe an diesem Projekt.\n\n");
    text.push_str(&format!("Aufgabe:\n{task}\n\nHerangehensweise: {hint}\n\n"));
    if !all_files.is_empty() {
        let listed: Vec<&str> = all_files.iter().take(60).map(String::as_str).collect();
        text.push_str(&format!(
            "Dateien im Projekt (Auszug): {}\n\n",
            listed.join(", ")
        ));
    }
    for (path, content) in files {
        text.push_str(&format!(
            "### Aktuelle Datei: {path}\n```\n{content}\n```\n\n"
        ));
    }
    if !too_large.is_empty() {
        text.push_str(&format!(
            "Zu groß für den Kontext (nur als Unified-Diff ändern, nie ersetzen): {}\n\n",
            too_large.join(", ")
        ));
    }
    text.push_str(
        "Antworte NUR mit Änderungen, je Datei ein Block in genau diesem Format:\n\n\
         ### Datei: relativer/pfad.ext\n```\nvollständiger neuer Inhalt der Datei\n```\n\n\
         Für neue Dateien gilt dasselbe Format. Ändere nur, was die Aufgabe verlangt. \
         Schreibe keine Erklärungen außerhalb der Blöcke außer einer Zeile Zusammenfassung am Ende.",
    );
    text
}

fn user_message(content: String) -> Message {
    Message {
        id: "agent-flow".to_owned(),
        conversation_id: "agent-flow".to_owned(),
        position: 0,
        role: MessageRole::User,
        content,
        status: MessageStatus::Complete,
        created_at_unix_ms: crate::now_unix_ms(),
    }
}

fn llama_rss(package_root: &Path) -> Option<u64> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );
    system
        .processes()
        .values()
        .filter(|p| {
            p.name().to_string_lossy().starts_with("llama-server")
                && p.exe().is_some_and(|exe| exe.starts_with(package_root))
        })
        .map(sysinfo::Process::memory)
        .max()
}

// ---------------------------------------------------------------------------
// Befehle
// ---------------------------------------------------------------------------

/// Prüft ein Projekt vor dem Start. Für Host-Projekte ist `host_approved` die
/// ausdrückliche Freigabe des Nutzers; sie wird im Tresor gemerkt.
#[tauri::command(async)]
pub async fn agent_flow_preflight(
    app: AppHandle,
    project_path: String,
    host_approved: bool,
) -> AppResult<ProjectPreflight> {
    tauri::async_runtime::spawn_blocking(move || {
        preflight_blocking(&app, &project_path, host_approved)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

fn preflight_blocking(
    app: &AppHandle,
    project_path: &str,
    host_approved: bool,
) -> AppResult<ProjectPreflight> {
    let state = app.state::<AppState>();
    require_session(&state)?;
    let project = open_project_for_check(app, project_path)?;
    let root = project.info().root.to_string_lossy().into_owned();
    let location = classify(&state, &project.info().root)?;
    if location == ProjectLocation::Host && !is_approved(&state, &root)? {
        if !host_approved {
            return Err(invalid(
                "Freigabe nötig: Dieses Projekt liegt auf dem Computer, nicht auf dem Stick.",
            ));
        }
        approve(&state, &root, location)?;
    }
    let volume = volume::query(&project.info().root).unwrap_or(volume::VolumeInfo {
        filesystem: "unbekannt".to_owned(),
        free_bytes: 0,
    });
    project
        .preflight(location, &volume.filesystem, volume.free_bytes)
        .map_err(invalid)
}

/// Laufender Fortschritt eines Kandidaten (geschätzt aus der Textlänge).
#[derive(Debug, Clone, Serialize)]
pub struct FlowProgress {
    pub task_id: String,
    pub candidate_id: String,
    pub tokens: u32,
    pub seconds: f64,
}

/// Vorgaben für die Oberfläche: Kandidatenzahl je nach Hardware.
#[derive(Debug, Serialize)]
pub struct FlowLimits {
    pub default_candidates: u32,
    pub max_candidates: u32,
    /// Sequenzielle Ausführung ist auf jeder Hardware so; auf T0 ist sie besonders langsam.
    pub weak_hardware: bool,
}

#[tauri::command]
pub fn agent_flow_limits(state: State<'_, AppState>) -> AppResult<FlowLimits> {
    let bootstrap = ensure_bootstrap(&state)?;
    let tier = require_session(&state)
        .ok()
        .and_then(|s| s.tier_override)
        .unwrap_or(bootstrap.plan.tier);
    let weak = matches!(tier, HardwareTier::T0 | HardwareTier::Unsupported);
    Ok(FlowLimits {
        default_candidates: if weak { T0_DEFAULT_CANDIDATES } else { 3 },
        max_candidates: MAX_CANDIDATES,
        weak_hardware: weak,
    })
}

/// Freigegebene Host-Projekte.
#[derive(Debug, Serialize)]
pub struct ApprovedProject {
    pub path: String,
    pub location: ProjectLocation,
}

#[tauri::command]
pub fn agent_flow_projects(state: State<'_, AppState>) -> AppResult<Vec<ApprovedProject>> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(
        pa_vault::agent_flow_store::list_approved_projects(vault.repository().connection())?
            .into_iter()
            .map(|(path, location)| ApprovedProject { path, location })
            .collect(),
    )
}

#[tauri::command]
pub fn agent_flow_revoke_project(state: State<'_, AppState>, path: String) -> AppResult<()> {
    let session = require_session(&state)?;
    let mut vault = session.vault_runtime.lock()?;
    pa_vault::agent_flow_store::revoke_project(vault.repository_mut().connection_mut(), &path)?;
    Ok(())
}

/// Startet die Kandidaten. Rückgabe sofort; der Fortschritt kommt über das Ereignis
/// `agent-flow-changed` und `list_jobs`.
#[tauri::command(async)]
pub async fn agent_flow_start(
    app: AppHandle,
    request: StartAgentFlowRequest,
) -> AppResult<AgentFlowTask> {
    tauri::async_runtime::spawn_blocking(move || start_blocking(&app, request))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

fn start_blocking(app: &AppHandle, request: StartAgentFlowRequest) -> AppResult<AgentFlowTask> {
    let state = app.state::<AppState>();
    require_session(&state)?;
    if request.candidates.is_empty() {
        return Err(invalid("Bitte mindestens einen Kandidaten wählen."));
    }
    if request.candidates.len() > MAX_CANDIDATES as usize {
        return Err(invalid(format!(
            "Höchstens {MAX_CANDIDATES} Kandidaten sind möglich."
        )));
    }
    if request.prompt.trim().is_empty() {
        return Err(invalid("Die Aufgabe darf nicht leer sein."));
    }
    let installed = installed_models_list(&state)?;
    for spec in &request.candidates {
        if !installed.iter().any(|m| m.id == spec.model_id) {
            return Err(invalid(format!(
                "Das Modell `{}` ist nicht installiert.",
                spec.model_id
            )));
        }
        if spec.max_tokens == 0 || spec.max_seconds == 0 {
            return Err(invalid(
                "Jeder Kandidat braucht ein Budget für Token und Zeit.",
            ));
        }
    }

    let preflight = preflight_blocking(app, &request.project_path, request.host_project_approved)?;
    if !preflight.blockers.is_empty() {
        return Err(invalid(preflight.blockers.join(" ")));
    }
    let project = open_project(app, &request.project_path)?;
    let task_id = format!("t-{}", &random_id("x")[2..10]);
    let base = project
        .create_base(&task_id, &request.base)
        .map_err(invalid)?;
    let worktrees_root = project.worktrees_root().to_path_buf();
    let candidates: Vec<Candidate> = request
        .candidates
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let index = u32::try_from(i + 1).unwrap_or(1);
            Candidate {
                id: format!("{task_id}-k{index}"),
                index,
                branch: format!("iap/{task_id}/k{index}"),
                worktree_path: worktrees_root
                    .join(&task_id)
                    .join(format!("k{index}"))
                    .to_string_lossy()
                    .into_owned(),
                model_id: spec.model_id.clone(),
                status: CandidateStatus::Queued { position: index },
                usage: ResourceUsage::default(),
                tests: TestStatus::NotRun {
                    reason: TESTS_NOT_RUN.to_owned(),
                },
                changed_files: Vec::new(),
                base_commit: base.base_commit.clone(),
            }
        })
        .collect();
    let task = AgentFlowTask {
        id: task_id.clone(),
        project_path: preflight.repo_root.clone(),
        location: preflight.location,
        prompt: request.prompt.trim().to_owned(),
        base,
        candidates,
        created_unix_ms: crate::now_unix_ms(),
    };
    publish(app, &task);

    let cancel = state.flow.agent_flow.register(&task_id);
    let worker_app = app.clone();
    let worker_task = task.clone();
    let specs = request.candidates;
    let context_files = request.context_files;
    thread::Builder::new()
        .name(format!("iap-agent-flow-{task_id}"))
        .spawn(move || {
            run_task(
                &worker_app,
                project,
                worker_task,
                &specs,
                &context_files,
                &cancel,
            )
        })
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(task)
}

/// Arbeitet die Kandidaten nacheinander ab (serielle Queue, sichtbar).
fn run_task(
    app: &AppHandle,
    project: FlowProject,
    mut task: AgentFlowTask,
    specs: &[CandidateSpec],
    context_files: &[String],
    cancel: &Arc<AtomicBool>,
) {
    let state = app.state::<AppState>();
    let original_model = require_session(&state)
        .map(|s| s.model_id)
        .unwrap_or_default();
    let mut current_model = original_model.clone();
    let total = task.candidates.len();

    for (position, spec) in specs.iter().enumerate().take(total) {
        let label = format!("Agent Flow: Kandidat {} von {total}", position + 1);
        if cancel.load(Ordering::SeqCst) {
            mark_remaining_cancelled(&mut task, position);
            break;
        }
        let mut job = state.flow.jobs.submit(JobKind::AgentFlow, label, true);
        if let Ok(mut map) = state.flow.agent_flow.jobs.lock() {
            map.insert(task.id.clone(), job.id().to_owned());
        }
        let job_cancel = job.cancel_flag();
        set_status(
            app,
            &mut task,
            position,
            CandidateStatus::Queued { position: 1 },
        );
        let mut slot = match job.acquire() {
            Ok(slot) => slot,
            Err(_) => {
                mark_remaining_cancelled(&mut task, position);
                break;
            }
        };
        set_status(app, &mut task, position, CandidateStatus::Loading);
        if spec.model_id != current_model {
            match tauri::async_runtime::block_on(select_model(app.clone(), spec.model_id.clone())) {
                Ok(_) => current_model = spec.model_id.clone(),
                Err(error) => {
                    slot.fail();
                    drop(slot);
                    set_status(
                        app,
                        &mut task,
                        position,
                        CandidateStatus::Failed {
                            reason: format!("Modellwechsel fehlgeschlagen: {error}"),
                        },
                    );
                    continue;
                }
            }
        }
        let outcome = run_candidate(
            app,
            &project,
            &mut task,
            position,
            spec,
            context_files,
            cancel,
            &job_cancel,
        );
        match outcome {
            Ok(()) => drop(slot),
            Err(CandidateEnd::Cancelled) => {
                slot.cancelled();
                drop(slot);
                set_status(app, &mut task, position, CandidateStatus::Cancelled);
                mark_remaining_cancelled(&mut task, position + 1);
                publish(app, &task);
                break;
            }
            Err(CandidateEnd::Failed(reason)) => {
                slot.fail();
                drop(slot);
                set_status(app, &mut task, position, CandidateStatus::Failed { reason });
            }
        }
    }

    if current_model != original_model && !original_model.is_empty() {
        if let Err(error) =
            tauri::async_runtime::block_on(select_model(app.clone(), original_model))
        {
            eprintln!("Agent Flow: ursprüngliches Modell nicht wiederhergestellt: {error}");
        }
    }
    publish(app, &task);
    sync_vault(&state);
    state.flow.agent_flow.finish(&task.id);
}

enum CandidateEnd {
    Cancelled,
    Failed(String),
}

fn mark_remaining_cancelled(task: &mut AgentFlowTask, from: usize) {
    for candidate in task.candidates.iter_mut().skip(from) {
        if matches!(
            candidate.status,
            CandidateStatus::Queued { .. }
                | CandidateStatus::Loading
                | CandidateStatus::Generating
                | CandidateStatus::Applying
        ) {
            candidate.status = CandidateStatus::Cancelled;
        }
    }
}

fn set_status(app: &AppHandle, task: &mut AgentFlowTask, position: usize, status: CandidateStatus) {
    if let Some(candidate) = task.candidates.get_mut(position) {
        candidate.status = status;
    }
    publish(app, task);
}

#[allow(clippy::too_many_arguments)]
fn run_candidate(
    app: &AppHandle,
    project: &FlowProject,
    task: &mut AgentFlowTask,
    position: usize,
    spec: &CandidateSpec,
    context_files: &[String],
    cancel: &Arc<AtomicBool>,
    job_cancel: &Arc<AtomicBool>,
) -> Result<(), CandidateEnd> {
    let state = app.state::<AppState>();
    let index = task.candidates[position].index;
    let base_commit = task.base.base_commit.clone();
    let (worktree, branch) = project
        .add_worktree(&task.id, index, &base_commit)
        .map_err(|e| CandidateEnd::Failed(format!("Worktree konnte nicht angelegt werden: {e}")))?;
    let scope = PathScope::new(&worktree).map_err(|e| CandidateEnd::Failed(e.to_string()))?;

    // Kontext dieses Kandidaten: Aufgabe, ausgewählte Dateien, eigene Herangehensweise.
    let tracked = project.tracked_files().unwrap_or_default();
    let mut files = Vec::new();
    let mut too_large = Vec::new();
    let mut used = 0_usize;
    for path in select_context(&tracked, &task.prompt, context_files) {
        if let Some(content) = read_in_worktree(&scope, &path) {
            if content.len() > FILE_CONTEXT_CHARS || used + content.len() > TOTAL_CONTEXT_CHARS {
                too_large.push(path);
            } else {
                used += content.len();
                files.push((path, content));
            }
        }
    }
    let hint = HINTS[position % HINTS.len()];
    let prompt = build_prompt(&task.prompt, hint, &files, &too_large, &tracked);

    set_status(app, task, position, CandidateStatus::Generating);
    let session = require_session(&state).map_err(|e| CandidateEnd::Failed(e.to_string()))?;
    let started = Instant::now();
    let max_chars = usize::try_from(spec.max_tokens)
        .unwrap_or(usize::MAX)
        .saturating_mul(4);
    let max_time = Duration::from_secs(spec.max_seconds);
    let produced = Cell::new(0_usize);
    let budget_hit = Cell::new(false);
    let outcome = {
        let mut engine = session
            .engine
            .lock()
            .map_err(|_| CandidateEnd::Failed("Modell-Sperre vergiftet".to_owned()))?;
        engine.set_thinking_level(ThinkingLevel::Kurz);
        let mut should_continue = || {
            let stop = cancel.load(Ordering::SeqCst) || job_cancel.load(Ordering::SeqCst);
            let over = started.elapsed() >= max_time || produced.get() >= max_chars;
            if over {
                budget_hit.set(true);
            }
            !stop && !over
        };
        let messages = [user_message(prompt)];
        let last_report = Cell::new(Instant::now());
        let (task_id, candidate_id) = (task.id.clone(), task.candidates[position].id.clone());
        engine.stream_chat_with_grammar(&messages, None, &mut should_continue, &mut |delta| {
            produced.set(produced.get() + delta.len());
            // Fortschritt für die Oberfläche: nur als Ereignis, nicht im Tresor.
            if last_report.get().elapsed() >= Duration::from_millis(700) {
                last_report.set(Instant::now());
                let _ = app.emit(
                    "agent-flow-progress",
                    FlowProgress {
                        task_id: task_id.clone(),
                        candidate_id: candidate_id.clone(),
                        tokens: u32::try_from(produced.get() / 4).unwrap_or(u32::MAX),
                        seconds: started.elapsed().as_secs_f64(),
                    },
                );
            }
            true
        })
    };
    let elapsed = started.elapsed().as_secs_f64();
    let text = match &outcome {
        Ok(result) => result.text.clone(),
        Err(error) => error.partial_text.clone(),
    };
    let root = lock(&state.package_root)
        .map(|guard| guard.clone())
        .unwrap_or_default();
    {
        let candidate = &mut task.candidates[position];
        candidate.usage = ResourceUsage {
            tokens: outcome
                .as_ref()
                .ok()
                .and_then(|o| o.completion_tokens)
                .unwrap_or_else(|| u32::try_from(text.len() / 4).unwrap_or(u32::MAX)),
            seconds: elapsed,
            peak_rss_bytes: llama_rss(&root),
        };
        candidate.branch = branch.clone();
        candidate.worktree_path = worktree.to_string_lossy().into_owned();
    }

    if cancel.load(Ordering::SeqCst) || job_cancel.load(Ordering::SeqCst) {
        return Err(CandidateEnd::Cancelled);
    }
    match outcome {
        Err(error) => {
            return Err(CandidateEnd::Failed(format!(
                "Das Modell hat nicht geantwortet: {}",
                error.message
            )))
        }
        Ok(result) if result.aborted || budget_hit.get() => {
            return Err(CandidateEnd::Failed(
                "Das Budget (Zeit oder Token) war ausgeschöpft, bevor die Antwort vollständig war."
                    .to_owned(),
            ));
        }
        Ok(_) => {}
    }

    set_status(app, task, position, CandidateStatus::Applying);
    let proposal = ChangeProposal::parse(&text).map_err(|e| CandidateEnd::Failed(e.to_string()))?;
    let changes = proposal
        .resolve(|path| read_in_worktree(&scope, path))
        .map_err(|e| CandidateEnd::Failed(e.to_string()))?;
    let message = format!(
        "IAP Kandidat {index}: {}",
        task.prompt.lines().next().unwrap_or("Aufgabe")
    );
    project
        .apply_and_commit(&worktree, &changes, &message, Some(job_cancel.as_ref()))
        .map_err(|e| match e {
            pa_code::git::GitError::Cancelled => CandidateEnd::Cancelled,
            other => CandidateEnd::Failed(other.to_string()),
        })?;
    let changed = project
        .changed_files(&base_commit, &branch)
        .map_err(|e| CandidateEnd::Failed(e.to_string()))?;
    task.candidates[position].changed_files = changed.into_iter().map(|(_, path)| path).collect();
    set_status(app, task, position, CandidateStatus::Ready);
    Ok(())
}

/// Alle gespeicherten Aufgaben, neueste zuerst.
#[tauri::command]
pub fn agent_flow_list(state: State<'_, AppState>) -> AppResult<Vec<AgentFlowTask>> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(pa_vault::agent_flow_store::list_tasks(
        vault.repository().connection(),
    )?)
}

/// Vollständiger Diff Basis → Kandidat.
#[tauri::command(async)]
pub async fn agent_flow_diff(
    app: AppHandle,
    task_id: String,
    candidate_id: String,
) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let task = load_task(&state, &task_id)?;
        let candidate = task
            .candidates
            .iter()
            .find(|c| c.id == candidate_id)
            .ok_or_else(|| invalid("Kandidat nicht gefunden"))?;
        let project = open_project(&app, &task.project_path)?;
        project
            .diff(&task.base.base_commit, &candidate.branch)
            .map_err(invalid)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

/// Bricht eine laufende Aufgabe kontrolliert ab. Bereits fertige Kandidaten bleiben erhalten.
#[tauri::command]
pub fn agent_flow_cancel(state: State<'_, AppState>, task_id: String) -> AppResult<()> {
    if let Ok(map) = state.flow.agent_flow.cancels.lock() {
        if let Some(flag) = map.get(&task_id) {
            flag.store(true, Ordering::SeqCst);
        }
    }
    let job = state
        .flow
        .agent_flow
        .jobs
        .lock()
        .ok()
        .and_then(|map| map.get(&task_id).cloned());
    if let Some(job) = job {
        state.flow.jobs.cancel(&job);
    }
    Ok(())
}

/// Vorschau der Übernahme mit Konfliktprüfung; schreibt nichts.
#[tauri::command(async)]
pub async fn agent_flow_adopt_preview(
    app: AppHandle,
    task_id: String,
    candidate_id: String,
) -> AppResult<AdoptPreview> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let task = load_task(&state, &task_id)?;
        let candidate = ready_candidate(&task, &candidate_id)?;
        open_project(&app, &task.project_path)?
            .adopt_preview(&candidate.id, &task.base, &candidate.branch)
            .map_err(invalid)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

fn ready_candidate<'a>(task: &'a AgentFlowTask, candidate_id: &str) -> AppResult<&'a Candidate> {
    let candidate = task
        .candidates
        .iter()
        .find(|c| c.id == candidate_id)
        .ok_or_else(|| invalid("Kandidat nicht gefunden"))?;
    if !matches!(candidate.status, CandidateStatus::Ready) {
        return Err(invalid("Nur ein fertiger Kandidat kann übernommen werden."));
    }
    Ok(candidate)
}

/// Übernimmt den Gewinner ins Zielprojekt. `confirmed` ist die zweite, ausdrückliche
/// Bestätigung nach der vollständigen Diff-Vorschau. Kein Commit, kein Push.
#[tauri::command(async)]
pub async fn agent_flow_adopt(
    app: AppHandle,
    task_id: String,
    candidate_id: String,
    confirmed: bool,
) -> AppResult<AdoptResult> {
    tauri::async_runtime::spawn_blocking(move || {
        if !confirmed {
            return Err(invalid(
                "Die Übernahme braucht eine ausdrückliche Bestätigung.",
            ));
        }
        let state = app.state::<AppState>();
        let task = load_task(&state, &task_id)?;
        if state.flow.agent_flow.running(&task_id) {
            return Err(invalid("Die Aufgabe läuft noch."));
        }
        let candidate = ready_candidate(&task, &candidate_id)?;
        let project = open_project(&app, &task.project_path)?;
        let result = project.adopt(&task.id, &candidate.id, &task.base, &candidate.branch);
        let decision = match &result {
            Ok(_) => Decision::Allow(Capability {
                action: CapabilityAction::FileWrite,
                canonical_path: None,
            }),
            Err(error) => Decision::Deny(error.to_string()),
        };
        lifecycle::audit_decision(
            &state,
            CapabilityAction::FileWrite,
            Some(task.project_path.clone()),
            &decision,
            "Gewinnerübernahme nach ausdrücklicher Bestätigung",
        );
        sync_vault(&state);
        result.map_err(invalid)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

/// Räumt eine Aufgabe auf: Worktrees und Branches aller Kandidaten werden entfernt.
/// Nur nach ausdrücklicher Bestätigung; die Ergebnisse sind danach weg.
#[tauri::command(async)]
pub async fn agent_flow_cleanup(app: AppHandle, task_id: String, confirmed: bool) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        if !confirmed {
            return Err(invalid(
                "Das Aufräumen braucht eine ausdrückliche Bestätigung.",
            ));
        }
        let state = app.state::<AppState>();
        let task = load_task(&state, &task_id)?;
        if state.flow.agent_flow.running(&task_id) {
            return Err(invalid("Die Aufgabe läuft noch."));
        }
        let project = open_project(&app, &task.project_path)?;
        let mut problems = Vec::new();
        for candidate in &task.candidates {
            let path = Path::new(&candidate.worktree_path);
            if path.exists() {
                if let Err(error) = project.remove_candidate(&task.id, path, &candidate.branch) {
                    problems.push(format!("{}: {error}", candidate.branch));
                }
            }
        }
        if !problems.is_empty() {
            return Err(invalid(format!(
                "Nicht alles ließ sich entfernen: {}",
                problems.join("; ")
            )));
        }
        let _ = project.repair();
        let session = require_session(&state)?;
        let mut vault = session.vault_runtime.lock()?;
        pa_vault::agent_flow_store::delete_task(vault.repository_mut().connection_mut(), &task.id)?;
        drop(vault);
        sync_vault(&state);
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

/// Markiert beim Entsperren Kandidaten, die durch ein Beenden mitten im Lauf hängen
/// geblieben sind, als fehlgeschlagen. Es wird nichts automatisch fortgesetzt.
pub fn recover_interrupted(state: &AppState) -> AppResult<()> {
    let session = require_session(state)?;
    let mut vault = session.vault_runtime.lock()?;
    let tasks = pa_vault::agent_flow_store::list_tasks(vault.repository().connection())?;
    for mut task in tasks {
        let mut changed = false;
        for candidate in &mut task.candidates {
            if matches!(
                candidate.status,
                CandidateStatus::Queued { .. }
                    | CandidateStatus::Loading
                    | CandidateStatus::Generating
                    | CandidateStatus::Applying
            ) {
                candidate.status = CandidateStatus::Failed {
                    reason: "IAP wurde beendet, bevor dieser Kandidat fertig war. Es wird nichts automatisch fortgesetzt.".to_owned(),
                };
                changed = true;
            }
        }
        if changed {
            pa_vault::agent_flow_store::upsert_task(
                vault.repository_mut().connection_mut(),
                &task,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked() -> Vec<String> {
        [
            "src/main.rs",
            "src/parser.rs",
            "README.md",
            ".env",
            "docs/handbuch.md",
            "tests/it.rs",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
    }

    #[test]
    fn context_follows_files_named_in_the_task_and_never_includes_secrets() {
        let picked = select_context(
            &tracked(),
            "Der Parser in parser.rs stürzt ab, siehe auch .env",
            &[],
        );
        assert_eq!(picked, vec!["src/parser.rs".to_owned()]);
    }

    #[test]
    fn explicit_files_win_and_unknown_or_sensitive_ones_are_dropped() {
        let picked = select_context(
            &tracked(),
            "egal",
            &[
                "README.md".to_owned(),
                ".env".to_owned(),
                "gibt_es_nicht.rs".to_owned(),
            ],
        );
        assert_eq!(picked, vec!["README.md".to_owned()]);
    }

    #[test]
    fn prompt_contains_task_hint_files_and_the_answer_format() {
        let prompt = build_prompt(
            "Behebe den Fehler",
            HINTS[0],
            &[("a.rs".to_owned(), "fn a() {}".to_owned())],
            &["riesig.rs".to_owned()],
            &["a.rs".to_owned(), "riesig.rs".to_owned()],
        );
        for expected in [
            "Behebe den Fehler",
            "### Aktuelle Datei: a.rs",
            "riesig.rs",
            "### Datei: relativer/pfad.ext",
        ] {
            assert!(prompt.contains(expected), "{expected}");
        }
    }

    #[test]
    fn hints_differ_between_the_five_candidates() {
        let unique: std::collections::HashSet<_> = HINTS.iter().collect();
        assert_eq!(unique.len(), MAX_CANDIDATES as usize);
    }
}
