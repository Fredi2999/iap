//! Werkzeug: Verzeichnisinhalt eines Unterordners im Workspace listen.

use std::path::PathBuf;

use pa_policy::{CapabilityAction, CapabilityRequest};
use serde_json::json;

use crate::{
    evaluate_and_audit, require_string, Tool, ToolContext, ToolError, ToolInvocation, ToolOutput,
    ToolSpec,
};

pub struct ListDirectoryTool;

impl Tool for ListDirectoryTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_directory".to_owned(),
            description: "Listet Dateien und Ordner eines Pfads relativ zum Workspace.".to_owned(),
            parameters_schema: json!({
                "type": "object",
                "required": ["path"],
                "properties": { "path": { "type": "string" } }
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
            action: CapabilityAction::FileList,
            relative_path: Some(relative.clone()),
            source: invocation.source,
            reason: format!("list_directory `{}`", relative.display()),
        };
        let capability = evaluate_and_audit(&invocation.name, &request, context)?;
        let path = capability
            .canonical_path
            .ok_or_else(|| ToolError::Invalid {
                tool: invocation.name.clone(),
                reason: "kein kanonischer Pfad".to_owned(),
            })?;
        let mut entries: Vec<String> = std::fs::read_dir(&path)
            .map_err(|source| ToolError::Internal {
                tool: invocation.name.clone(),
                source,
            })?
            .filter_map(|entry| entry.ok())
            .map(|entry| {
                let file_type = entry.file_type();
                let suffix = match file_type {
                    Ok(t) if t.is_dir() => "/",
                    Ok(t) if t.is_file() => "",
                    Ok(t) if t.is_symlink() => "*",
                    _ => "?",
                };
                format!("{}{suffix}", entry.file_name().to_string_lossy())
            })
            .collect();
        entries.sort();
        let content = if entries.is_empty() {
            "(leer)".to_owned()
        } else {
            entries.join("\n")
        };
        Ok(ToolOutput {
            tool: invocation.name.clone(),
            content,
            is_untrusted: true,
            truncated_from_bytes: None,
        })
    }
}
