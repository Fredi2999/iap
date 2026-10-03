//! Rundlauf- und Formtests der Verträge für Flow Version, Sprache, Bildschirm und Avatar.

use pa_types::{
    agent_flow::{BaseChoice, CandidateStatus, TestStatus, MAX_CANDIDATES, T0_DEFAULT_CANDIDATES},
    avatar::{AvatarState, JobInfo, JobKind, JobStatus, WindowMode},
    flow::{
        NodeKind, RunBudget, RunStatus, WorkflowEdge, WorkflowGraph, WorkflowNode,
        WORKFLOW_SCHEMA_VERSION,
    },
    screen::{ScreenBlock, ScreenTarget},
    voice::{VoiceCommand, VoiceState},
};

fn round_trip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    serde_json::from_str(&serde_json::to_string(value).expect("serialize")).expect("deserialize")
}

#[test]
fn workflow_graph_round_trips_and_uses_tagged_nodes() {
    let graph = WorkflowGraph {
        version: WORKFLOW_SCHEMA_VERSION,
        nodes: vec![
            WorkflowNode {
                id: "a".into(),
                kind: NodeKind::ManualStart,
                x: 0.0,
                y: 0.0,
            },
            WorkflowNode {
                id: "b".into(),
                kind: NodeKind::Condition { max_iterations: 3 },
                x: 120.5,
                y: 40.0,
            },
        ],
        edges: vec![WorkflowEdge {
            id: "e1".into(),
            from: "a".into(),
            from_port: "out".into(),
            to: "b".into(),
            to_port: "in".into(),
        }],
    };
    assert_eq!(round_trip(&graph), graph);
    let json = serde_json::to_value(&graph).unwrap();
    assert_eq!(json["nodes"][0]["kind"]["type"], "manual_start");
    assert_eq!(json["nodes"][1]["kind"]["type"], "condition");
}

#[test]
fn run_status_and_budget_use_snake_case() {
    assert_eq!(
        serde_json::to_string(&RunStatus::NotSufficientlySupported).unwrap(),
        "\"not_sufficiently_supported\""
    );
    let budget = RunBudget {
        max_searches: 6,
        max_iterations: 3,
        max_seconds: 300,
        max_tokens: 8_000,
        max_cost_usd: 0.25,
    };
    assert_eq!(round_trip(&budget), budget);
}

#[test]
fn agent_flow_limits_and_status_forms() {
    assert_eq!(MAX_CANDIDATES, 5);
    assert_eq!(T0_DEFAULT_CANDIDATES, 2);
    let status = CandidateStatus::Queued { position: 2 };
    assert_eq!(serde_json::to_value(&status).unwrap()["kind"], "queued");
    assert_eq!(round_trip(&status), status);
    let tests = TestStatus::NotRun {
        reason: "keine Sandbox".into(),
    };
    assert_eq!(round_trip(&tests), tests);
    let base = BaseChoice::Snapshot {
        include_untracked: vec!["neu.rs".into()],
    };
    assert_eq!(round_trip(&base), base);
}

#[test]
fn avatar_window_and_job_types_round_trip() {
    assert_eq!(
        serde_json::to_string(&AvatarState::AwaitingApproval).unwrap(),
        "\"awaiting_approval\""
    );
    assert_eq!(serde_json::to_string(&WindowMode::Pet).unwrap(), "\"pet\"");
    let job = JobInfo {
        id: "j1".into(),
        kind: JobKind::AgentFlow,
        label: "Kandidat 2".into(),
        status: JobStatus::Waiting,
        queue_position: Some(1),
        started_unix_ms: 1,
        cancelable: true,
    };
    assert_eq!(round_trip(&job), job);
}

#[test]
fn voice_and_screen_types_round_trip() {
    assert_eq!(
        serde_json::to_string(&VoiceState::Transcribing).unwrap(),
        "\"transcribing\""
    );
    assert_eq!(
        serde_json::to_string(&VoiceCommand::OpenWorkflows).unwrap(),
        "\"open_workflows\""
    );
    assert_eq!(
        serde_json::to_string(&ScreenBlock::NotSetUp).unwrap(),
        "\"not_set_up\""
    );
    let region = ScreenTarget::Region {
        monitor: 0,
        x: 10,
        y: 20,
        width: 300,
        height: 200,
    };
    assert_eq!(serde_json::to_value(&region).unwrap()["kind"], "region");
    assert_eq!(round_trip(&region), region);
}
