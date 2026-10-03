//! Auswertung von Modell-Evals: feste Testfragen mit prüfbaren Erwartungen.
//!
//! Warum: Konzept 3.2 warnt, dass „abliterierte“ Modelle bei Werkzeugaufrufen und strukturiertem
//! JSON schwächer sind, und die Modell-Manifeste nennen diese Qualität als ungeprüft. Ohne feste
//! Fragen bleibt jede Modellwahl ein Eindruck. Dieses Modul enthält nur die Logik (Fallliste lesen,
//! Antwort bewerten, Ergebnis zusammenfassen) und läuft ohne Modell. Den Lauf gegen ein echtes
//! Modell macht `tests/model_eval.rs` (ignoriert, braucht llama-server und ein Modell).
//!
//! Die Fälle liegen in `evals/cases.toml`. Eine bestandene Prüfung heißt nur „diese Antwort erfüllt
//! diese Erwartung“, nicht „das Modell ist gut“: Die Fallliste ist klein und ein Anhaltspunkt.

use pa_core::tool_loop::{parse_envelope, ToolStep};
use pa_tools::ToolSpec;
use serde::Deserialize;
use serde_json::Value;

/// Art des Falls: mit Werkzeug-Hülle (JSON-Grammatik) oder als freier Chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseKind {
    /// System-Prompt mit Werkzeugliste, Grammatik erzwingt die JSON-Hülle.
    Tool,
    /// Freier Text ohne Grammatik.
    Chat,
}

/// Eine Nachricht im Verlauf des Falls.
#[derive(Debug, Clone, Deserialize)]
pub struct EvalMessage {
    /// `system`, `user` oder `assistant`.
    pub role: String,
    pub content: String,
}

/// Ein Testfall mit Erwartungen. Nicht gesetzte Erwartungen werden nicht geprüft.
#[derive(Debug, Clone, Deserialize)]
pub struct EvalCase {
    pub id: String,
    pub kind: CaseKind,
    /// Verlauf; die letzte Nachricht ist die Frage.
    pub messages: Vec<EvalMessage>,
    /// Bei `tool`: erwartet `call` oder `answer`.
    pub expect_action: Option<String>,
    /// Bei `call`: Name des erwarteten Werkzeugs.
    pub expect_tool: Option<String>,
    /// Bei `call`: Argumente, die vorhanden sein müssen.
    #[serde(default)]
    pub expect_args: Vec<String>,
    /// Bei `call`: Werkzeuge, die auf keinen Fall aufgerufen werden dürfen.
    #[serde(default)]
    pub forbid_tools: Vec<String>,
    /// Muss im Text (bei `answer` im Antworttext) vorkommen; Groß-/Kleinschreibung egal.
    #[serde(default)]
    pub contains: Vec<String>,
    /// Darf nicht vorkommen.
    #[serde(default)]
    pub not_contains: Vec<String>,
    pub max_chars: Option<usize>,
}

/// Werkzeug für den System-Prompt (gleiche Felder wie `ToolSpec`, Schema als JSON-Text).
#[derive(Debug, Clone, Deserialize)]
struct ToolEntry {
    name: String,
    description: String,
    category: String,
    parameters_schema: String,
}

#[derive(Debug, Deserialize)]
struct SuiteFile {
    #[serde(default)]
    tool: Vec<ToolEntry>,
    #[serde(default)]
    case: Vec<EvalCase>,
}

/// Geladene Fallliste.
#[derive(Debug, Clone)]
pub struct EvalSuite {
    pub tools: Vec<ToolSpec>,
    pub cases: Vec<EvalCase>,
}

/// Liest `evals/cases.toml`. Fehlerhafte Fälle (doppelte Kennung, Schema kein JSON, leerer Verlauf)
/// brechen den Lauf ab, statt still zu fehlen.
pub fn parse_suite(text: &str) -> Result<EvalSuite, String> {
    let file: SuiteFile = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut tools = Vec::new();
    for entry in file.tool {
        let parameters_schema: Value = serde_json::from_str(&entry.parameters_schema)
            .map_err(|error| format!("Werkzeug `{}`: Schema ist kein JSON: {error}", entry.name))?;
        tools.push(ToolSpec {
            name: entry.name,
            description: entry.description,
            parameters_schema,
            category: entry.category,
        });
    }
    let mut seen = std::collections::HashSet::new();
    for case in &file.case {
        if !seen.insert(case.id.clone()) {
            return Err(format!("doppelte Fall-Kennung `{}`", case.id));
        }
        if case.messages.is_empty() {
            return Err(format!("Fall `{}` hat keinen Verlauf", case.id));
        }
        if case
            .messages
            .iter()
            .any(|m| !matches!(m.role.as_str(), "system" | "user" | "assistant"))
        {
            return Err(format!("Fall `{}`: unbekannte Rolle", case.id));
        }
    }
    Ok(EvalSuite {
        tools,
        cases: file.case,
    })
}

