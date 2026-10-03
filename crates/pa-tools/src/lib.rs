//! Kern-Werkzeuge für PortableAI.
//!
//! Jedes Werkzeug hat einen `ToolSpec` (Definition für den Prompt) und wird
//! über [`Tool::invoke`] ausgeführt. Datei- und Suchwerkzeuge rufen
//! ausschließlich [`pa_policy::evaluate`] und [`pa_policy::safe_join`]; ein
//! direkter `std::fs`-Aufruf abseits davon ist ein Bug und wird durch die
//! Tests in [`crates/pa-policy/tests/path_attacks.rs`](../pa-policy/tests/path_attacks.rs)
//! nur mittelbar, aber verlässlich abgefangen.

pub mod calculator;
pub mod clock;
pub mod file_read;
pub mod file_write;
pub mod list_dir;
pub mod registry;
pub mod rubric;
pub mod search;

use std::collections::BTreeMap;

use pa_policy::PolicyError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub use calculator::CalculatorTool;
pub use clock::ClockTool;
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use list_dir::ListDirectoryTool;
pub use registry::ToolRegistry;
pub use search::WorkspaceSearchTool;

/// Trennt vertrauensrelevante Kategorien beim Aufruf und in der Antwort.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Werkzeug `{tool}`: {0}", .reason)]
    Invalid { tool: String, reason: String },
    #[error("Werkzeug `{tool}` durch Policy verweigert: {0}", .reason)]
    Denied { tool: String, reason: String },
    #[error("Werkzeug `{tool}` benötigt Bestätigung: {0}", .reason)]
    ConfirmationRequired { tool: String, reason: String },
    #[error("Werkzeug `{tool}` intern: {source}")]
    Internal {
        tool: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Policy-Fehler: {0}")]
    Policy(#[from] PolicyError),
}

/// Beschreibung eines Werkzeugs, wie sie an das Modell übergeben wird.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON-Schema der Parameter (offen im Vertrag; die Werkzeuge validieren selbst).
    pub parameters_schema: Value,
    /// Kurzer Kategoriehinweis für die dynamische Werkzeugauswahl aus 8.4.
    pub category: String,
}

/// Aufruf eines Werkzeugs nach dem Parsen der Modellantwort.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub name: String,
    pub arguments: BTreeMap<String, Value>,
    /// Herkunft dieses Aufrufs (Nutzer oder Fremdinhalt). Wichtig für die Policy.
    pub source: pa_policy::DerivationSource,
}

/// Ergebnis eines Werkzeugaufrufs.
///
/// `content` ist der für das Modell sichtbare Text; `is_untrusted` markiert
/// ihn als extern und verhindert im Orchestrator, dass Auto-Freigaben auf
/// darin genannte Ziele ausgedehnt werden (Konzept 10.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutput {
    pub tool: String,
    pub content: String,
    pub is_untrusted: bool,
    pub truncated_from_bytes: Option<u64>,
}

impl ToolOutput {
    /// Kürzt Text auf `max_chars` Zeichen und setzt die Trunkierungsangabe.
    pub fn truncate(mut self, max_chars: usize) -> Self {
        if self.content.chars().count() <= max_chars {
            return self;
        }
        let truncated: String = self.content.chars().take(max_chars).collect();
        let original = self.content.len() as u64;
        self.content = format!("{truncated}\n[Werkzeugausgabe gekürzt]");
        self.truncated_from_bytes = Some(original);
        self
    }
}

/// Vertrag aller Werkzeuge; wird sowohl von den Kern-Skills als auch vom
/// späteren WASM-Host bedient.
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError>;
}

/// Kontext, den der Orchestrator jedem Werkzeugaufruf mitgibt.
///
/// `audit` ist bewusst `&mut dyn AuditSink`, damit derselbe Werkzeugpfad je
/// nach Aufrufer entweder den prozessinternen `AuditStore` (Tests, MVP-CLI
/// bevor der Vault existiert) oder eine Vault-persistente Bridge nutzt.
///
/// `permission_hook` (Konzept 10.3, Meilenstein 8) darf leer bleiben — dann
/// gilt eine `Decision::Prompt` als „Werkzeug-Fehler ConfirmationRequired"
/// wie im MVP. Ist ein Hook gesetzt, wird er statt des Fehlers gefragt; ein
/// `Allow` schaltet das Werkzeug frei (die Freigabe zählt dann rückwirkend
/// als `UserIntent`), ein `Deny` beendet den Aufruf sauber mit Audit-Deny.
pub struct ToolContext<'a> {
    pub workspace: &'a pa_policy::PathScope,
    pub mode: pa_policy::Mode,
    pub grants: &'a pa_policy::GrantStore,
    /// Wird nach Ausführung aktualisiert: Einmal-Grants werden konsumiert,
    /// Ergebnisse können neue Auto-Grants verlangen (aktuell keine).
    pub audit: &'a mut dyn pa_policy::AuditSink,
    pub now_unix_ms: i64,
    /// Optionaler Freigabedialog. Leer = Prompt wird als Fehler zurückgemeldet.
    pub permission_hook: Option<&'a mut dyn PermissionHook>,
}

