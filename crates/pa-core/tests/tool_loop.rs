use std::cell::RefCell;

use pa_core::tool_loop::{run_tool_loop, ToolEvent, ToolLoopConfig, ToolLoopError};
use pa_policy::{AuditStore, GrantStore, Mode, PathScope};
use pa_tools::{ToolContext, ToolRegistry};

fn workspace_context() -> (tempfile::TempDir, PathScope, GrantStore, AuditStore) {
    let temp = tempfile::TempDir::with_prefix("pa-core-loop").unwrap();
    std::fs::write(temp.path().join("hello.txt"), "Hallo Welt").unwrap();
    let scope = PathScope::new(temp.path()).unwrap();
    (
        temp,
        scope,
        GrantStore::default(),
        AuditStore::open_in_memory().unwrap(),
    )
}

#[test]
fn happy_path_calls_read_file_then_answers() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M1Workspace,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let registry = ToolRegistry::with_defaults();
    let scripted = RefCell::new(vec![
        r#"{"action":"call","tool":"read_file","arguments":{"path":"hello.txt"}}"#.to_owned(),
        r#"{"action":"answer","text":"Datei enthält Hallo Welt."}"#.to_owned(),
    ]);
    let events = RefCell::new(Vec::<ToolEvent>::new());
    let final_text = run_tool_loop(
        &registry,
        &mut ctx,
        ToolLoopConfig::default(),
        Vec::new(),
        &registry.all_specs(),
        |_history| Ok(scripted.borrow_mut().remove(0)),
        |event| events.borrow_mut().push(event),
    )
    .unwrap();
    assert_eq!(final_text, "Datei enthält Hallo Welt.");
    let events = events.borrow();
    assert!(matches!(events[1], ToolEvent::ToolCall { .. }));
    assert!(matches!(
        events[2],
        ToolEvent::ToolResult {
            is_untrusted: true,
            ..
        }
    ));
}

#[test]
fn parse_error_on_first_iteration_returns_error() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M1Workspace,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let registry = ToolRegistry::with_defaults();
    let err = run_tool_loop(
        &registry,
        &mut ctx,
        ToolLoopConfig::default(),
        Vec::new(),
        &registry.all_specs(),
        |_history| Ok("kein JSON".into()),
        |_| {},
    )
    .unwrap_err();
    assert!(matches!(err, ToolLoopError::Parse(_)));
}

#[test]
fn max_iterations_returns_last_text_gracefully() {
    let (_temp, scope, grants, mut audit) = workspace_context();
    let mut ctx = ToolContext {
        workspace: &scope,
        mode: Mode::M1Workspace,
        grants: &grants,
        audit: &mut audit,
        now_unix_ms: 1,
        permission_hook: None,
    };
    let registry = ToolRegistry::with_defaults();
    // Modell ruft immer wieder dasselbe Werkzeug auf; wir setzen die Grenze auf 2.
    let final_text = run_tool_loop(
        &registry,
        &mut ctx,
        ToolLoopConfig {
            max_iterations: 2,
            max_tool_output_chars: 4096,
        },
        Vec::new(),
        &registry.all_specs(),
        |_history| {
            Ok(
                r#"{"action":"call","tool":"read_file","arguments":{"path":"hello.txt"}}"#
                    .to_owned(),
            )
        },
        |_| {},
    )
    .unwrap();
    assert!(final_text.contains("max_iterations") || final_text.contains("\"call\""));
    // letzte Modellrunde bleibt sichtbar
}
