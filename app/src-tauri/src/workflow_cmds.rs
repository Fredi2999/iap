//! Workflows: Definitionen, Läufe und die App-seitigen Anschlüsse für den Runner.
//!
//! Der Runner (`pa_core::workflow`) kennt weder Netz noch Modell. Hier hängen
//! die Anschlüsse an die echten Dinge – und jeder Exa-Aufruf geht durch
//! `pa_policy::egress::authorize_exa` mit Audit-Eintrag:
//!
//! - **Air Gap** wird bei *jedem* Aufruf neu gelesen; ein Umschalten mitten im
//!   Lauf sperrt sofort, der Lauf endet ehrlich.
//! - **Freigabe je Lauf**: [`ExaRunGrant`] entsteht beim Start, gilt nur für
//!   diese `run_id` und wird nirgends gespeichert. Ein neuer Lauf (auch eine
//!   „Fortsetzung“) braucht eine neue Freigabe; nach einem Neustart läuft nichts
//!   automatisch weiter.
//! - Workflows laufen nur, solange das Hauptfenster offen ist: der Wechsel zum
//!   Pet und das Beenden brechen sie ab (`cancel_all`).

use std::{
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use pa_core::workflow::{
    self, ExaContentsOutcome, ExaSearchOutcome, ModelOutcome, PortError, RunSettings, WorkflowPorts,
};
use pa_policy::{
    egress::{
        authorize_connector, Connector, ConnectorRequest, DataOrigin, ExaEndpoint, ExaRunGrant,
        PublicText,
    },
    Decision,
};
use pa_types::{
    avatar::JobKind,
    chat::{Message, MessageRole, MessageStatus, ThinkingLevel},
    flow::{
        ExaStatus, NodeKind, RunBudget, RunEvent, RunStatus, RunUsage, StartRunRequest,
        WorkflowDefinition, WorkflowGraph, WorkflowRunReport, WorkflowValidation,
    },
    ipc::ConnectorConfig,
    ipc::ExaSearchResult,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    exa::ExaClient, lifecycle, lock, now_unix_ms, random_id, require_session, AppError, AppResult,
    AppState,
};

const MAX_NODES: usize = 40;
const MAX_EDGES: usize = 120;

/// Abbruch-Flags laufender Läufe.
#[derive(Default)]
pub struct Runtime {
    runs: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Runtime {
    /// Leerer Zustand.
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, run_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut map) = self.runs.lock() {
            map.insert(run_id.to_owned(), Arc::clone(&flag));
        }
        flag
    }

    fn finish(&self, run_id: &str) {
        if let Ok(mut map) = self.runs.lock() {
            map.remove(run_id);
        }
    }

    /// Ob gerade ein Lauf aktiv ist.
    pub fn any_running(&self) -> bool {
        self.runs.lock().is_ok_and(|map| !map.is_empty())
    }

    /// Der laufende Lauf, falls es einen gibt (es läuft immer höchstens einer).
    pub fn active(&self) -> Option<String> {
        self.runs
            .lock()
            .ok()
            .and_then(|map| map.keys().next().cloned())
    }

    /// Bricht einen Lauf ab; `true`, wenn er lief.
    pub fn cancel(&self, run_id: &str) -> bool {
        self.runs.lock().is_ok_and(|map| {
            map.get(run_id).is_some_and(|flag| {
                flag.store(true, Ordering::SeqCst);
                true
            })
        })
    }

    /// Bricht alle Läufe ab (Wechsel zum Pet, Beenden).
    pub fn cancel_all(&self) -> usize {
        self.runs.lock().map_or(0, |map| {
            for flag in map.values() {
                flag.store(true, Ordering::SeqCst);
            }
            map.len()
        })
    }
}

fn invalid(text: impl Into<String>) -> AppError {
    AppError::Invalid(text.into())
}

/// Sichtbare Höchstwerte prüfen: harte Obergrenzen, damit auch ein fehlerhafter
/// Aufrufer keinen unbegrenzten Lauf startet.
pub fn check_budget(budget: &RunBudget) -> Result<(), String> {
    if budget.max_searches > 50 {
        return Err("Höchstens 50 Exa-Aufrufe je Lauf.".to_owned());
    }
    if budget.max_iterations > 5 {
        return Err("Höchstens 5 Wiederholungen je Lauf.".to_owned());
    }
    if !(10..=3_600).contains(&budget.max_seconds) {
        return Err("Die Laufzeit muss zwischen 10 Sekunden und 1 Stunde liegen.".to_owned());
    }
    if !(256..=200_000).contains(&budget.max_tokens) {
        return Err("Das Token-Limit muss zwischen 256 und 200000 liegen.".to_owned());
    }
    if !budget.max_cost_usd.is_finite() || !(0.0..=5.0).contains(&budget.max_cost_usd) {
        return Err("Das Kostenlimit muss zwischen 0 und 5 US-Dollar liegen.".to_owned());
    }
    Ok(())
}

#[cfg(test)]
fn uses_exa(graph: &WorkflowGraph) -> bool {
    connectors_used(graph).contains(&Connector::Exa)
}

