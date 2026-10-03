//! Serialisierbare Rollen-Ausgaben.
//!
//! Critic und Verifier antworten laut Konzept 7.2 ausschließlich mit JSON
//! nach fest definiertem Schema. Diese Datei ist die einzige Stelle, an
//! der die Rust-Repräsentation lebt; die GBNF-Grammatiken in
//! [`crate::grammar`] erzwingen dieselbe Struktur formal für den Server.

use serde::{Deserialize, Serialize};

pub use crate::severity::Severity;

/// Ein einzelner Befund des Critics.
///
/// Alle vier Felder sind Pflicht — ein Critic, der schwammig antwortet,
/// wird durch die Grammatik dazu gezwungen, das Feld zumindest mit
/// einem leeren String zu füllen, was in der UI sofort auffällt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CriticFinding {
    pub befund: String,
    pub schweregrad: Severity,
    pub betrifft: String,
    pub vorschlag: String,
}

/// Gesamtausgabe des Critics.
///
/// Ein leeres `findings`-Array ist ein gültiges Ergebnis und Auslöser
/// für den Frühabbruch (Konzept 7.1: „Findet der Kritiker nichts,
/// werden Verifier und Synthesizer übersprungen").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CriticReport {
    pub findings: Vec<CriticFinding>,
}

impl CriticReport {
    /// Deterministische Frühabbruch-Regel: gibt es einen Befund mit
    /// mindestens `medium`?
    pub fn triggers_follow_up(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.schweregrad.triggers_verifier_and_synth())
    }
}

/// Bewertungsstatus einer einzelnen prüfbaren Aussage.
///
/// `not_verifiable` ist ausdrücklich zulässig — der Verifier soll eher
/// „nicht prüfbar" sagen als eine Fantasieprüfung durchzuführen
/// (Konzept 7.2: „Der Verifier meint nichts, er prüft").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierStatus {
    Confirmed,
    Refuted,
    NotVerifiable,
}

impl VerifierStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            VerifierStatus::Confirmed => "confirmed",
            VerifierStatus::Refuted => "refuted",
            VerifierStatus::NotVerifiable => "not_verifiable",
        }
    }
}

/// Ein einzelner Prüfbeleg.
///
/// `beleg` sollte einen Werkzeugverweis (Tool + Ergebnisausriss) enthalten,
/// damit der Nutzer im Laufbaum die Herkunft nachvollziehen kann.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifierClaim {
    pub aussage: String,
    pub status: VerifierStatus,
    pub beleg: String,
}

/// Gesamtausgabe des Verifiers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifierReport {
    pub claims: Vec<VerifierClaim>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_critic_report_does_not_trigger_follow_up() {
        let report = CriticReport { findings: vec![] };
        assert!(!report.triggers_follow_up());
    }

    #[test]
    fn low_only_findings_do_not_trigger_follow_up() {
        let report = CriticReport {
            findings: vec![CriticFinding {
                befund: "kleiner Tippfehler".into(),
                schweregrad: Severity::Low,
                betrifft: "README.md".into(),
                vorschlag: "korrigieren".into(),
            }],
        };
        assert!(!report.triggers_follow_up());
    }

    #[test]
    fn a_single_medium_finding_triggers_follow_up() {
        let report = CriticReport {
            findings: vec![
                CriticFinding {
                    befund: "unklarer Fehlerpfad".into(),
                    schweregrad: Severity::Medium,
                    betrifft: "src/foo.rs".into(),
                    vorschlag: "Result verwenden".into(),
                },
                CriticFinding {
                    befund: "Tippfehler".into(),
                    schweregrad: Severity::Low,
                    betrifft: "README.md".into(),
                    vorschlag: "korrigieren".into(),
                },
            ],
        };
        assert!(report.triggers_follow_up());
    }

    #[test]
    fn json_round_trip_preserves_fields() {
        let report = CriticReport {
            findings: vec![CriticFinding {
                befund: "keine Randfallprüfung".into(),
                schweregrad: Severity::High,
                betrifft: "src/util.rs:42".into(),
                vorschlag: "Guard gegen 0 einbauen".into(),
            }],
        };
        let json = serde_json::to_string(&report).unwrap();
        let decoded: CriticReport = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, report);
    }

    #[test]
    fn verifier_status_round_trips_snake_case() {
        for status in [
            VerifierStatus::Confirmed,
            VerifierStatus::Refuted,
            VerifierStatus::NotVerifiable,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let decoded: VerifierStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, status);
            assert!(json.contains(status.as_str()));
        }
    }
}
