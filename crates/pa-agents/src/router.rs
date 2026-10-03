//! Router: schlägt eine Eskalationsstufe vor.
//!
//! Konzept 7.1: „Heuristik aus Anfragelänge, erkannten Schlüsselbegriffen
//! ('bewerte', 'plane', 'prüfe', 'vergleiche'), aktivem Skill und Tier —
//! und der Nutzer bestätigt oder überschreibt sie mit einem Klick."
//!
//! Wichtig: ausschließlich Heuristik, kein Modellaufruf. Der Router
//! verbraucht null Tokens.

use pa_types::model::HardwareTier;
use serde::{Deserialize, Serialize};

use crate::runner::Stage;

/// Empfehlung des Routers samt Begründung und Zeitschätzung. Die UI
/// zeigt beides *vor* dem Klick auf „Start".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouterSuggestion {
    pub stage: Stage,
    pub reason: String,
    pub estimated_seconds: Option<f64>,
}

/// Schätzung der Wanduhr-Dauer einer Stufe auf Basis der zuletzt
/// gemessenen Generierungsrate.
#[derive(Debug, Clone, Copy)]
pub struct TimeEstimate {
    /// Zuletzt gemessene Antwort-Rate; `None` = noch keine Messung
    /// verfügbar → keine Schätzung.
    pub predicted_tokens_per_second: Option<f64>,
    /// Zusätzliche Latenz pro Runde (Prompt-Aufbau, Netzwerk-Overhead).
    pub per_round_overhead_seconds: f64,
}

impl TimeEstimate {
    /// Wandelt ein Token-Budget in eine geschätzte Wanduhr-Dauer.
    pub fn seconds_for(&self, budget_tokens: u32, rounds: u32) -> Option<f64> {
        let rate = self.predicted_tokens_per_second?;
        if rate <= 0.0 {
            return None;
        }
        let generation = budget_tokens as f64 / rate;
        Some(generation + self.per_round_overhead_seconds * rounds as f64)
    }
}

/// Konfigurierbare Heuristik. Die Werte sind bewusst schlicht, damit
/// der Router in `<1 ms` läuft und keine Trainingsdaten braucht.
#[derive(Debug, Clone)]
pub struct RouterHeuristic {
    /// Wörter, die typischerweise Kritik/Analyse einfordern (L2/L3).
    pub critic_keywords: Vec<String>,
    /// Wörter, die eine tiefere Prüfung mit Werkzeugen einfordern (L3).
    pub verifier_keywords: Vec<String>,
    /// Zeichenlängen-Schwellwerte für L1 / L2.
    pub self_check_threshold_chars: usize,
    pub critic_threshold_chars: usize,
    /// Zeitschätzung.
    pub estimate: TimeEstimate,
}

