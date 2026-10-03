//! Modell-Evals gegen einen echten `llama-server` (siehe evals/README.md).
//!
//! Ignoriert, weil das ein Modell und das Programm braucht und Minuten dauert. Aufruf:
//!   IAP_EVAL_SERVER=<llama-server.exe> IAP_EVAL_MODEL=<modell.gguf> IAP_EVAL_FAMILY=gemma4 \
//!   cargo test -p pa-launcher --test model_eval -- --ignored --nocapture
//! Optional: IAP_EVAL_THREADS (Standard 4), IAP_EVAL_MIN_PASS (Standard 0.7),
//! IAP_EVAL_OUT (Datei für den Bericht als JSON).
//!
//! Geprüft wird mit demselben Adapter, derselben Werkzeug-Hülle und derselben Grammatik wie in der
//! App. Der Lauf selbst wurde bisher nicht gegen ein echtes Modell ausgeführt (nur die Auswertung
//! ist durch Tests abgesichert).

use std::{env, path::PathBuf, time::Duration};

use pa_core::tool_loop::{render_tool_prompt, TOOL_ENVELOPE_GBNF};
use pa_inference::{
    adapter::AdapterKind,
    chat::{stream_chat_cancelable_with_options, ChatOptions},
    config::ServerConfig,
    loopback::LoopbackEndpoint,
    process::ServerProcess,
};
use pa_launcher::eval::{judge, parse_suite, summarize, CaseKind, EvalCase};
use pa_types::{
    chat::{Message, MessageRole, MessageStatus},
    model::KvQuantization,
};

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("Umgebungsvariable {name} fehlt"))
}

fn to_messages(case: &EvalCase, tool_prompt: Option<&str>) -> Vec<Message> {
    let mut messages = Vec::new();
    let mut push = |role: MessageRole, content: &str| {
        let position = i64::try_from(messages.len()).unwrap_or(0);
        messages.push(Message {
            id: format!("eval-{position}"),
            conversation_id: "eval".to_owned(),
            position,
            role,
            content: content.to_owned(),
            status: MessageStatus::Complete,
            created_at_unix_ms: 0,
        });
    };
    if let Some(prompt) = tool_prompt {
        push(MessageRole::System, prompt);
    }
    for message in &case.messages {
        let role = match message.role.as_str() {
            "system" => MessageRole::System,
            "assistant" => MessageRole::Assistant,
            _ => MessageRole::User,
        };
        push(role, &message.content);
    }
    messages
}

#[test]
#[ignore = "braucht llama-server und ein Modell (IAP_EVAL_SERVER, IAP_EVAL_MODEL, IAP_EVAL_FAMILY)"]
fn run_model_evals() {
    let suite = parse_suite(include_str!("../../../evals/cases.toml")).expect("Fallliste");
    let (port, api_key) = ServerConfig::random_endpoint().expect("Port");
    let config = ServerConfig {
        executable: PathBuf::from(required("IAP_EVAL_SERVER")),
        model: PathBuf::from(required("IAP_EVAL_MODEL")),
        model_alias: "eval".to_owned(),
        port,
        api_key: api_key.clone(),
        context_tokens: 8192,
        threads: env::var("IAP_EVAL_THREADS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(4),
        gpu_layers: 0,
        kv_quantization: KvQuantization::F16,
        mmproj: None,
    };
    let mut server = ServerProcess::spawn(&config).expect("llama-server starten");
    server
        .wait_ready(Duration::from_secs(240))
        .expect("llama-server wurde nicht bereit");
    let endpoint = LoopbackEndpoint::new(port, api_key).expect("Endpunkt");
    let adapter = AdapterKind::from_family(&required("IAP_EVAL_FAMILY"), "eval");
    let tool_prompt = render_tool_prompt(&suite.tools);

    let mut verdicts = Vec::new();
    let mut report = Vec::new();
    for case in &suite.cases {
        let is_tool = case.kind == CaseKind::Tool;
        let messages = to_messages(case, is_tool.then_some(tool_prompt.as_str()));
        let options = ChatOptions {
            grammar: is_tool.then(|| TOOL_ENVELOPE_GBNF.to_owned()),
            ..ChatOptions::default()
        };
        let outcome = stream_chat_cancelable_with_options(
            &endpoint,
            &adapter,
            &messages,
            &options,
            || true,
            |_| true,
        );
        let verdict = match outcome {
            Ok(outcome) => {
                let verdict = judge(case, &outcome.text);
                report.push(serde_json::json!({
                    "id": case.id, "passed": verdict.passed, "reasons": verdict.reasons, "output": outcome.text,
                }));
                verdict
            }
            Err(error) => {
                let verdict = pa_launcher::eval::Verdict {
                    passed: false,
                    reasons: vec![format!("Anfrage fehlgeschlagen: {error}")],
                };
                report.push(serde_json::json!({ "id": case.id, "passed": false, "reasons": verdict.reasons }));
                verdict
            }
        };
        println!(
            "{} {} {}",
            if verdict.passed { "OK  " } else { "FAIL" },
            case.id,
            verdict.reasons.join("; ")
        );
        verdicts.push(verdict);
    }
    let _ = server.stop();

    let summary = summarize(&verdicts);
    println!(
        "{} von {} bestanden ({:.0} %)",
        summary.passed,
        summary.total,
        summary.pass_rate() * 100.0
    );
    if let Ok(path) = env::var("IAP_EVAL_OUT") {
        let text = serde_json::to_string_pretty(&serde_json::json!({
            "family": required("IAP_EVAL_FAMILY"),
            "passed": summary.passed,
            "total": summary.total,
            "cases": report,
        }))
        .expect("Bericht");
        std::fs::write(path, text).expect("Bericht schreiben");
    }
    let minimum: f64 = env::var("IAP_EVAL_MIN_PASS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.7);
    assert!(
        summary.pass_rate() >= minimum,
        "Bestehensquote {:.2} liegt unter {minimum:.2}",
        summary.pass_rate()
    );
}
