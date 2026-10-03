use std::{collections::BTreeMap, fs};

use pa_policy::{AuditStore, DerivationSource, GrantStore, Mode, PathScope};
use pa_tools::{
    CalculatorTool, ClockTool, FileReadTool, FileWriteTool, ListDirectoryTool, Tool, ToolContext,
    ToolError, ToolInvocation, WorkspaceSearchTool,
};
use serde_json::Value;

fn workspace_context() -> (tempfile::TempDir, PathScope, GrantStore, AuditStore) {
    let temp = tempfile::TempDir::with_prefix("pa-tools").expect("temp");
    fs::write(temp.path().join("notes.txt"), "Ziel: notes\ninhalt").unwrap();
    fs::create_dir_all(temp.path().join("sub")).unwrap();
    fs::write(temp.path().join("sub/geheim.txt"), "geheim").unwrap();
    let scope = PathScope::new(temp.path()).unwrap();
    let grants = GrantStore::default();
    let audit = AuditStore::open_in_memory().unwrap();
    (temp, scope, grants, audit)
}

fn invoke(
    tool: &dyn Tool,
    mut args: BTreeMap<String, Value>,
    source: DerivationSource,
    ctx: &mut ToolContext<'_>,
    name: &str,
) -> Result<pa_tools::ToolOutput, ToolError> {
    let invocation = ToolInvocation {
        name: name.to_owned(),
        arguments: std::mem::take(&mut args),
        source,
    };
    tool.invoke(&invocation, ctx)
}

#[test]
fn read_file_returns_untrusted_content() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M1Workspace,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let mut args = BTreeMap::new();
    args.insert("path".into(), Value::String("notes.txt".into()));
    let output = invoke(
        &FileReadTool,
        args,
        DerivationSource::UserIntent,
        &mut ctx,
        "read_file",
    )
    .unwrap();
    assert!(output.is_untrusted);
    assert!(output.content.contains("Ziel: notes"));
    assert_eq!(audit.all().unwrap().len(), 1);
}

#[test]
fn write_file_produces_diff_without_apply() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M1Workspace,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let mut args = BTreeMap::new();
    args.insert("path".into(), Value::String("notes.txt".into()));
    args.insert("content".into(), Value::String("neue\nnotiz".into()));
    let output = invoke(
        &FileWriteTool,
        args,
        DerivationSource::UserIntent,
        &mut ctx,
        "write_file",
    )
    .unwrap();
    assert!(!output.is_untrusted);
    assert!(output.content.starts_with("Vorschau:"));
}

#[test]
fn write_file_apply_actually_writes() {
    let (temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M1Workspace,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let mut args = BTreeMap::new();
    args.insert("path".into(), Value::String("new.txt".into()));
    args.insert("content".into(), Value::String("Hallo".into()));
    args.insert("apply".into(), Value::Bool(true));
    let output = invoke(
        &FileWriteTool,
        args,
        DerivationSource::UserIntent,
        &mut ctx,
        "write_file",
    )
    .unwrap();
    assert!(output.content.starts_with("Geschrieben:"));
    let written = fs::read_to_string(temp.path().join("new.txt")).unwrap();
    assert_eq!(written, "Hallo");
}

#[test]
fn write_file_in_m0_is_denied_and_audited() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M0Observe,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let mut args = BTreeMap::new();
    args.insert("path".into(), Value::String("notes.txt".into()));
    args.insert("content".into(), Value::String("neu".into()));
    let err = invoke(
        &FileWriteTool,
        args,
        DerivationSource::UserIntent,
        &mut ctx,
        "write_file",
    )
    .unwrap_err();
    assert!(matches!(err, ToolError::Denied { .. }));
    let entries = audit.all().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].outcome, pa_policy::audit::AuditOutcome::Deny);
}

#[test]
fn untrusted_write_source_prompts_even_in_m3() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M3Autonomous,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let mut args = BTreeMap::new();
    args.insert("path".into(), Value::String("notes.txt".into()));
    args.insert("content".into(), Value::String("aus_datei".into()));
    let err = invoke(
        &FileWriteTool,
        args,
        DerivationSource::UntrustedContent,
        &mut ctx,
        "write_file",
    )
    .unwrap_err();
    assert!(matches!(err, ToolError::ConfirmationRequired { .. }));
}

#[test]
fn workspace_search_finds_hits_and_marks_untrusted() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M1Workspace,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let mut args = BTreeMap::new();
    args.insert("query".into(), Value::String("Ziel".into()));
    let output = invoke(
        &WorkspaceSearchTool,
        args,
        DerivationSource::UserIntent,
        &mut ctx,
        "workspace_search",
    )
    .unwrap();
    assert!(output.is_untrusted);
    assert!(output.content.contains("notes.txt"));
}

#[test]
fn list_directory_returns_sorted_entries() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M1Workspace,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let mut args = BTreeMap::new();
    args.insert("path".into(), Value::String(".".into()));
    let output = invoke(
        &ListDirectoryTool,
        args,
        DerivationSource::UserIntent,
        &mut ctx,
        "list_directory",
    )
    .unwrap();
    let lines: Vec<&str> = output.content.lines().collect();
    assert!(lines.contains(&"notes.txt"));
    assert!(lines.contains(&"sub/"));
}

#[test]
fn calculator_evaluates_pure_and_audits_pure_action() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M0Observe,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let mut args = BTreeMap::new();
    args.insert("expression".into(), Value::String("2+3*4".into()));
    let output = invoke(
        &CalculatorTool,
        args,
        DerivationSource::UserIntent,
        &mut ctx,
        "calculator",
    )
    .unwrap();
    assert_eq!(output.content, "14");
    let entries = audit.all().unwrap();
    assert_eq!(entries[0].action, pa_policy::CapabilityAction::Pure);
    assert_eq!(entries[0].outcome, pa_policy::audit::AuditOutcome::Allow);
}

#[test]
fn now_tool_formats_context_time_as_iso_utc() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M1Workspace,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 0,
        permission_hook: None,
    };
    let output = invoke(
        &ClockTool,
        BTreeMap::new(),
        DerivationSource::UserIntent,
        &mut ctx,
        "now",
    )
    .unwrap();
    assert_eq!(output.content, "1970-01-01T00:00:00.000Z");
}