/// Welche externen Dienste ein Ablauf braucht, ohne Wiederholung.
fn connectors_used(graph: &WorkflowGraph) -> Vec<Connector> {
    let mut used = Vec::new();
    for node in &graph.nodes {
        let connector = match node.kind {
            NodeKind::ExaSearch { .. } | NodeKind::ExaContents { .. } => Connector::Exa,
            NodeKind::WikipediaSearch { .. } => Connector::Wikipedia,
            NodeKind::BraveSearch { .. } => Connector::Brave,
            NodeKind::Weather { .. } => Connector::OpenMeteo,
            _ => continue,
        };
        if !used.contains(&connector) {
            used.push(connector);
        }
    }
    used
}

/// Anzeigename eines Konnektors in Meldungen.
fn connector_name(connector: Connector) -> &'static str {
    match connector {
        Connector::Exa => "Exa",
        Connector::Wikipedia => "Wikipedia",
        Connector::OpenMeteo => "Open-Meteo (Wetter)",
        Connector::Brave => "Brave Search",
        Connector::Mail => "Gmail",
        Connector::Calendar => "Kalender",
    }
}

/// Ob der Konnektor eingerichtet ist (eingeschaltet, Schlüssel vorhanden). `Err` mit Hinweis für den Nutzer.
fn connector_ready(config: &ConnectorConfig, connector: Connector) -> Result<(), String> {
    let (enabled, has_key) = match connector {
        Connector::Exa => (config.exa_enabled, !config.exa_api_key.trim().is_empty()),
        Connector::Wikipedia => (config.wikipedia_enabled, true),
        Connector::OpenMeteo => (config.open_meteo_enabled, true),
        Connector::Brave => (
            config.brave_enabled,
            !config.brave_api_key.trim().is_empty(),
        ),
        Connector::Mail => (config.gmail_enabled, true),
        // Der Kalender hat eigene Verbindungen im Kalender-Tab und läuft nie in Abläufen.
        Connector::Calendar => (false, true),
    };
    let name = connector_name(connector);
    if !enabled {
        return Err(format!(
            "{name} ist in den Konnektor-Einstellungen ausgeschaltet."
        ));
    }
    if connector.needs_key() && !has_key {
        return Err(format!("Für {name} ist kein Schlüssel hinterlegt."));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Exa-Zustand
// ---------------------------------------------------------------------------

/// Zustand eines Konnektors für die Oberfläche (ohne Schlüssel).
#[derive(Debug, Clone, Serialize)]
pub struct ConnectorState {
    /// `exa`, `wikipedia`, `open_meteo`, `brave`.
    pub id: &'static str,
    pub name: &'static str,
    pub enabled: bool,
    /// Eingeschaltet und, wenn nötig, mit Schlüssel.
    pub ready: bool,
    /// Warum er nicht bereit ist (deutscher Text für `t()`).
    pub reason: Option<String>,
}

/// Übersicht über alle Web-Konnektoren und den Air Gap.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectorOverview {
    pub air_gap: bool,
    pub services: Vec<ConnectorState>,
}

/// Zustand aller Web-Konnektoren (ohne Schlüssel); damit prüft die Oberfläche vor einem Lauf.
#[tauri::command]
pub fn connector_overview(state: State<'_, AppState>) -> AppResult<ConnectorOverview> {
    let config = lock(&state.connector_config)?.clone();
    let services = [
        ("exa", Connector::Exa, config.exa_enabled),
        ("wikipedia", Connector::Wikipedia, config.wikipedia_enabled),
        (
            "open_meteo",
            Connector::OpenMeteo,
            config.open_meteo_enabled,
        ),
        ("brave", Connector::Brave, config.brave_enabled),
    ]
    .into_iter()
    .map(|(id, connector, enabled)| {
        let check = connector_ready(&config, connector);
        ConnectorState {
            id,
            name: connector_name(connector),
            enabled,
            ready: check.is_ok(),
            reason: check.err(),
        }
    })
    .collect();
    Ok(ConnectorOverview {
        air_gap: config.offline_mode,
        services,
    })
}

/// Zustand der Exa-Anbindung (ohne Schlüssel).
#[tauri::command]
pub fn exa_status(state: State<'_, AppState>) -> AppResult<ExaStatus> {
    let config = lock(&state.connector_config)?.clone();
    Ok(ExaStatus {
        has_key: !config.exa_api_key.trim().is_empty(),
        enabled: config.exa_enabled,
        air_gap: config.offline_mode,
    })
}

/// Echter Einzeltest eines Konnektors (`exa`, `wikipedia`, `open_meteo`, `brave`). Der Text geht an
/// den Dienst; deshalb braucht der Aufruf die ausdrückliche Bestätigung `approved`.
///
/// Der Test läuft über dieselben Anschlüsse und dieselbe Policy wie ein Workflow-Lauf.
#[tauri::command(async)]
pub async fn test_connector(
    app: AppHandle,
    connector: String,
    query: String,
    approved: bool,
) -> AppResult<Vec<ExaSearchResult>> {
    tauri::async_runtime::spawn_blocking(move || {
        if !approved {
            return Err(invalid(
                "Bitte bestätige, dass dieser Text an den Dienst gesendet werden darf.",
            ));
        }
        let state = app.state::<AppState>();
        require_session(&state)?;
        let text =
            PublicText::new(query, DataOrigin::UserPublic).map_err(|e| invalid(e.to_string()))?;
        let mut ports = AppPorts::new(
            &app,
            format!("test-{}", random_id("x")),
            2,
            Arc::new(AtomicBool::new(false)),
            0,
        );
        let denied = |e: PortError| match e {
            PortError::Denied(reason) | PortError::Failed(reason) => invalid(reason),
            PortError::Cancelled => invalid("Abgebrochen"),
        };
        let from_hits = |hits: Vec<pa_core::workflow::SearchHit>| -> Vec<ExaSearchResult> {
            hits.into_iter()
                .map(|hit| ExaSearchResult {
                    title: hit.title,
                    url: hit.url,
                    snippet: hit.text.chars().take(300).collect(),
                })
                .collect()
        };
        match connector.as_str() {
            "exa" => Ok(from_hits(
                ports.exa_search("", &text, 5).map_err(denied)?.hits,
            )),
            "wikipedia" => {
                let hits = ports
                    .web_search("", pa_types::flow::WebService::Wikipedia, "de", &text, 5)
                    .map_err(denied)?
                    .hits;
                Ok(from_hits(hits))
            }
            "brave" => {
                let hits = ports
                    .web_search("", pa_types::flow::WebService::Brave, "", &text, 5)
                    .map_err(denied)?
                    .hits;
                Ok(from_hits(hits))
            }
            "open_meteo" => {
                let report = ports.weather("", &text, 3).map_err(denied)?;
                Ok(vec![ExaSearchResult {
                    title: "Wetter".to_owned(),
                    url: String::new(),
                    snippet: report,
                }])
            }
            other => Err(invalid(format!("Unbekannter Konnektor: {other}"))),
        }
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}

/// Echter Einzeltest der Exa-Anbindung (Kurzform von [`test_connector`]).
#[tauri::command(async)]
pub async fn test_exa_search(
    app: AppHandle,
    query: String,
    approved: bool,
) -> AppResult<Vec<ExaSearchResult>> {
    test_connector(app, "exa".to_owned(), query, approved).await
}

// ---------------------------------------------------------------------------
// Anschlüsse
// ---------------------------------------------------------------------------

struct AppPorts {
    /// Abweichung der lokalen Zeit von UTC (aus dem Startdialog) für den Kalender-Baustein.
    tz_offset_minutes: i32,
    app: AppHandle,
    /// Lauf, für den die Freigabe gilt; der Runner übergibt dieselbe ID je Aufruf.
    run_id: String,
    grant: ExaRunGrant,
    cancel: Arc<AtomicBool>,
}

impl AppPorts {
    fn new(
        app: &AppHandle,
        run_id: String,
        max_calls: u32,
        cancel: Arc<AtomicBool>,
        tz_offset_minutes: i32,
    ) -> Self {
        Self {
            tz_offset_minutes,
            app: app.clone(),
            grant: ExaRunGrant::new(run_id.clone(), max_calls),
            run_id,
            cancel,
        }
    }

    /// Policy-Prüfung mit Audit für jeden Web-Konnektor. Erst danach entsteht ein Client.
    ///
    /// Gibt die Konfiguration zurück, damit der Aufrufer den Schlüssel lesen kann.
    fn authorize_web(
        &mut self,
        connector: Connector,
        host: &str,
        what: &str,
        text: &PublicText,
    ) -> Result<ConnectorConfig, PortError> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(PortError::Cancelled);
        }
        let state = self.app.state::<AppState>();
        let config = lock(&state.connector_config)
            .map_err(|e| PortError::Failed(e.to_string()))?
            .clone();
        let request = ConnectorRequest {
            run_id: &self.run_id,
            connector,
            host,
            text,
        };
        // Air Gap wird bei jedem Aufruf frisch gelesen.
        let decision = authorize_connector(&mut self.grant, config.offline_mode, &request);
        lifecycle::audit_decision(
            &state,
            connector.action(),
            Some(format!(
                "{what} ({} Zeichen)",
                text.as_str().chars().count()
            )),
            &decision,
            &format!("{}-Anfrage im Workflow-Lauf", connector_name(connector)),
        );
        match decision {
            Decision::Allow(_) => {}
            Decision::Prompt(reason) | Decision::Deny(reason) => {
                return Err(PortError::Denied(reason))
            }
        }
        connector_ready(&config, connector).map_err(PortError::Denied)?;
        Ok(config)
    }

    /// Exa: Policy-Prüfung, dann Client.
    fn authorize(
        &mut self,
        endpoint: ExaEndpoint,
        text: &PublicText,
    ) -> Result<ExaClient, PortError> {
        let config = self.authorize_web(
            Connector::Exa,
            pa_policy::egress::EXA_HOST,
            &format!("POST {}", endpoint.path()),
            text,
        )?;
        Ok(ExaClient::new(&config.exa_api_key))
    }
}

impl WorkflowPorts for AppPorts {
    fn exa_search(
        &mut self,
        run_id: &str,
        query: &PublicText,
        num_results: u32,
    ) -> Result<ExaSearchOutcome, PortError> {
        if !run_id.is_empty() && run_id != self.run_id {
            return Err(PortError::Denied("Falsche Lauf-ID".to_owned()));
        }
        let client = self.authorize(ExaEndpoint::Search, query)?;
        client
            .search(query, num_results)
            .map_err(|e| PortError::Failed(e.to_string()))
    }

    fn exa_contents(
        &mut self,
        run_id: &str,
        request: &PublicText,
        urls: &[String],
        max_characters: u32,
    ) -> Result<ExaContentsOutcome, PortError> {
        if run_id != self.run_id {
            return Err(PortError::Denied("Falsche Lauf-ID".to_owned()));
        }
        let client = self.authorize(ExaEndpoint::Contents, request)?;
        client
            .contents(urls, max_characters)
            .map_err(|e| PortError::Failed(e.to_string()))
    }

    fn local_model(
        &mut self,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<ModelOutcome, PortError> {
        let state = self.app.state::<AppState>();
        let mut job = state
            .flow
            .jobs
            .submit(JobKind::Workflow, "Workflow: Modell", true);
        let job_cancel = job.cancel_flag();
        let mut slot = job.acquire().map_err(|_| PortError::Cancelled)?;
        let session = require_session(&state).map_err(|e| PortError::Failed(e.to_string()))?;
        let make = |position: i64, role: MessageRole, content: &str| Message {
            id: format!("workflow-{position}"),
            conversation_id: "workflow".to_owned(),
            position,
            role,
            content: content.to_owned(),
            status: MessageStatus::Complete,
            created_at_unix_ms: now_unix_ms(),
        };
        // Frischer, getrennter Kontext: nur diese zwei Nachrichten, keine Werkzeuge.
        let messages = [
            make(0_i64, MessageRole::System, system),
            make(1, MessageRole::User, user),
        ];
        let max_chars = usize::try_from(max_tokens)
            .unwrap_or(usize::MAX)
            .saturating_mul(4);
        let produced = std::cell::Cell::new(0_usize);
        let outcome = {
            let mut engine = session
                .engine
                .lock()
                .map_err(|_| PortError::Failed("Modell-Sperre vergiftet".to_owned()))?;
            engine.set_thinking_level(ThinkingLevel::Kurz);
            let mut go = || {
                !self.cancel.load(Ordering::SeqCst)
                    && !job_cancel.load(Ordering::SeqCst)
                    && produced.get() < max_chars
            };
            engine.stream_chat_with_grammar(&messages, None, &mut go, &mut |delta| {
                produced.set(produced.get() + delta.len());
                true
            })
        };
        if self.cancel.load(Ordering::SeqCst) || job_cancel.load(Ordering::SeqCst) {
            slot.cancelled();
            return Err(PortError::Cancelled);
        }
        match outcome {
            Ok(result) => {
                let tokens = result
                    .completion_tokens
                    .unwrap_or_else(|| u32::try_from(result.text.len() / 4).unwrap_or(u32::MAX));
                Ok(ModelOutcome {
                    text: result.text,
                    tokens,
                })
            }
            Err(error) => {
                slot.fail();
                Err(PortError::Failed(format!(
                    "Das Modell hat nicht geantwortet: {}",
                    error.message
                )))
            }
        }
    }

    fn web_search(
        &mut self,
        run_id: &str,
        service: pa_types::flow::WebService,
        lang: &str,
        query: &PublicText,
        num_results: u32,
    ) -> Result<ExaSearchOutcome, PortError> {
        use crate::connectors::{
            brave::BraveClient,
            wikipedia::{Lang, WikipediaClient},
        };
        use pa_types::flow::WebService;
        if !run_id.is_empty() && run_id != self.run_id {
            return Err(PortError::Denied("Falsche Lauf-ID".to_owned()));
        }
        match service {
            WebService::Wikipedia => {
                let language = Lang::parse(lang);
                self.authorize_web(Connector::Wikipedia, language.host(), "GET Suche", query)?;
                WikipediaClient::new(language)
                    .search(query, num_results)
                    .map_err(|e| PortError::Failed(e.to_string()))
            }
            WebService::Brave => {
                let config = self.authorize_web(
                    Connector::Brave,
                    "api.search.brave.com",
                    "GET Suche",
                    query,
                )?;
                BraveClient::new(&config.brave_api_key)
                    .search(query, num_results)
                    .map_err(|e| PortError::Failed(e.to_string()))
            }
        }
    }

    fn weather(
        &mut self,
        run_id: &str,
        place: &PublicText,
        days: u32,
    ) -> Result<String, PortError> {
        use crate::connectors::open_meteo::OpenMeteoClient;
        if !run_id.is_empty() && run_id != self.run_id {
            return Err(PortError::Denied("Falsche Lauf-ID".to_owned()));
        }
        // Zwei Anfragen (Ort, Vorhersage) laufen unter einer Freigabe; beide stehen im Audit.
        self.authorize_web(
            Connector::OpenMeteo,
            "geocoding-api.open-meteo.com",
            "GET Ort",
            place,
        )?;
        let client = OpenMeteoClient::new();
        let found = client
            .find_place(place)
            .map_err(|e| PortError::Failed(e.to_string()))?;
        self.authorize_web(
            Connector::OpenMeteo,
            "api.open-meteo.com",
            "GET Vorhersage",
            place,
        )?;
        client
            .report(&found, days)
            .map_err(|e| PortError::Failed(e.to_string()))
    }

    fn mail_text(
        &mut self,
        from: &str,
        subject: &str,
        unread_only: bool,
        limit: u32,
    ) -> Result<String, PortError> {
        use crate::mail::{
            imap::Search,
            reader::{self, Account},
            tls::SystemMailTransport,
            IMAP_ENDPOINT,
        };
        let state = self.app.state::<AppState>();
        let config = lock(&state.connector_config)
            .map_err(|e| PortError::Failed(e.to_string()))?
            .clone();
        // Zugriff nur mit ausdrücklicher Freigabe in den Einstellungen und bei Air Gap aus.
        let decision = if !config.gmail_enabled || !config.gmail_allow_read {
            Decision::Deny(
                "Der Lesezugriff auf das Postfach ist in den Konnektor-Einstellungen nicht freigegeben"
                    .to_owned(),
            )
        } else {
            pa_policy::egress::authorize_mail(
                crate::net::air_gap_on(),
                IMAP_ENDPOINT.0,
                pa_policy::CapabilityAction::MailRead,
            )
        };
        // Im Audit steht nur das Ziel, nie Filtertext oder Mailinhalt.
        lifecycle::audit_decision(
            &state,
            pa_policy::CapabilityAction::MailRead,
            Some(format!("{} (Workflow-Suche)", IMAP_ENDPOINT.0)),
            &decision,
            "Postfach im Workflow-Lauf lesen",
        );
        match decision {
            Decision::Allow(_) => {}
            Decision::Prompt(reason) | Decision::Deny(reason) => {
                return Err(PortError::Denied(reason))
            }
        }
        let password = {
            let session = require_session(&state).map_err(|e| PortError::Failed(e.to_string()))?;
            let vault = session
                .vault_runtime
                .lock()
                .map_err(|_| PortError::Failed("Tresor-Sperre vergiftet".to_owned()))?;
            vault
                .repository()
                .setting("mail.password")
                .ok()
                .flatten()
                .filter(|p| !p.is_empty())
        }
        .ok_or_else(|| PortError::Denied("Es ist kein App-Passwort hinterlegt".to_owned()))?;
        let non_empty = |text: &str| Some(text.trim().to_owned()).filter(|t| !t.is_empty());
        let criteria = Search {
            unseen: unread_only,
            from: non_empty(from),
            subject: non_empty(subject),
            text: None,
        };
        let account = Account {
            address: config.gmail_address.trim(),
            password: &password,
        };
        let items = reader::search(
            &SystemMailTransport,
            &account,
            &criteria,
            usize::try_from(limit).unwrap_or(5),
        )
        .map_err(|e| PortError::Failed(e.to_string()))?;
        Ok(reader::render_summaries(&items))
    }

    fn calendar_text(&mut self, days_ahead: u32, include_tasks: bool) -> Result<String, PortError> {
        let state = self.app.state::<AppState>();
        // Verbundene Kalender (Apple, Google) aus dem Zwischenspeicher; vor der Tresor-Sperre lesen.
        let remote = crate::calendar_cmds::cached_events(&state);
        let session = require_session(&state).map_err(|e| PortError::Failed(e.to_string()))?;
        let vault = session
            .vault_runtime
            .lock()
            .map_err(|_| PortError::Failed("Tresor-Sperre vergiftet".to_owned()))?;
        // Der Kalender liegt als JSON im Tresor (Einstellungen `ui.scheduler.*`), nirgends sonst.
        let read = |key: &str| {
            vault
                .repository()
                .setting(&format!("uistate.{key}"))
                .ok()
                .flatten()
        };
        let mut events: Vec<pa_scheduler::Event> = read("ui.scheduler.events")
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        events.extend(remote.iter().map(crate::calendar_cmds::as_scheduler_event));
        let tasks: Vec<pa_scheduler::Task> = read("ui.scheduler.tasks")
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Ok(crate::agenda_text::format_agenda(
            &events,
            &tasks,
            now_unix_ms(),
            days_ahead,
            include_tasks,
            self.tz_offset_minutes,
        ))
    }

    fn memory_search(&mut self, query: &str, max_hits: u32) -> Result<String, PortError> {
        let state = self.app.state::<AppState>();
        let memory = crate::session_memory(&state).map_err(|e| PortError::Failed(e.to_string()))?;
        let store = memory
            .lock()
            .map_err(|_| PortError::Failed("Gedächtnis-Sperre vergiftet".to_owned()))?;
        let hits = store
            .retrieve(query)
            .map_err(|e| PortError::Failed(format!("Gedächtnissuche fehlgeschlagen: {e}")))?;
        if hits.is_empty() {
            return Ok("Im Gedächtnis wurde nichts Passendes gefunden.".to_owned());
        }
        Ok(hits
            .iter()
            .take(max_hits as usize)
            .map(|hit| format!("- {}", hit.preview.trim()))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn skill_instructions(&mut self, skill_id: &str) -> Result<String, PortError> {
        let state = self.app.state::<AppState>();
        let session = require_session(&state).map_err(|e| PortError::Failed(e.to_string()))?;
        if skill_id.is_empty()
            || !skill_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(PortError::Denied(format!(
                "Ungültige Skill-Kennung: {skill_id}"
            )));
        }
        let root = crate::lock(&state.package_root)
            .map_err(|e| PortError::Failed(e.to_string()))?
            .clone();
        let skill = crate::instruction_skills::registry(&root)
            .read(skill_id)
            .ok_or_else(|| {
                PortError::Failed(format!(
                    "Der Anleitungs-Skill „{skill_id}“ ist nicht installiert."
                ))
            })?;
        let total = (session.context_tokens as usize).clamp(2_000, 12_000);
        let text =
            pa_skills::instructions::prompt_for(std::slice::from_ref(&skill), total / 2, total)
                .unwrap_or_default();
        Ok(format!(
            "Wende die folgende Anleitung auf den Text des Nutzers an. Antworte nur mit dem Ergebnis.\n\n{text}"
        ))
    }
}

// ---------------------------------------------------------------------------
// Definitionen
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn workflow_list(state: State<'_, AppState>) -> AppResult<Vec<WorkflowDefinition>> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(pa_vault::flow_store::list_workflows(
        vault.repository().connection(),
    )?)
}

/// Speichert eine Definition. Entwürfe dürfen ungültig sein; gestartet wird nur
/// nach bestandener Prüfung.
#[tauri::command]
pub fn workflow_save(
    state: State<'_, AppState>,
    definition: WorkflowDefinition,
) -> AppResult<WorkflowDefinition> {
    if definition.id.trim().is_empty() || definition.id.len() > 80 {
        return Err(invalid("Ungültige Kennung."));
    }
    if definition.name.trim().is_empty() || definition.name.chars().count() > 80 {
        return Err(invalid("Der Name muss 1 bis 80 Zeichen lang sein."));
    }
    if definition.graph.nodes.len() > MAX_NODES || definition.graph.edges.len() > MAX_EDGES {
        return Err(invalid("Der Ablauf ist zu groß."));
    }
    let mut definition = definition;
    definition.updated_unix_ms = now_unix_ms();
    let session = require_session(&state)?;
    {
        let mut vault = session.vault_runtime.lock()?;
        pa_vault::flow_store::save_workflow(vault.repository_mut().connection_mut(), &definition)?;
        let mut no_fault = pa_vault::hot_copy::NoFault;
        let _ = vault.sync(&mut no_fault);
    }
    Ok(definition)
}

#[tauri::command]
pub fn workflow_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let session = require_session(&state)?;
    let mut vault = session.vault_runtime.lock()?;
    pa_vault::flow_store::delete_workflow(vault.repository_mut().connection_mut(), &id)?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

#[tauri::command]
pub fn workflow_validate(graph: WorkflowGraph) -> WorkflowValidation {
    workflow::validate(&graph)
}

// ---------------------------------------------------------------------------
// Läufe
// ---------------------------------------------------------------------------

/// Fortschritt für die Live-Ansicht.
#[derive(Debug, Clone, Serialize)]
pub struct RunProgress {
    pub run_id: String,
    pub event: RunEvent,
    pub usage: RunUsage,
    pub current_node: Option<String>,
}

/// Kurzangabe eines früheren Laufs.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub run_id: String,
    pub status: RunStatus,
    pub started_unix_ms: i64,
}

