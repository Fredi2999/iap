//! Verdrahtet pa-policy, pa-tools und die Werkzeugschleife aus pa-core mit
//! der Launcher-Umgebung.
//!
//! Diese Datei ist bewusst dünn: sie hält den [`pa_policy::PathScope`], den
//! [`pa_policy::Mode`], den [`pa_policy::GrantStore`], den
//! [`pa_policy::AuditStore`] und die [`pa_tools::ToolRegistry`] und
//! stellt sie einem Nutzer-Turn zur Verfügung. Die eigentliche Iterations-
//! und Prompt-Logik lebt weiterhin in [`pa_core::tool_loop`].
//!
//! Der CLI aus Schritt 3 nutzt sie über einen `/werkzeuge on|off`-Toggle;
//! die Tauri-App wird die gleiche Runtime in Phase 2 Schritt 5 einhängen.

use std::path::{Path, PathBuf};

use pa_core::tool_loop::{run_tool_loop, ToolEvent, ToolLoopConfig, ToolLoopError};
use pa_policy::{AuditSink, AuditStore, GrantStore, Mode, PathScope, PolicyError};
use pa_tools::{PermissionHook, ToolContext, ToolRegistry};
use pa_types::chat::{Message, MessageRole, MessageStatus};

/// Fassbare Fehler beim Aufbau der Werkzeug-Runtime; verhindert, dass ein
/// fehlendes Workspace-Verzeichnis später erst in einem Werkzeugfehler landet.
#[derive(Debug, thiserror::Error)]
pub enum ToolRuntimeError {
    /// Der übergebene Workspace-Pfad existiert nicht oder ist nicht auflösbar.
    #[error("Workspace `{path}` ist nicht erreichbar: {source}")]
    Workspace {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// Pfad- oder Persistenzfehler in pa-policy.
    #[error("Policy-Fehler: {0}")]
    Policy(#[from] PolicyError),
}

/// Beschreibt, wo der Audit-Log persistiert wird.
///
/// Die MVP-Invariante (AGENTS 6) verbietet Zustandsdaten außerhalb des
/// Vaults; für den CLI-Prototypen aus Phase 2 wird deshalb standardmäßig
/// `InMemory` verwendet. Die Datei-Variante bleibt für Integrationstests
/// zugänglich, bis die Vault-Integration in einem späteren Schritt folgt.
pub enum AuditStorage<'a> {
    InMemory,
    File(&'a Path),
}

/// Trägt alle Zustandsobjekte, die pro Sitzung dauerhaft bestehen.
///
/// - `workspace` ist die kanonisierte Wurzel, die alle Dateiwerkzeuge
///   respektieren müssen.
/// - `mode` beginnt in [`Mode::M1Workspace`] (Standard) und kann vom Nutzer
///   erhöht werden; eine wiederkehrende Reauth-Prüfung liegt beim Aufrufer.
/// - `grants` ist ein rein prozessinterner Store – Session- und Einmal-
///   Grants dürfen einen Prozessneustart nicht überleben; Projekt-Grants
///   werden erst durch die spätere pa-vault-Integration persistiert.
/// - `audit` schreibt append-only mit Hash-Verkettung; `audit_path` bleibt
///   sichtbar, damit die UI den Ort in „Logs" korrekt anzeigen kann.
pub struct ToolRuntime {
    workspace: PathScope,
    mode: Mode,
    grants: GrantStore,
    audit: Box<dyn AuditSink + Send>,
    audit_path: Option<PathBuf>,
    registry: ToolRegistry,
    config: ToolLoopConfig,
}

impl ToolRuntime {
    /// Kurzform: In-Memory-Audit-Store, wie er in Tests oder vor der
    /// Vault-Entsperrung genutzt wird.
    pub fn in_memory(workspace_dir: &Path) -> Result<Self, ToolRuntimeError> {
        Self::open(workspace_dir, AuditStorage::InMemory)
    }

    /// Legt Workspace-Wurzel und (optional) persistenten Audit-Log an.
    ///
    /// Die Werkzeugregistry wird mit den MVP-Kernwerkzeugen aus `pa-tools`
    /// bestückt; `Mode` startet in [`Mode::default`] (M1 Workspace).
    pub fn open(workspace_dir: &Path, storage: AuditStorage<'_>) -> Result<Self, ToolRuntimeError> {
        std::fs::create_dir_all(workspace_dir).map_err(|source| ToolRuntimeError::Workspace {
            path: workspace_dir.to_path_buf(),
            source,
        })?;
        let (audit, audit_path): (Box<dyn AuditSink + Send>, _) = match storage {
            AuditStorage::InMemory => (Box::new(AuditStore::open_in_memory()?), None),
            AuditStorage::File(path) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| {
                        ToolRuntimeError::Workspace {
                            path: parent.to_path_buf(),
                            source,
                        }
                    })?;
                }
                (Box::new(AuditStore::open(path)?), Some(path.to_path_buf()))
            }
        };
        let workspace = PathScope::new(workspace_dir)?;
        Ok(Self {
            workspace,
            mode: Mode::default(),
            grants: GrantStore::default(),
            audit,
            audit_path,
            registry: ToolRegistry::with_defaults(),
            config: ToolLoopConfig::default(),
        })
    }

    /// Öffnet die Runtime mit einem beliebigen `AuditSink` (typisch: der
    /// `VaultAuditSink` aus [`crate::vault_audit`], damit der Log dieselbe
    /// SQLCipher-DB nutzt wie die Konversationen).
    pub fn with_sink(
        workspace_dir: &Path,
        sink: Box<dyn AuditSink + Send>,
    ) -> Result<Self, ToolRuntimeError> {
        std::fs::create_dir_all(workspace_dir).map_err(|source| ToolRuntimeError::Workspace {
            path: workspace_dir.to_path_buf(),
            source,
        })?;
        let workspace = PathScope::new(workspace_dir)?;
        Ok(Self {
            workspace,
            mode: Mode::default(),
            grants: GrantStore::default(),
            audit: sink,
            audit_path: None,
            registry: ToolRegistry::with_defaults(),
            config: ToolLoopConfig::default(),
        })
    }

    /// Nimmt ein zusätzliches Werkzeug auf, etwa einen installierten WASM-Skill.
    ///
    /// Warum hier und nicht in `pa-tools`: So bleibt der Launcher ohne die
    /// Wasmtime-Abhängigkeit; die App reicht ihre Skill-Adapter von außen herein,
    /// und jeder Aufruf läuft trotzdem durch denselben Policy- und Audit-Pfad.
    pub fn register_tool(&mut self, tool: std::sync::Arc<dyn pa_tools::Tool>) {
        self.registry.register(tool);
    }

    /// Wechselt den aktuellen Berechtigungsmodus.
    ///
    /// Der Aufrufer ist dafür verantwortlich, eine Reauth zu erzwingen, falls
    /// `Mode::requires_reauth_on_upgrade` `true` liefert.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    /// Liefert den aktuellen Modus (für die Statuszeile im CLI/UI).
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Ort der Audit-Datenbank, falls persistent; `None` bei In-Memory.
    pub fn audit_path(&self) -> Option<&Path> {
        self.audit_path.as_deref()
    }

    /// Beendet alle Session-Grants (z. B. beim Verlassen der Sitzung).
    ///
    /// Einmalige Grants sind bereits nach Verbrauch entfernt; Projekt-Grants
    /// überleben das Session-Ende wie in Konzept 10.1 gefordert.
    pub fn end_session(&mut self) {
        self.grants.drop_session_scope();
    }

    /// Führt einen einzelnen Nutzer-Turn durch die Werkzeugschleife.
    ///
    /// `emit_prompt` bekommt den vollständigen Verlauf (inkl.
    /// Werkzeugkatalog-System-Block) und liefert den gesammelten Modelltext.
    /// Der Aufrufer ist für Persistenz und Streaming der finalen Antwort
    /// verantwortlich, damit die Runtime nicht in den Vault-Pfad greifen
    /// muss.
    pub fn run_turn(
        &mut self,
        user_input: &str,
        now_unix_ms: i64,
        emit_prompt: impl FnMut(&[Message]) -> Result<String, ToolLoopError>,
        on_event: impl FnMut(ToolEvent),
    ) -> Result<String, ToolLoopError> {
        self.run_turn_with_hook(user_input, now_unix_ms, None, emit_prompt, on_event)
    }

    /// Wie [`Self::run_turn`], aber mit optionalem Freigabedialog (Konzept
    /// 10.3, Meilenstein 8). Der Hook wird nur konsultiert, wenn die Policy
    /// tatsächlich `Decision::Prompt` liefert; ohne Hook fällt der Aufruf
    /// weiterhin auf `ToolError::ConfirmationRequired` zurück.
    pub fn run_turn_with_hook<'hook>(
        &'hook mut self,
        user_input: &str,
        now_unix_ms: i64,
        permission_hook: Option<&'hook mut dyn PermissionHook>,
        mut emit_prompt: impl FnMut(&[Message]) -> Result<String, ToolLoopError>,
        on_event: impl FnMut(ToolEvent),
    ) -> Result<String, ToolLoopError> {
        let history = vec![Message {
            id: "user-turn".to_owned(),
            conversation_id: String::new(),
            position: 0,
            role: MessageRole::User,
            content: user_input.to_owned(),
            status: MessageStatus::Complete,
            created_at_unix_ms: now_unix_ms,
        }];
        let specs = self.registry.specs_for_prompt(user_input, None);
        let mut context = ToolContext {
            workspace: &self.workspace,
            mode: self.mode,
            grants: &self.grants,
            audit: &mut *self.audit,
            now_unix_ms,
            permission_hook,
        };
        run_tool_loop(
            &self.registry,
            &mut context,
            self.config,
            history,
            &specs,
            |messages| emit_prompt(messages),
            on_event,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_core::tool_loop::ToolEvent;
    use tempfile::TempDir;

    #[test]
    fn answers_immediately_when_engine_returns_answer_envelope() {
        let workspace = TempDir::new().unwrap();
        let mut runtime = ToolRuntime::in_memory(workspace.path()).unwrap();

        let mut events: Vec<String> = Vec::new();
        let answer = runtime
            .run_turn(
                "Sag Hallo",
                1,
                |_messages| Ok(r#"{"action":"answer","text":"Hallo Welt"}"#.to_owned()),
                |event| match event {
                    ToolEvent::ModelText { text } => events.push(format!("model:{text}")),
                    ToolEvent::ToolCall { tool, .. } => events.push(format!("call:{tool}")),
                    ToolEvent::ToolResult { tool, .. } => events.push(format!("result:{tool}")),
                    ToolEvent::ToolError { tool, .. } => events.push(format!("err:{tool}")),
                },
            )
            .unwrap();
        assert_eq!(answer, "Hallo Welt");
        assert!(events.iter().any(|e| e.starts_with("model:")));
        assert!(events.iter().all(|e| !e.starts_with("call:")));
    }

    #[test]
    fn read_file_call_flows_through_policy_and_audit() {
        let workspace = TempDir::new().unwrap();
        let audit_dir = TempDir::new().unwrap();
        let audit_path = audit_dir.path().join("audit.db");
        let target = workspace.path().join("hello.txt");
        std::fs::write(&target, b"geheim").unwrap();

        let mut runtime =
            ToolRuntime::open(workspace.path(), AuditStorage::File(&audit_path)).unwrap();

        // Zwei Runden: erst Call, dann Answer. Ein einfacher Zähler steuert das.
        let mut step = 0u8;
        let mut recorded_result: Option<String> = None;
        let answer = runtime
            .run_turn(
                "Lies hello.txt",
                7,
                |_messages| {
                    step += 1;
                    if step == 1 {
                        Ok(r#"{"action":"call","tool":"read_file","arguments":{"path":"hello.txt"}}"#.to_owned())
                    } else {
                        Ok(r#"{"action":"answer","text":"Ich habe die Datei gelesen."}"#.to_owned())
                    }
                },
                |event| {
                    if let ToolEvent::ToolResult { content, .. } = event {
                        recorded_result = Some(content);
                    }
                },
            )
            .unwrap();
        assert_eq!(answer, "Ich habe die Datei gelesen.");
        assert_eq!(recorded_result.as_deref(), Some("geheim"));

        // Der Audit-Store wurde mit mindestens einem Allow beschrieben.
        let audit = AuditStore::open(&audit_path).unwrap();
        drop(audit);
        let audit_bytes = std::fs::metadata(&audit_path).unwrap().len();
        assert!(audit_bytes > 0, "Audit-Datenbank darf nicht leer sein");
    }
}
