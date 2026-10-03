//! Werkzeug: Dateiinhalt aus dem Workspace lesen.
//!
//! Der Rückgabewert wird immer als `is_untrusted = true` markiert, damit der
//! Orchestrator ihn im Prompt als externen Inhalt kennzeichnet und keine
//! automatischen Freigaben auf darin genannte Ziele ausdehnt (Konzept 10.3).

use std::path::PathBuf;

use pa_policy::{CapabilityAction, CapabilityRequest};
use serde_json::json;

use crate::{
    evaluate_and_audit, require_string, Tool, ToolContext, ToolError, ToolInvocation, ToolOutput,
    ToolSpec,
};

const MAX_CHARS: usize = 16 * 1024;

/// Liest maximal 16 000 Zeichen einer Datei und liefert sie als UTF-8 zurück.
pub struct FileReadTool;

impl Tool for FileReadTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".to_owned(),
            description:
                "Liest eine Datei relativ zum Workspace. Antwortlänge ist auf 16k Zeichen begrenzt."
                    .to_owned(),
            parameters_schema: json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string", "description": "Pfad relativ zum Workspace" }
                }
            }),
            category: "files".to_owned(),
        }
    }

    fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let relative = PathBuf::from(require_string(invocation, "path")?);
        let request = CapabilityRequest {
            action: CapabilityAction::FileRead,
            relative_path: Some(relative.clone()),
            source: invocation.source,
            reason: format!("read_file `{}`", relative.display()),
        };
        let capability = evaluate_and_audit(&invocation.name, &request, context)?;
        let path = capability
            .canonical_path
            .ok_or_else(|| ToolError::Invalid {
                tool: invocation.name.clone(),
                reason: "kein kanonischer Pfad".to_owned(),
            })?;
        let bytes = std::fs::read(&path).map_err(|source| ToolError::Internal {
            tool: invocation.name.clone(),
            source,
        })?;
        let content = String::from_utf8_lossy(&bytes).into_owned();
        Ok(ToolOutput {
            tool: invocation.name.clone(),
            content,
            is_untrusted: true,
            truncated_from_bytes: None,
        }
        .truncate(MAX_CHARS))
    }
}