fn persist_event(app: &AppHandle, run_id: &str, event: &RunEvent) {
    let state = app.state::<AppState>();
    if let Ok(session) = require_session(&state) {
        if let Ok(mut vault) = session.vault_runtime.lock() {
            if let Err(error) = pa_vault::flow_store::append_run_event(
                vault.repository_mut().connection_mut(),
                run_id,
                event,
            ) {
                eprintln!("Workflow: Protokolleintrag nicht gespeichert: {error}");
            }
        }
    }
}

/// Startet einen Lauf. `exa_approved` ist die ausdrückliche Freigabe genau dieses Laufs.
#[tauri::command(async)]
pub async fn workflow_start_run(app: AppHandle, request: StartRunRequest) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || start_blocking(&app, request))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

fn start_blocking(app: &AppHandle, request: StartRunRequest) -> AppResult<String> {
    let state = app.state::<AppState>();
    let session = require_session(&state)?;
    check_budget(&request.budget).map_err(invalid)?;
    if state.flow.workflows.any_running() {
        return Err(invalid(
            "Es läuft bereits ein Workflow. Auf diesem Rechner läuft immer nur einer.",
        ));
    }
    let definition = {
        let vault = session.vault_runtime.lock()?;
        pa_vault::flow_store::get_workflow(vault.repository().connection(), &request.workflow_id)?
            .ok_or_else(|| invalid("Der Workflow wurde nicht gefunden."))?
    };
    let validation = workflow::validate(&definition.graph);
    if !validation.ok {
        return Err(invalid(format!(
            "Der Ablauf ist nicht gültig: {}",
            validation
                .problems
                .iter()
                .take(3)
                .map(|p| p.message.clone())
                .collect::<Vec<_>>()
                .join(" ")
        )));
    }
    let used = connectors_used(&definition.graph);
    if !used.is_empty() {
        let config = lock(&state.connector_config)?.clone();
        let names = used
            .iter()
            .map(|c| connector_name(*c))
            .collect::<Vec<_>>()
            .join(", ");
        if config.offline_mode {
            return Err(invalid(format!(
                "Air Gap ist eingeschaltet: Dieser Ablauf braucht {names} und kann nicht starten. Schalte den Air Gap erst aus, wenn du das willst."
            )));
        }
        for connector in &used {
            connector_ready(&config, *connector)
                .map_err(|reason| invalid(format!("{reason} Richte ihn unter Konnektoren ein.")))?;
        }
        if !request.exa_approved {
            return Err(invalid(format!(
                "Die Freigabe für {names} in diesem Lauf fehlt. Sie gilt nur für diesen einen Lauf."
            )));
        }
        if request.budget.max_searches == 0 {
            return Err(invalid(
                "Dieser Ablauf braucht mindestens einen externen Aufruf im Limit.",
            ));
        }
    }

    let run_id = random_id("run");
    let started = now_unix_ms();
    {
        let mut vault = session.vault_runtime.lock()?;
        pa_vault::flow_store::create_run(
            vault.repository_mut().connection_mut(),
            &run_id,
            &definition,
            &request.budget,
            started,
        )?;
    }
    let cancel = state.flow.workflows.register(&run_id);
    let worker_app = app.clone();
    let worker_id = run_id.clone();
    let budget = request.budget;
    let inputs = request.inputs;
    let tz_offset_minutes = request.tz_offset_minutes;
    thread::Builder::new()
        .name(format!("iap-workflow-{run_id}"))
        .spawn(move || {
            run_thread(
                &worker_app,
                &worker_id,
                &definition,
                budget,
                cancel,
                inputs,
                tz_offset_minutes,
            );
        })
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(run_id)
}

