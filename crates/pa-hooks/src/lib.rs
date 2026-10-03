//! Deklarative Hooks aus `hooks.toml` (Konzept 8.5).
//!
//! Ereignisse: `on_session_start`, `on_session_end`, `before_tool_call`,
//! `after_tool_call`, `on_file_added`, `on_idle`, `on_agent_stage_done`.
//!
//! **Kein Shell-Aufruf.** Ein Hook darf nur einen Kern-Werkzeug- oder
//! WASM-Skill-Namen nennen; der Runner ruft sie über einen [`HookInvoker`]-
//! Trait ausführbar. Beliebige Programme über die Hintertür sind
//! architektonisch ausgeschlossen — Konzept 8.5 explizit: „Sonst wäre die
//! Policy-Engine über die Hintertür aushebelbar."

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HookError {
    #[error("hooks.toml: {0}")]
    Parse(String),
    #[error("Hook `{name}` ruft nicht erlaubtes Werkzeug `{tool}`")]
    UnknownTool { name: String, tool: String },
    #[error("Hook `{name}` schlug beim Werkzeugaufruf fehl: {source}")]
    Invocation {
        name: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Alle Ereignistypen. Jedes darf mehrfach vorkommen (`[[on_session_start]]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    OnSessionStart,
    OnSessionEnd,
    BeforeToolCall,
    AfterToolCall,
    OnFileAdded,
    OnIdle,
    OnAgentStageDone,
}

/// Eine einzelne Regel aus `hooks.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookRule {
    #[serde(default)]
    pub description: String,
    /// Name des Werkzeugs (Kern-Werkzeug oder WASM-Skill-Tool).
    pub tool: String,
    /// Statische Argumente. Der Runner reicht sie 1:1 an das Werkzeug weiter.
    #[serde(default)]
    pub arguments: HashMap<String, Value>,
    /// Optionaler Filter: gilt nur, wenn der Kontext-Wert (`context.tool`
    /// oder `context.path`) dem angegebenen String entspricht.
    #[serde(default)]
    pub match_tool: Option<String>,
    #[serde(default)]
    pub match_path_prefix: Option<String>,
}

impl HookRule {
    /// Prüft, ob die Regel für den aktuellen Kontext gilt.
    pub fn matches(&self, ctx: &HookContext) -> bool {
        if let Some(tool) = &self.match_tool {
            if ctx.tool.as_deref() != Some(tool.as_str()) {
                return false;
            }
        }
        if let Some(prefix) = &self.match_path_prefix {
            match &ctx.path {
                Some(path) if path.starts_with(prefix) => {}
                _ => return false,
            }
        }
        true
    }
}

/// Vollständige Hook-Konfiguration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HooksConfig {
    #[serde(default, rename = "on_session_start")]
    pub on_session_start: Vec<HookRule>,
    #[serde(default, rename = "on_session_end")]
    pub on_session_end: Vec<HookRule>,
    #[serde(default, rename = "before_tool_call")]
    pub before_tool_call: Vec<HookRule>,
    #[serde(default, rename = "after_tool_call")]
    pub after_tool_call: Vec<HookRule>,
    #[serde(default, rename = "on_file_added")]
    pub on_file_added: Vec<HookRule>,
    #[serde(default, rename = "on_idle")]
    pub on_idle: Vec<HookRule>,
    #[serde(default, rename = "on_agent_stage_done")]
    pub on_agent_stage_done: Vec<HookRule>,
}

impl HooksConfig {
    /// Parst einen `hooks.toml`-Text.
    pub fn parse(text: &str) -> Result<Self, HookError> {
        toml::from_str::<HooksConfig>(text).map_err(|error| HookError::Parse(error.to_string()))
    }

    /// Liefert alle Regeln für ein Ereignis in Reihenfolge.
    pub fn rules_for(&self, event: HookEvent) -> &[HookRule] {
        match event {
            HookEvent::OnSessionStart => &self.on_session_start,
            HookEvent::OnSessionEnd => &self.on_session_end,
            HookEvent::BeforeToolCall => &self.before_tool_call,
            HookEvent::AfterToolCall => &self.after_tool_call,
            HookEvent::OnFileAdded => &self.on_file_added,
            HookEvent::OnIdle => &self.on_idle,
            HookEvent::OnAgentStageDone => &self.on_agent_stage_done,
        }
    }
}

/// Kontext, den der Runner den Regeln beim Abgleich mitgibt.
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    pub tool: Option<String>,
    pub path: Option<String>,
    pub extra: HashMap<String, Value>,
}

/// Ausführungs-Backend. Die App-Schicht implementiert dies über die
/// bestehende [`pa_tools::ToolRegistry`] plus WASM-Skill-Registry und
/// bekommt so **keinen** Zugriff auf beliebige Prozesse.
pub trait HookInvoker {
    /// Führt einen Werkzeugaufruf aus.
    fn invoke(
        &mut self,
        tool: &str,
        arguments: &HashMap<String, Value>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>>;

    /// Wird `false`, ist der Werkzeugname nicht erlaubt (weder Kern-Werkzeug
    /// noch installierter WASM-Skill). Der Runner meldet das dann als
    /// `HookError::UnknownTool`.
    fn is_allowed(&self, tool: &str) -> bool;
}

/// Führt alle passenden Regeln für ein Ereignis aus. Reihenfolge = Konfig.
/// Erster harter Fehler bricht ab, weiche Fehler landen im `Vec<HookReport>`.
pub fn run(
    config: &HooksConfig,
    event: HookEvent,
    ctx: &HookContext,
    invoker: &mut dyn HookInvoker,
) -> Result<Vec<HookReport>, HookError> {
    let mut reports = Vec::new();
    for (index, rule) in config.rules_for(event).iter().enumerate() {
        if !rule.matches(ctx) {
            continue;
        }
        if !invoker.is_allowed(&rule.tool) {
            return Err(HookError::UnknownTool {
                name: rule_label(rule, index),
                tool: rule.tool.clone(),
            });
        }
        match invoker.invoke(&rule.tool, &rule.arguments) {
            Ok(value) => reports.push(HookReport {
                rule: rule_label(rule, index),
                tool: rule.tool.clone(),
                result: value,
            }),
            Err(error) => {
                return Err(HookError::Invocation {
                    name: rule_label(rule, index),
                    source: error,
                });
            }
        }
    }
    Ok(reports)
}

fn rule_label(rule: &HookRule, index: usize) -> String {
    if rule.description.is_empty() {
        format!("#{index}")
    } else {
        rule.description.clone()
    }
}

/// Report einer erfolgreichen Regel-Ausführung.
#[derive(Debug, Clone, Serialize)]
pub struct HookReport {
    pub rule: String,
    pub tool: String,
    pub result: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeInvoker {
        allowed: Vec<String>,
        calls: std::cell::RefCell<Vec<(String, HashMap<String, Value>)>>,
    }

    impl HookInvoker for FakeInvoker {
        fn invoke(
            &mut self,
            tool: &str,
            arguments: &HashMap<String, Value>,
        ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
            self.calls
                .borrow_mut()
                .push((tool.to_owned(), arguments.clone()));
            Ok(Value::String("ok".into()))
        }

        fn is_allowed(&self, tool: &str) -> bool {
            self.allowed.iter().any(|allowed| allowed == tool)
        }
    }

    #[test]
    fn parses_all_events() {
        let text = r#"
[[on_session_start]]
description = "Übersicht"
tool = "list_dir"
arguments = { path = "workspace" }

[[before_tool_call]]
match_tool = "write_file"
tool = "log_event"
arguments = { level = "info" }
"#;
        let config = HooksConfig::parse(text).unwrap();
        assert_eq!(config.on_session_start.len(), 1);
        assert_eq!(config.before_tool_call.len(), 1);
    }

    #[test]
    fn unknown_tool_is_hard_error() {
        let config = HooksConfig::parse(
            r#"
[[on_idle]]
tool = "curl_something"
"#,
        )
        .unwrap();
        let mut invoker = FakeInvoker {
            allowed: vec!["list_dir".into()],
            calls: Default::default(),
        };
        let error = run(
            &config,
            HookEvent::OnIdle,
            &HookContext::default(),
            &mut invoker,
        )
        .unwrap_err();
        assert!(matches!(error, HookError::UnknownTool { .. }));
    }

    #[test]
    fn match_tool_narrows_before_tool_call() {
        let config = HooksConfig::parse(
            r#"
[[before_tool_call]]
match_tool = "write_file"
tool = "log_event"
"#,
        )
        .unwrap();
        let mut invoker = FakeInvoker {
            allowed: vec!["log_event".into()],
            calls: Default::default(),
        };
        let ctx_match = HookContext {
            tool: Some("write_file".into()),
            ..HookContext::default()
        };
        let ctx_miss = HookContext {
            tool: Some("read_file".into()),
            ..HookContext::default()
        };
        let matched = run(&config, HookEvent::BeforeToolCall, &ctx_match, &mut invoker).unwrap();
        assert_eq!(matched.len(), 1);
        let missed = run(&config, HookEvent::BeforeToolCall, &ctx_miss, &mut invoker).unwrap();
        assert!(missed.is_empty());
    }
}
