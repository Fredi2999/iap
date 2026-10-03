//! Werkzeug: Datei im Workspace mit Diff-Vorschau schreiben.
//!
//! Statt sofort zu schreiben, liefert das Werkzeug einen Text-Diff als
//! Ergebnis; die tatsächliche Schreiboperation läuft nur, wenn `apply=true`
//! übergeben wird und die Policy Allow zurückgibt. Damit ist auch bei
//! irrtümlicher Bestätigung noch eine Zwischenkontrolle möglich.

use std::path::PathBuf;

use pa_policy::{CapabilityAction, CapabilityRequest};
use serde_json::json;

use crate::{
    evaluate_and_audit, require_string, Tool, ToolContext, ToolError, ToolInvocation, ToolOutput,
    ToolSpec,
};

pub struct FileWriteTool;

impl Tool for FileWriteTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".to_owned(),
            description:
                "Erzeugt einen Text-Diff für eine geplante Schreiboperation. `apply=true` schreibt tatsächlich."
                    .to_owned(),
            parameters_schema: json!({
                "type": "object",
                "required": ["path", "content"],
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "apply": { "type": "boolean", "default": false }
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
        let content = require_string(invocation, "content")?;
        let apply = invocation
            .arguments
            .get("apply")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let request = CapabilityRequest {
            action: CapabilityAction::FileWrite,
            relative_path: Some(relative.clone()),
            source: invocation.source,
            reason: format!(
                "write_file `{}` ({} Zeichen, apply={apply})",
                relative.display(),
                content.chars().count()
            ),
        };
        let capability = evaluate_and_audit(&invocation.name, &request, context)?;
        let path = capability
            .canonical_path
            .ok_or_else(|| ToolError::Invalid {
                tool: invocation.name.clone(),
                reason: "kein kanonischer Pfad".to_owned(),
            })?;
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let diff = simple_diff(&existing, &content);
        if apply {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|source| ToolError::Internal {
                    tool: invocation.name.clone(),
                    source,
                })?;
            }
            std::fs::write(&path, &content).map_err(|source| ToolError::Internal {
                tool: invocation.name.clone(),
                source,
            })?;
        }
        let header = if apply { "Geschrieben:" } else { "Vorschau:" };
        Ok(ToolOutput {
            tool: invocation.name.clone(),
            content: format!("{header}\n{diff}"),
            is_untrusted: false,
            truncated_from_bytes: None,
        })
    }
}

/// Sehr einfache zeilenweise Diff-Darstellung. Für Chatzwecke ausreichend; ein
/// echter LCS-Diff kommt später mit dem Code-Bereich (Konzept 9.4).
fn simple_diff(before: &str, after: &str) -> String {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    let max = before_lines.len().max(after_lines.len());
    let mut output = String::new();
    for i in 0..max {
        match (before_lines.get(i), after_lines.get(i)) {
            (Some(a), Some(b)) if a == b => output.push_str(&format!("  {a}\n")),
            (Some(a), Some(b)) => {
                output.push_str(&format!("- {a}\n"));
                output.push_str(&format!("+ {b}\n"));
            }
            (Some(a), None) => output.push_str(&format!("- {a}\n")),
            (None, Some(b)) => output.push_str(&format!("+ {b}\n")),
            (None, None) => {}
        }
    }
    output
}