fn run_thread(
    app: &AppHandle,
    run_id: &str,
    definition: &WorkflowDefinition,
    budget: RunBudget,
    cancel: Arc<AtomicBool>,
    inputs: std::collections::HashMap<String, String>,
    tz_offset_minutes: i32,
) {
    let state = app.state::<AppState>();
    let settings = RunSettings {
        run_id: run_id.to_owned(),
        workflow_id: definition.id.clone(),
        budget,
        // Die Suchbegriffe stammen aus der sichtbaren Definition, die der Nutzer im
        // Startdialog als öffentlich bestätigt hat – nie aus Chat, Dateien oder Gedächtnis.
        input_origin: DataOrigin::UserPublic,
        inputs,
    };
    let mut ports = AppPorts::new(
        app,
        run_id.to_owned(),
        budget.max_searches,
        Arc::clone(&cancel),
        tz_offset_minutes,
    );
    let sink_app = app.clone();
    let sink_id = run_id.to_owned();
    let result = catch_unwind(AssertUnwindSafe(|| {
        workflow::run(
            &definition.graph,
            &settings,
            &cancel,
            &mut ports,
            &mut |event: &RunEvent, usage: &RunUsage, current: Option<&str>| {
                persist_event(&sink_app, &sink_id, event);
                let _ = sink_app.emit(
                    "workflow-run-event",
                    RunProgress {
                        run_id: sink_id.clone(),
                        event: event.clone(),
                        usage: *usage,
                        current_node: current.map(str::to_owned),
                    },
                );
            },
        )
    }));
    // Die Freigabe endet mit dem Lauf.
    ports.grant.revoke();
    let (status, usage, outcome_result) = match result {
        Ok(outcome) => (outcome.status, outcome.usage, outcome.result),
        Err(_) => (RunStatus::Failed, RunUsage::default(), None),
    };
    if let Ok(session) = require_session(&state) {
        if let Ok(mut vault) = session.vault_runtime.lock() {
            let _ = pa_vault::flow_store::update_run(
                vault.repository_mut().connection_mut(),
                run_id,
                status,
                &usage,
                outcome_result.as_ref(),
                Some(now_unix_ms()),
            );
            let mut no_fault = pa_vault::hot_copy::NoFault;
            let _ = vault.sync(&mut no_fault);
        }
    }
    state.flow.workflows.finish(run_id);
    if let Some(report) = load_report(&state, run_id) {
        let _ = app.emit("workflow-run-finished", report);
    }
}