/// Ergebnis eines Falls: bestanden oder die Gründe, warum nicht.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub passed: bool,
    pub reasons: Vec<String>,
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Bewertet die rohe Modellantwort eines Falls.
pub fn judge(case: &EvalCase, output: &str) -> Verdict {
    let mut reasons = Vec::new();
    // Text, auf den sich `contains` bezieht: bei Werkzeug-Fällen mit Antwort deren Text, bei einem
    // Aufruf die Argumente als JSON, sonst die Rohausgabe.
    let mut text_to_check = output.to_owned();

    if case.kind == CaseKind::Tool {
        match parse_envelope(output) {
            Err(error) => reasons.push(format!("keine gültige JSON-Hülle: {error}")),
            Ok(step) => match &step {
                ToolStep::Answer(text) => {
                    text_to_check = text.clone();
                    if let Some(expected) = &case.expect_action {
                        if expected != "answer" {
                            reasons.push(format!("erwartet `{expected}`, bekam `answer`"));
                        }
                    }
                    if case.expect_tool.is_some() {
                        reasons
                            .push("erwartete einen Werkzeugaufruf, bekam eine Antwort".to_owned());
                    }
                }
                ToolStep::Call { tool, arguments } => {
                    text_to_check = serde_json::to_string(arguments).unwrap_or_default();
                    if let Some(expected) = &case.expect_action {
                        if expected != "call" {
                            reasons.push(format!("erwartet `{expected}`, bekam `call`"));
                        }
                    }
                    if let Some(expected) = &case.expect_tool {
                        if expected != tool {
                            reasons.push(format!("erwartet Werkzeug `{expected}`, bekam `{tool}`"));
                        }
                    }
                    if case.forbid_tools.iter().any(|forbidden| forbidden == tool) {
                        reasons.push(format!("verbotenes Werkzeug `{tool}` aufgerufen"));
                    }
                    for name in &case.expect_args {
                        if !arguments.contains_key(name) {
                            reasons.push(format!("Argument `{name}` fehlt"));
                        }
                    }
                }
            },
        }
    }

    for needle in &case.contains {
        if !contains_ignore_case(&text_to_check, needle) {
            reasons.push(format!("`{needle}` fehlt"));
        }
    }
    for needle in &case.not_contains {
        if contains_ignore_case(&text_to_check, needle) {
            reasons.push(format!("`{needle}` darf nicht vorkommen"));
        }
    }
    if let Some(max) = case.max_chars {
        let length = text_to_check.chars().count();
        if length > max {
            reasons.push(format!("{length} Zeichen, erlaubt sind {max}"));
        }
    }
    Verdict {
        passed: reasons.is_empty(),
        reasons,
    }
}

/// Zusammenfassung eines Laufs.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub total: usize,
    pub passed: usize,
}

impl Summary {
    /// Anteil bestandener Fälle von 0 bis 1; ohne Fälle 0, damit ein leerer Lauf nie „besteht“.
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.passed as f64 / self.total as f64
        }
    }
}