/// Nutzerantwort auf einen Werkzeug-Freigabedialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionAnswer {
    pub allow: bool,
    /// Zeigt an, dass die Freigabe für die Session merken werden soll. Der
    /// Aufrufer entscheidet, ob er daraus einen persistenten Grant baut.
    pub remember_for_session: bool,
}

/// Rendezvous-Callback für Werkzeug-Freigaben (Konzept 10.3).
///
/// Aufrufer sind für das Blockieren des Turn-Threads verantwortlich; die
/// UI wird über einen separaten Kanal informiert (Tauri-Event
/// `PermissionRequested`) und schickt die Antwort synchron zurück.
pub trait PermissionHook: Send {
    fn ask(
        &mut self,
        tool: &str,
        request: &pa_policy::CapabilityRequest,
        reason: &str,
    ) -> Result<PermissionAnswer, ToolError>;
}

/// Hilfsfunktion: verlangt einen Parameter als String und schlägt sonst
/// mit `ToolError::Invalid` fehl.
pub fn require_string(invocation: &ToolInvocation, key: &str) -> Result<String, ToolError> {
    invocation
        .arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ToolError::Invalid {
            tool: invocation.name.clone(),
            reason: format!("Parameter `{key}` fehlt oder ist kein String"),
        })
}

/// Reicht Policy-Entscheidungen und Audit-Log an die Werkzeuge weiter.
///
/// Bei `Decision::Prompt` wird — falls ein [`PermissionHook`] am Kontext
/// hängt — der Nutzer synchron gefragt. Ein `allow=true` schaltet die
/// Aktion frei (rückwirkend als `UserIntent`), ein `allow=false` führt zum
/// Deny; beide Wege werden im Audit-Log getrennt vermerkt.
pub fn evaluate_and_audit(
    tool_name: &str,
    request: &pa_policy::CapabilityRequest,
    context: &mut ToolContext<'_>,
) -> Result<pa_policy::Capability, ToolError> {
    let decision =
        pa_policy::capability::evaluate(request, context.mode, context.workspace, context.grants)?;
    let (outcome, capability, error) = match &decision {
        pa_policy::Decision::Allow(cap) => (
            pa_policy::audit::AuditOutcome::Allow,
            Some(cap.clone()),
            None,
        ),
        pa_policy::Decision::Prompt(reason) => {
            if let Some(hook) = context.permission_hook.as_deref_mut() {
                let answer = hook.ask(tool_name, request, reason)?;
                if answer.allow {
                    // Re-evaluate mit `UserIntent`, damit die reguläre
                    // Allow-Prüfung greift und die Capability sauber entsteht.
                    let promoted = pa_policy::CapabilityRequest {
                        source: pa_policy::DerivationSource::UserIntent,
                        ..request.clone()
                    };
                    let promoted_decision = pa_policy::capability::evaluate(
                        &promoted,
                        context.mode,
                        context.workspace,
                        context.grants,
                    )?;
                    match promoted_decision {
                        pa_policy::Decision::Allow(cap) => {
                            (pa_policy::audit::AuditOutcome::Allow, Some(cap), None)
                        }
                        pa_policy::Decision::Prompt(inner) | pa_policy::Decision::Deny(inner) => (
                            pa_policy::audit::AuditOutcome::Deny,
                            None,
                            Some(ToolError::Denied {
                                tool: tool_name.to_owned(),
                                reason: format!(
                                    "Freigabe erteilt, aber Modus/Path erlaubt sie nicht: {inner}"
                                ),
                            }),
                        ),
                    }
                } else {
                    (
                        pa_policy::audit::AuditOutcome::Deny,
                        None,
                        Some(ToolError::Denied {
                            tool: tool_name.to_owned(),
                            reason: format!("Nutzer hat Freigabe verweigert: {reason}"),
                        }),
                    )
                }
            } else {
                (
                    pa_policy::audit::AuditOutcome::Prompt,
                    None,
                    Some(ToolError::ConfirmationRequired {
                        tool: tool_name.to_owned(),
                        reason: reason.clone(),
                    }),
                )
            }
        }
        pa_policy::Decision::Deny(reason) => (
            pa_policy::audit::AuditOutcome::Deny,
            None,
            Some(ToolError::Denied {
                tool: tool_name.to_owned(),
                reason: reason.clone(),
            }),
        ),
    };
    context.audit.append(
        pa_policy::AuditLog {
            mode: context.mode,
            action: request.action,
            target: request
                .relative_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            outcome,
            reason: request.reason.clone(),
        },
        context.now_unix_ms,
    )?;
    match (capability, error) {
        (Some(cap), None) => Ok(cap),
        (None, Some(err)) => Err(err),
        _ => unreachable!("evaluate liefert genau eines der beiden Ergebnisse"),
    }
}