fn load_report(state: &AppState, run_id: &str) -> Option<WorkflowRunReport> {
    let session = require_session(state).ok()?;
    let vault = session.vault_runtime.lock().ok()?;
    pa_vault::flow_store::get_run_report(vault.repository().connection(), run_id)
        .ok()
        .flatten()
}

/// Der gerade laufende Lauf. Warum: Verlässt der Nutzer die Seite, läuft der Lauf
/// weiter; beim Zurückkommen soll er wieder sichtbar und abbrechbar sein.
#[tauri::command]
pub fn workflow_active_run(state: State<'_, AppState>) -> Option<String> {
    state.flow.workflows.active()
}

#[tauri::command]
pub fn workflow_cancel_run(state: State<'_, AppState>, run_id: String) -> bool {
    state.flow.workflows.cancel(&run_id)
}

#[tauri::command]
pub fn workflow_run_report(
    state: State<'_, AppState>,
    run_id: String,
) -> AppResult<Option<WorkflowRunReport>> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(pa_vault::flow_store::get_run_report(
        vault.repository().connection(),
        &run_id,
    )?)
}

#[tauri::command]
pub fn workflow_runs(
    state: State<'_, AppState>,
    workflow_id: String,
) -> AppResult<Vec<RunSummary>> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(
        pa_vault::flow_store::list_runs(vault.repository().connection(), &workflow_id)?
            .into_iter()
            .map(|(run_id, status, started_unix_ms)| RunSummary {
                run_id,
                status,
                started_unix_ms,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget() -> RunBudget {
        RunBudget {
            max_searches: 5,
            max_iterations: 2,
            max_seconds: 300,
            max_tokens: 4000,
            max_cost_usd: 0.5,
        }
    }

    #[test]
    fn budgets_have_hard_ceilings() {
        assert!(check_budget(&budget()).is_ok());
        for change in [
            |b: &mut RunBudget| b.max_searches = 51,
            |b: &mut RunBudget| b.max_iterations = 6,
            |b: &mut RunBudget| b.max_seconds = 5,
            |b: &mut RunBudget| b.max_seconds = 4000,
            |b: &mut RunBudget| b.max_tokens = 10,
            |b: &mut RunBudget| b.max_cost_usd = 9.0,
            |b: &mut RunBudget| b.max_cost_usd = f64::NAN,
            |b: &mut RunBudget| b.max_cost_usd = -1.0,
        ] {
            let mut b = budget();
            change(&mut b);
            assert!(check_budget(&b).is_err());
        }
    }

    #[test]
    fn cancel_all_reaches_every_registered_run() {
        let runtime = Runtime::new();
        let a = runtime.register("a");
        let b = runtime.register("b");
        assert!(runtime.any_running());
        assert_eq!(runtime.cancel_all(), 2);
        assert!(a.load(Ordering::SeqCst) && b.load(Ordering::SeqCst));
        assert!(runtime.cancel("a"));
        runtime.finish("a");
        runtime.finish("b");
        assert!(!runtime.any_running());
        assert!(!runtime.cancel("a"));
    }

    #[test]
    fn exa_use_is_detected_from_the_graph() {
        let graph = pa_types::flow::WorkflowGraph {
            version: 1,
            nodes: vec![pa_types::flow::WorkflowNode {
                id: "s".to_owned(),
                kind: NodeKind::ExaSearch { num_results: 3 },
                x: 0.0,
                y: 0.0,
            }],
            edges: vec![],
        };
        assert!(uses_exa(&graph));
        let local = pa_types::flow::WorkflowGraph {
            nodes: vec![],
            ..graph
        };
        assert!(!uses_exa(&local));
    }

    fn node_of(id: &str, kind: NodeKind) -> pa_types::flow::WorkflowNode {
        pa_types::flow::WorkflowNode {
            id: id.to_owned(),
            kind,
            x: 0.0,
            y: 0.0,
        }
    }

    #[test]
    fn a_graph_lists_each_external_service_once() {
        let graph = pa_types::flow::WorkflowGraph {
            version: 1,
            nodes: vec![
                node_of("a", NodeKind::ExaSearch { num_results: 3 }),
                node_of(
                    "b",
                    NodeKind::ExaContents {
                        max_characters: 1000,
                    },
                ),
                node_of(
                    "c",
                    NodeKind::WikipediaSearch {
                        num_results: 3,
                        lang: "de".into(),
                    },
                ),
                node_of("d", NodeKind::Weather { days: 2 }),
                node_of("e", NodeKind::BraveSearch { num_results: 3 }),
                node_of("f", NodeKind::Output),
            ],
            edges: vec![],
        };
        assert_eq!(
            connectors_used(&graph),
            vec![
                Connector::Exa,
                Connector::Wikipedia,
                Connector::OpenMeteo,
                Connector::Brave
            ]
        );
        let local = pa_types::flow::WorkflowGraph {
            nodes: vec![
                node_of("f", NodeKind::Output),
                node_of(
                    "c",
                    NodeKind::Calendar {
                        days_ahead: 1,
                        include_tasks: true,
                    },
                ),
            ],
            ..graph
        };
        assert!(connectors_used(&local).is_empty());
    }

    #[test]
    fn the_overview_names_the_reason_and_never_contains_a_key() {
        let config = ConnectorConfig {
            exa_enabled: true,
            exa_api_key: "GEHEIM".into(),
            wikipedia_enabled: true,
            ..ConnectorConfig::default()
        };
        let ok = connector_ready(&config, Connector::Exa);
        assert!(ok.is_ok());
        let missing = connector_ready(&config, Connector::Brave).unwrap_err();
        assert!(missing.contains("Brave") && !missing.contains("GEHEIM"));
    }

    #[test]
    fn a_connector_is_ready_only_when_enabled_and_keyed() {
        let mut config = ConnectorConfig::default();
        for connector in [
            Connector::Exa,
            Connector::Wikipedia,
            Connector::OpenMeteo,
            Connector::Brave,
        ] {
            assert!(connector_ready(&config, connector)
                .unwrap_err()
                .contains("ausgeschaltet"));
        }
        config.exa_enabled = true;
        config.brave_enabled = true;
        config.wikipedia_enabled = true;
        config.open_meteo_enabled = true;
        assert!(connector_ready(&config, Connector::Wikipedia).is_ok());
        assert!(
            connector_ready(&config, Connector::OpenMeteo).is_ok(),
            "ohne Schlüssel nutzbar"
        );
        assert!(connector_ready(&config, Connector::Exa)
            .unwrap_err()
            .contains("Schlüssel"));
        assert!(connector_ready(&config, Connector::Brave)
            .unwrap_err()
            .contains("Schlüssel"));
        config.exa_api_key = "  ".into();
        assert!(
            connector_ready(&config, Connector::Exa).is_err(),
            "Leerraum ist kein Schlüssel"
        );
        config.exa_api_key = "k".into();
        config.brave_api_key = "k".into();
        assert!(
            connector_ready(&config, Connector::Exa).is_ok()
                && connector_ready(&config, Connector::Brave).is_ok()
        );
    }
}
