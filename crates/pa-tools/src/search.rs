//! Werkzeug: Textsuche im Workspace.
//!
//! Rekursive Substring-Suche mit Zeilen- und Zeichenlimit. Regex kommt später;
//! im MVP wird bewusst keine Volltextindex-Engine (Tantivy/Bleve) mitgeliefert.

use std::path::PathBuf;

use pa_policy::{CapabilityAction, CapabilityRequest};
use serde_json::json;

use crate::{
    evaluate_and_audit, require_string, Tool, ToolContext, ToolError, ToolInvocation, ToolOutput,
    ToolSpec,
};

const MAX_HITS: usize = 40;
const MAX_LINE_LEN: usize = 200;

pub struct WorkspaceSearchTool;

impl Tool for WorkspaceSearchTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "workspace_search".to_owned(),
            description:
                "Rekursive Substring-Suche im Workspace. Antwortet mit bis zu 40 Treffern im Format `pfad:zeile:zeilentext`."
                    .to_owned(),
            parameters_schema: json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "subdir": { "type": "string", "description": "Optional: Unterordner relativ zum Workspace" }
                }
            }),
            category: "search".to_owned(),
        }
    }

    fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let query = require_string(invocation, "query")?;
        if query.is_empty() {
            return Err(ToolError::Invalid {
                tool: invocation.name.clone(),
                reason: "leere Suchanfrage".to_owned(),
            });
        }
        let subdir = invocation
            .arguments
            .get("subdir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let request = CapabilityRequest {
            action: CapabilityAction::FileList,
            relative_path: Some(subdir.clone()),
            source: invocation.source,
            reason: format!("workspace_search `{query}` in `{}`", subdir.display()),
        };
        let capability = evaluate_and_audit(&invocation.name, &request, context)?;
        let base = capability
            .canonical_path
            .ok_or_else(|| ToolError::Invalid {
                tool: invocation.name.clone(),
                reason: "kein kanonischer Pfad".to_owned(),
            })?;
        let mut hits = Vec::new();
        walk_and_search(&base, &query, context.workspace.root(), &mut hits);
        let content = if hits.is_empty() {
            "(keine Treffer)".to_owned()
        } else {
            hits.join("\n")
        };
        Ok(ToolOutput {
            tool: invocation.name.clone(),
            content,
            is_untrusted: true,
            truncated_from_bytes: None,
        })
    }
}

fn walk_and_search(
    directory: &std::path::Path,
    needle: &str,
    workspace_root: &std::path::Path,
    hits: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if hits.len() >= MAX_HITS {
            return;
        }
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            walk_and_search(&path, needle, workspace_root, hits);
        } else if metadata.is_file() && metadata.len() < 512 * 1024 {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                if line.contains(needle) {
                    let rel = path
                        .strip_prefix(workspace_root)
                        .unwrap_or(&path)
                        .to_string_lossy();
                    let display: String = line.chars().take(MAX_LINE_LEN).collect();
                    hits.push(format!("{rel}:{}:{display}", index + 1));
                    if hits.len() >= MAX_HITS {
                        return;
                    }
                }
            }
        }
    }
}