/// Zählt bestandene Fälle.
pub fn summarize(verdicts: &[Verdict]) -> Summary {
    Summary {
        total: verdicts.len(),
        passed: verdicts.iter().filter(|verdict| verdict.passed).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(kind: CaseKind) -> EvalCase {
        EvalCase {
            id: "t".to_owned(),
            kind,
            messages: vec![EvalMessage {
                role: "user".to_owned(),
                content: "x".to_owned(),
            }],
            expect_action: None,
            expect_tool: None,
            expect_args: vec![],
            forbid_tools: vec![],
            contains: vec![],
            not_contains: vec![],
            max_chars: None,
        }
    }

    #[test]
    fn a_valid_call_with_the_expected_tool_and_arguments_passes() {
        let mut c = case(CaseKind::Tool);
        c.expect_action = Some("call".to_owned());
        c.expect_tool = Some("read_file".to_owned());
        c.expect_args = vec!["path".to_owned()];
        let ok = r#"{"action":"call","tool":"read_file","arguments":{"path":"a.md"}}"#;
        assert!(judge(&c, ok).passed);
        let wrong_tool = r#"{"action":"call","tool":"delete_file","arguments":{"path":"a.md"}}"#;
        assert!(!judge(&c, wrong_tool).passed);
        let missing_arg = r#"{"action":"call","tool":"read_file","arguments":{}}"#;
        let verdict = judge(&c, missing_arg);
        assert!(verdict.reasons.iter().any(|r| r.contains("`path` fehlt")));
    }

    #[test]
    fn prose_instead_of_the_json_envelope_fails_a_tool_case() {
        let c = case(CaseKind::Tool);
        let verdict = judge(&c, "Klar, ich lese die Datei für dich.");
        assert!(!verdict.passed);
        assert!(verdict.reasons[0].contains("JSON-Hülle"));
    }

    #[test]
    fn a_forbidden_tool_fails_even_when_the_json_is_valid() {
        let mut c = case(CaseKind::Tool);
        c.forbid_tools = vec!["delete_file".to_owned()];
        let verdict = judge(
            &c,
            r#"{"action":"call","tool":"delete_file","arguments":{"path":"x"}}"#,
        );
        assert!(!verdict.passed);
    }

    #[test]
    fn answers_are_checked_against_their_text_not_the_json_wrapper() {
        let mut c = case(CaseKind::Tool);
        c.expect_action = Some("answer".to_owned());
        c.contains = vec!["wien".to_owned()];
        assert!(
            judge(
                &c,
                r#"{"action":"answer","text":"Die Hauptstadt ist Wien."}"#
            )
            .passed
        );
        assert!(!judge(&c, r#"{"action":"answer","text":"Graz."}"#).passed);
        // `action` im Rohtext würde ein Wrapper-Treffer sein; geprüft wird nur der Antworttext.
        c.contains = vec!["action".to_owned()];
        assert!(!judge(&c, r#"{"action":"answer","text":"Wien"}"#).passed);
    }

    #[test]
    fn chat_cases_check_contains_not_contains_and_length() {
        let mut c = case(CaseKind::Chat);
        c.not_contains = vec!["Zeppelin".to_owned()];
        c.max_chars = Some(20);
        assert!(judge(&c, "Das sage ich nicht.").passed);
        assert!(!judge(&c, "Das Wort ist zeppelin.").passed);
        assert!(
            !judge(
                &c,
                "Ein sehr langer Satz, der weit über das Limit hinausgeht."
            )
            .passed
        );
    }

    #[test]
    fn an_empty_run_never_counts_as_passed() {
        assert_eq!(summarize(&[]).pass_rate(), 0.0);
        let verdicts = [
            Verdict {
                passed: true,
                reasons: vec![],
            },
            Verdict {
                passed: false,
                reasons: vec!["x".to_owned()],
            },
        ];
        assert_eq!(summarize(&verdicts).pass_rate(), 0.5);
    }

    #[test]
    fn the_shipped_case_file_is_valid() {
        let text = include_str!("../../../evals/cases.toml");
        let suite = parse_suite(text).expect("evals/cases.toml muss gültig sein");
        assert!(
            suite.cases.len() >= 8,
            "zu wenige Fälle: {}",
            suite.cases.len()
        );
        assert!(!suite.tools.is_empty());
        // Jeder Werkzeug-Fall erwartet etwas Prüfbares, sonst bestünde er immer.
        for case in &suite.cases {
            let checks = case.expect_action.is_some()
                || case.expect_tool.is_some()
                || !case.contains.is_empty()
                || !case.not_contains.is_empty()
                || !case.forbid_tools.is_empty()
                || case.max_chars.is_some();
            assert!(checks, "Fall `{}` prüft nichts", case.id);
        }
    }

    #[test]
    fn broken_suites_are_errors() {
        assert!(parse_suite("[[case]]\nid=\"a\"\nkind=\"chat\"\nmessages=[]").is_err());
        let dup = "[[case]]\nid=\"a\"\nkind=\"chat\"\nmessages=[{role=\"user\",content=\"x\"}]\n[[case]]\nid=\"a\"\nkind=\"chat\"\nmessages=[{role=\"user\",content=\"x\"}]";
        assert!(parse_suite(dup).unwrap_err().contains("doppelte"));
        let bad_role =
            "[[case]]\nid=\"a\"\nkind=\"chat\"\nmessages=[{role=\"boss\",content=\"x\"}]";
        assert!(parse_suite(bad_role).is_err());
    }
}
