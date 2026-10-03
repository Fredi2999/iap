//! GBNF-Grammatiken für Critic und Verifier.
//!
//! Wir übergeben diese Grammatiken an `llama-server` als
//! Server-Sampling-Parameter, damit die Modellausgabe formal nichts
//! anderes sein kann als die im [`crate::schema`] beschriebene
//! JSON-Struktur (Konzept 7.2: „Strukturierter Output per
//! GBNF-Grammar. Der Critic *kann* formal nichts anderes ausgeben als
//! ein Array von Befunden mit Pflichtfeldern").
//!
//! Ein leeres Array `[]` ist in beiden Grammatiken zugelassen — das ist
//! die konzeptuelle Voraussetzung für den Frühabbruch (Critic) und
//! die Aussage „nichts prüfbar" (Verifier).

/// GBNF für den Critic. Ergebnis-Body: `{"findings": [ {…}, … ]}`.
pub const CRITIC_GBNF: &str = r#"
root       ::= "{\"findings\":" findings "}"
findings   ::= "[]" | "[" finding ("," finding)* "]"
finding    ::= "{"
                 "\"befund\":" string ","
                 "\"schweregrad\":" severity ","
                 "\"betrifft\":" string ","
                 "\"vorschlag\":" string
               "}"
severity   ::= "\"low\"" | "\"medium\"" | "\"high\"" | "\"critical\""
string     ::= "\"" schar* "\""
schar      ::= [^"\\] | "\\" ["\\/bfnrt] | "\\u" [0-9a-fA-F]{4}
"#;

/// GBNF für den Verifier. Body: `{"claims": [ {…}, … ]}`.
pub const VERIFIER_GBNF: &str = r#"
root       ::= "{\"claims\":" claims "}"
claims     ::= "[]" | "[" claim ("," claim)* "]"
claim      ::= "{"
                 "\"aussage\":" string ","
                 "\"status\":" status ","
                 "\"beleg\":" string
               "}"
status     ::= "\"confirmed\"" | "\"refuted\"" | "\"not_verifiable\""
string     ::= "\"" schar* "\""
schar      ::= [^"\\] | "\\" ["\\/bfnrt] | "\\u" [0-9a-fA-F]{4}
"#;

/// Parst die Critic-Antwort (roher Modelltext) in das strukturierte
/// [`crate::schema::CriticReport`]. Toleriert leichtes Whitespace-
/// Rauschen, aber keinen Text jenseits des JSON-Objektes.
pub fn parse_critic(text: &str) -> Result<crate::schema::CriticReport, crate::AgentError> {
    let candidate = extract_balanced_object(text)
        .ok_or_else(|| crate::AgentError::Parse("kein JSON-Objekt gefunden".to_owned()))?;
    serde_json::from_str(&candidate)
        .map_err(|error| crate::AgentError::Parse(format!("Critic-JSON: {error}")))
}

/// Parst die Verifier-Antwort analog.
pub fn parse_verifier(text: &str) -> Result<crate::schema::VerifierReport, crate::AgentError> {
    let candidate = extract_balanced_object(text)
        .ok_or_else(|| crate::AgentError::Parse("kein JSON-Objekt gefunden".to_owned()))?;
    serde_json::from_str(&candidate)
        .map_err(|error| crate::AgentError::Parse(format!("Verifier-JSON: {error}")))
}

fn extract_balanced_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escape = false;
    for (index, &byte) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escape {
                escape = false;
            } else if byte == b'\\' {
                escape = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=index].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Severity, VerifierStatus};

    #[test]
    fn empty_critic_findings_are_accepted() {
        let text = r#"{"findings":[]}"#;
        let report = parse_critic(text).unwrap();
        assert!(report.findings.is_empty());
        assert!(!report.triggers_follow_up());
    }

    #[test]
    fn critic_with_single_medium_finding_is_parsed() {
        let text = r#"{"findings":[{"befund":"nix","schweregrad":"medium","betrifft":"x","vorschlag":"y"}]}"#;
        let report = parse_critic(text).unwrap();
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].schweregrad, Severity::Medium);
        assert!(report.triggers_follow_up());
    }

    #[test]
    fn verifier_all_three_statuses_round_trip() {
        let text = r#"{"claims":[
            {"aussage":"2+2=4","status":"confirmed","beleg":"calc"},
            {"aussage":"2+2=5","status":"refuted","beleg":"calc"},
            {"aussage":"morgen regnet es","status":"not_verifiable","beleg":"Zukunft"}
        ]}"#;
        let report = parse_verifier(text).unwrap();
        assert_eq!(report.claims.len(), 3);
        assert_eq!(report.claims[0].status, VerifierStatus::Confirmed);
        assert_eq!(report.claims[1].status, VerifierStatus::Refuted);
        assert_eq!(report.claims[2].status, VerifierStatus::NotVerifiable);
    }

    #[test]
    fn tolerates_prefix_text_before_json_object() {
        let text = r#"Hier meine Analyse: {"findings":[]} nachher noch mehr Text"#;
        let report = parse_critic(text).unwrap();
        assert!(report.findings.is_empty());
    }

    #[test]
    fn missing_field_is_an_error_not_a_default() {
        let text = r#"{"findings":[{"befund":"x","schweregrad":"medium","betrifft":"y"}]}"#;
        assert!(parse_critic(text).is_err());
    }
}