impl Default for RouterHeuristic {
    fn default() -> Self {
        Self {
            critic_keywords: [
                "bewerte",
                "plane",
                "prüfe",
                "vergleiche",
                "analysiere",
                "review",
                "kritisiere",
                "risiko",
                "risiken",
                "annahmen",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            verifier_keywords: [
                "berechne",
                "rechne",
                "welche datei",
                "existiert",
                "kompiliert",
                "läuft der test",
                "beleg",
                "quelle",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            self_check_threshold_chars: 400,
            critic_threshold_chars: 900,
            estimate: TimeEstimate {
                predicted_tokens_per_second: None,
                per_round_overhead_seconds: 1.0,
            },
        }
    }
}

impl RouterHeuristic {
    /// Für Tests / UI-Voreinstellung: Rate aus dem letzten realen Lauf setzen.
    pub fn with_rate(mut self, tokens_per_second: f64) -> Self {
        self.estimate.predicted_tokens_per_second = Some(tokens_per_second);
        self
    }

    /// Liefert eine Empfehlung für den gegebenen Nutzertext und die Tier.
    ///
    /// Wichtig: T0 bekommt niemals automatisch L3 (Konzept 7.1). Wenn die
    /// Heuristik L3 vorschlägt, aber die Tier T0 ist, wird L2 empfohlen
    /// mit expliziter Begründung.
    pub fn suggest(&self, user_text: &str, tier: HardwareTier) -> RouterSuggestion {
        let lowered = user_text.to_lowercase();
        let has_verifier_keyword = self
            .verifier_keywords
            .iter()
            .any(|kw| lowered.contains(kw.as_str()));
        let has_critic_keyword = self
            .critic_keywords
            .iter()
            .any(|kw| lowered.contains(kw.as_str()));
        let length = user_text.chars().count();

        let mut stage = if has_verifier_keyword {
            Stage::L3Full
        } else if has_critic_keyword || length >= self.critic_threshold_chars {
            Stage::L2Critique
        } else if length >= self.self_check_threshold_chars {
            Stage::L1SelfCheck
        } else {
            Stage::L0Direct
        };

        let mut reason = if has_verifier_keyword {
            format!(
                "Verifier-Schlüsselbegriff erkannt (‚{}'), L3 vorgeschlagen.",
                self.first_match(&lowered, &self.verifier_keywords)
                    .unwrap_or_default()
            )
        } else if has_critic_keyword {
            format!(
                "Critic-Schlüsselbegriff erkannt (‚{}'), L2 vorgeschlagen.",
                self.first_match(&lowered, &self.critic_keywords)
                    .unwrap_or_default()
            )
        } else if length >= self.critic_threshold_chars {
            format!("Text ist lang ({length} Zeichen) — Kritik-Runde empfohlen.")
        } else if length >= self.self_check_threshold_chars {
            format!("Text ist mittellang ({length} Zeichen) — Self-Check empfohlen.")
        } else {
            "Kurze Anfrage — direkt beantworten.".to_owned()
        };

        if stage == Stage::L3Full && matches!(tier, HardwareTier::T0) {
            stage = Stage::L2Critique;
            reason.push_str(" Auf Tier 0 wird L3 nie automatisch gewählt; ");
            reason.push_str("Nutzer muss L3 ausdrücklich bestätigen.");
        }

        let (budget_tokens, rounds) = stage.token_and_round_budget();
        let estimated_seconds = self.estimate.seconds_for(budget_tokens, rounds);
        RouterSuggestion {
            stage,
            reason,
            estimated_seconds,
        }
    }

    fn first_match(&self, lowered: &str, keywords: &[String]) -> Option<String> {
        keywords
            .iter()
            .find(|kw| lowered.contains(kw.as_str()))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_question_maps_to_l0() {
        let router = RouterHeuristic::default();
        let suggestion = router.suggest("Was ist 2+2?", HardwareTier::T1);
        assert_eq!(suggestion.stage, Stage::L0Direct);
    }

    #[test]
    fn critique_keyword_forces_l2() {
        let router = RouterHeuristic::default();
        let suggestion = router.suggest("Bewerte meinen Plan", HardwareTier::T1);
        assert_eq!(suggestion.stage, Stage::L2Critique);
    }

    #[test]
    fn verifier_keyword_forces_l3_on_higher_tiers() {
        let router = RouterHeuristic::default();
        let suggestion = router.suggest("Berechne die Marge", HardwareTier::T1);
        assert_eq!(suggestion.stage, Stage::L3Full);
    }

    #[test]
    fn tier_zero_never_auto_selects_l3() {
        let router = RouterHeuristic::default();
        let suggestion = router.suggest("Berechne die Marge", HardwareTier::T0);
        assert_eq!(suggestion.stage, Stage::L2Critique);
        assert!(suggestion.reason.contains("ausdrücklich"));
    }

    #[test]
    fn long_text_without_keywords_maps_to_self_check() {
        let router = RouterHeuristic::default();
        let filler = "a".repeat(500);
        let suggestion = router.suggest(&filler, HardwareTier::T1);
        assert_eq!(suggestion.stage, Stage::L1SelfCheck);
    }

    #[test]
    fn estimate_uses_last_measured_rate_when_available() {
        let router = RouterHeuristic::default().with_rate(20.0);
        let suggestion = router.suggest("Bewerte meinen Plan", HardwareTier::T1);
        // L2 hat Budget 1100 Tokens; bei 20 tok/s ≈ 55 s + Overhead.
        assert!(suggestion.estimated_seconds.unwrap() > 50.0);
    }

    #[test]
    fn missing_rate_yields_no_estimate() {
        let router = RouterHeuristic::default();
        let suggestion = router.suggest("Bewerte meinen Plan", HardwareTier::T1);
        assert!(suggestion.estimated_seconds.is_none());
    }
}
