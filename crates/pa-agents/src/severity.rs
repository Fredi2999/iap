//! Schweregrad-Skala für Critic-Befunde (Konzept 7.1 „Frühabbruch").
//!
//! Wird sowohl für die Serialisierung in JSON/GBNF benutzt als auch für
//! die deterministische Frühabbruch-Regel: mindestens ein Befund mit
//! `medium` oder höher lässt Verifier und Synthesizer laufen; sonst
//! wird der Proposer-Text direkt zurückgegeben.

use serde::{Deserialize, Serialize};

/// Vier Stufen; die Werte spiegeln die Byte-Bezeichner, die in der
/// GBNF-Grammatik erlaubt sind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    /// Deterministische Regel für den Frühabbruch (Konzept 7.1).
    pub fn triggers_verifier_and_synth(self) -> bool {
        self >= Severity::Medium
    }

    /// Stabile Byte-Bezeichnung, wie sie in JSON/GBNF landet.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_medium_or_higher_triggers_follow_up_roles() {
        assert!(!Severity::Low.triggers_verifier_and_synth());
        assert!(Severity::Medium.triggers_verifier_and_synth());
        assert!(Severity::High.triggers_verifier_and_synth());
        assert!(Severity::Critical.triggers_verifier_and_synth());
    }

    #[test]
    fn ordering_matches_conceptual_severity() {
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
        assert!(Severity::High < Severity::Critical);
    }
}
