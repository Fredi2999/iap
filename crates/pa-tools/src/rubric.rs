//! Rubrik-Engine für Ideenbewertungen (Konzept 4.2).
//!
//! Drei Stufen, exakt nach Konzept:
//!
//! 1. **Strukturierte Erfassung** – fester Fragebogen; fehlende Felder
//!    werden aktiv erfragt, statt vom Modell erfunden zu werden.
//! 2. **Bewertung entlang fester Rubrik** – 1–5-Punkte-Skala pro
//!    Kriterium, Pflichtbegründung, Herkunftsangabe (Nutzer / Dokument /
//!    Modellannahme). Das JSON-Schema kommt hier aus Rust; die
//!    GBNF-Grammatik in [`RUBRIC_GBNF`] erzwingt die Struktur formal
//!    für den Server.
//! 3. **Deterministische Auswertung** – gewichtete Gesamtpunktzahl in
//!    Rust plus Monte-Carlo-Simulation über Nutzerspannen (10 000 Läufe
//!    Standard). Das Modell rechnet nichts.

use serde::{Deserialize, Serialize};

/// Herkunft einer Bewertung (Konzept 4.2 Stufe 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    User,
    Document,
    Assumption,
}

impl EvidenceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceSource::User => "user",
            EvidenceSource::Document => "document",
            EvidenceSource::Assumption => "assumption",
        }
    }
}

/// Ein Bewertungskriterium mit Gewicht (0..1) und Kurzbeschreibung.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Criterion {
    pub id: String,
    pub label: String,
    /// 0.0 bis 1.0; die Summe aller Gewichte einer Rubrik sollte 1 sein
    /// (validiert in [`Rubric::validate`]).
    pub weight: f64,
    pub description: String,
}

/// Vollständige Rubrik.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rubric {
    pub id: String,
    pub label: String,
    pub criteria: Vec<Criterion>,
}

impl Rubric {
    /// Standardrubrik für „Ideenbewertung" nach Konzept 4.2.
    pub fn business_idea_default() -> Self {
        let criteria = [
            (
                "problem_sharpness",
                "Problemschärfe",
                0.15,
                "Wie klar ist das gelöste Problem?",
            ),
            (
                "willingness_to_pay",
                "Zahlungsbereitschaft",
                0.15,
                "Ist die Zielgruppe bereit zu zahlen?",
            ),
            (
                "market_access",
                "Marktzugang",
                0.10,
                "Wie erreicht die Lösung ihre Nutzer?",
            ),
            (
                "differentiation",
                "Differenzierung",
                0.10,
                "Womit hebt sich die Idee ab?",
            ),
            (
                "capital_intensity",
                "Kapitalintensität",
                0.10,
                "Wie viel Kapital braucht der Start?",
            ),
            (
                "time_to_revenue",
                "Time-to-Revenue",
                0.10,
                "Wie schnell entstehen Einnahmen?",
            ),
            (
                "regulatory_risk",
                "Regulatorisches Risiko",
                0.10,
                "Rechtliche Hürden und Auflagen",
            ),
            (
                "personal_fit",
                "Persönliche Passung",
                0.10,
                "Passt Idee zu Fähigkeiten und Motivation?",
            ),
            (
                "scalability",
                "Skalierbarkeit",
                0.10,
                "Wie skaliert das Modell?",
            ),
        ];
        let criteria = criteria
            .into_iter()
            .map(|(id, label, weight, description)| Criterion {
                id: id.to_owned(),
                label: label.to_owned(),
                weight,
                description: description.to_owned(),
            })
            .collect();
        Self {
            id: "business_idea".to_owned(),
            label: "Ideenbewertung".to_owned(),
            criteria,
        }
    }

    /// Prüft die Rubrik: Gewichtssumme ≈ 1, IDs eindeutig.
    pub fn validate(&self) -> Result<(), String> {
        let sum: f64 = self.criteria.iter().map(|c| c.weight).sum();
        if (sum - 1.0).abs() > 0.01 {
            return Err(format!("Gewichtssumme {sum} weicht von 1.0 ab"));
        }
        let mut ids: Vec<&str> = self.criteria.iter().map(|c| c.id.as_str()).collect();
        ids.sort_unstable();
        for pair in ids.windows(2) {
            if pair[0] == pair[1] {
                return Err(format!("doppelte Kriterien-ID `{}`", pair[0]));
            }
        }
        Ok(())
    }
}

/// Antworten auf den Erfassungs-Fragebogen (Konzept 4.2 Stufe 1).
///
/// Alle Felder sind optional, damit die UI unvollständige Eingaben
/// speichern und später ergänzen kann. Die Bewertung darf nur mit
/// vollständigem Datensatz laufen ([`QuestionnaireResponses::missing_fields`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestionnaireResponses {
    pub problem: Option<String>,
    pub audience: Option<String>,
    pub solution: Option<String>,
    pub revenue_model: Option<String>,
    pub cost_drivers: Option<String>,
    pub competition: Option<String>,
    pub unfair_advantage: Option<String>,
    pub regulatory_context: Option<String>,
    pub required_capital: Option<String>,
    pub time_to_first_revenue: Option<String>,
}

impl QuestionnaireResponses {
    /// Fehlende Pflichtfelder; die UI muss dem Nutzer diese aktiv stellen,
    /// bevor die LLM-Rubrik läuft.
    pub fn missing_fields(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.problem.as_deref().unwrap_or("").trim().is_empty() {
            missing.push("problem");
        }
        if self.audience.as_deref().unwrap_or("").trim().is_empty() {
            missing.push("audience");
        }
        if self.solution.as_deref().unwrap_or("").trim().is_empty() {
            missing.push("solution");
        }
        if self
            .revenue_model
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            missing.push("revenue_model");
        }
        if self.cost_drivers.as_deref().unwrap_or("").trim().is_empty() {
            missing.push("cost_drivers");
        }
        missing
    }
}

/// Bewertung eines einzelnen Kriteriums (Konzept 4.2 Stufe 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CriterionScore {
    pub criterion_id: String,
    /// 1..5 (validiert bei Aufnahme).
    pub score: u8,
    pub rationale: String,
    pub source: EvidenceSource,
}

/// Vollständige LLM-Antwort für Stufe 2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RubricScoreSheet {
    pub scores: Vec<CriterionScore>,
}

impl RubricScoreSheet {
    /// Validiert gegen die Rubrik: jedes Kriterium genau einmal, Score
    /// in 1..=5. Wird sowohl vor der Rechenauswertung als auch beim
    /// Anzeigen in der UI benutzt.
    pub fn validate(&self, rubric: &Rubric) -> Result<(), String> {
        for score in &self.scores {
            if !(1..=5).contains(&score.score) {
                return Err(format!(
                    "Score {} für Kriterium `{}` liegt nicht in 1..5",
                    score.score, score.criterion_id
                ));
            }
            if score.rationale.trim().is_empty() {
                return Err(format!(
                    "Kriterium `{}` hat keine Begründung",
                    score.criterion_id
                ));
            }
        }
        for criterion in &rubric.criteria {
            let count = self
                .scores
                .iter()
                .filter(|s| s.criterion_id == criterion.id)
                .count();
            if count == 0 {
                return Err(format!("Kriterium `{}` fehlt", criterion.id));
            }
            if count > 1 {
                return Err(format!("Kriterium `{}` mehrfach vorhanden", criterion.id));
            }
        }
        Ok(())
    }
}

/// Gewichtete Auswertung (Konzept 4.2 Stufe 3, deterministisch).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightedResult {
    /// Summe(score_i * gewicht_i) — liegt zwischen 1 und 5.
    pub weighted_score: f64,
    /// Gleiche Zahl in 0..1 (`(weighted_score - 1) / 4`) für Balken.
    pub normalized: f64,
    /// Für die UI: die einzelnen Beiträge.
    pub contributions: Vec<WeightedContribution>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightedContribution {
    pub criterion_id: String,
    pub label: String,
    pub score: u8,
    pub weight: f64,
    pub contribution: f64,
}

impl WeightedResult {
    /// Determinismus-Test-freundliche Konstruktion. Weight- und Score-
    /// Validierung liegt beim Aufrufer (`Rubric::validate`,
    /// `RubricScoreSheet::validate`).
    pub fn from(rubric: &Rubric, sheet: &RubricScoreSheet) -> Self {
        let mut contributions = Vec::with_capacity(rubric.criteria.len());
        let mut weighted = 0.0_f64;
        for criterion in &rubric.criteria {
            let score = sheet
                .scores
                .iter()
                .find(|s| s.criterion_id == criterion.id)
                .map(|s| s.score)
                .unwrap_or(0);
            let contribution = score as f64 * criterion.weight;
            weighted += contribution;
            contributions.push(WeightedContribution {
                criterion_id: criterion.id.clone(),
                label: criterion.label.clone(),
                score,
                weight: criterion.weight,
                contribution,
            });
        }
        WeightedResult {
            weighted_score: weighted,
            normalized: ((weighted - 1.0) / 4.0).clamp(0.0, 1.0),
            contributions,
        }
    }
}

/// Nutzer-Spanne für die Monte-Carlo-Simulation (Konzept 4.2 Stufe 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonteCarloInput {
    pub price_per_unit: RangeInput,
    pub units_per_month: RangeInput,
    pub fixed_cost_per_month: RangeInput,
    pub variable_cost_per_unit: RangeInput,
    pub starting_capital: f64,
}

/// Uniform-verteilte Spanne. `min` ≤ `max`; der Solver kürzt sonst.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RangeInput {
    pub min: f64,
    pub max: f64,
}

impl RangeInput {
    pub fn sample(self, u01: f64) -> f64 {
        let (lo, hi) = if self.min <= self.max {
            (self.min, self.max)
        } else {
            (self.max, self.min)
        };
        lo + (hi - lo) * u01.clamp(0.0, 1.0)
    }
}

/// Simulationsergebnis; enthält Perzentile für die UI-Verteilungsanzeige.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub runs: u32,
    pub monthly_profit_p10: f64,
    pub monthly_profit_p50: f64,
    pub monthly_profit_p90: f64,
    pub probability_positive_month: f64,
    pub expected_break_even_months: Option<f64>,
    pub expected_runway_months: Option<f64>,
}

/// Führt eine Simulation durch.
///
/// `seed` speist einen eigenen kleinen PRNG (splitmix64), damit das
/// Ergebnis deterministisch reproduzierbar bleibt — wichtig für die
/// UI-Anzeige und für Regressionstests.
pub fn run_monte_carlo(input: &MonteCarloInput, runs: u32, seed: u64) -> MonteCarloResult {
    let mut state = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut sample = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1_u64 << 53) as f64
    };

    let mut profits = Vec::with_capacity(runs as usize);
    let mut positives = 0_u32;
    let mut break_even_sum = 0.0_f64;
    let mut break_even_count = 0_u32;
    let mut runway_sum = 0.0_f64;
    let mut runway_count = 0_u32;
    for _ in 0..runs {
        let price = input.price_per_unit.sample(sample());
        let units = input.units_per_month.sample(sample());
        let variable = input.variable_cost_per_unit.sample(sample());
        let fixed = input.fixed_cost_per_month.sample(sample());
        let profit = (price - variable) * units - fixed;
        profits.push(profit);
        if profit > 0.0 {
            positives += 1;
            let months = if profit > 0.0 {
                input.starting_capital / profit
            } else {
                f64::INFINITY
            };
            if months.is_finite() {
                break_even_sum += months.max(0.0);
                break_even_count += 1;
            }
        } else if input.starting_capital > 0.0 && profit < 0.0 {
            let runway = input.starting_capital / -profit;
            if runway.is_finite() {
                runway_sum += runway.max(0.0);
                runway_count += 1;
            }
        }
    }
    profits.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p = |q: f64| {
        if profits.is_empty() {
            0.0
        } else {
            let idx = ((profits.len() as f64 - 1.0) * q).round() as usize;
            profits[idx.min(profits.len() - 1)]
        }
    };
    MonteCarloResult {
        runs,
        monthly_profit_p10: p(0.10),
        monthly_profit_p50: p(0.50),
        monthly_profit_p90: p(0.90),
        probability_positive_month: positives as f64 / runs.max(1) as f64,
        expected_break_even_months: if break_even_count > 0 {
            Some(break_even_sum / break_even_count as f64)
        } else {
            None
        },
        expected_runway_months: if runway_count > 0 {
            Some(runway_sum / runway_count as f64)
        } else {
            None
        },
    }
}

/// GBNF für die LLM-Antwort in Stufe 2.
///
/// Erzwingt Struktur, Score-Range 1..5 und die drei erlaubten
/// Herkunfts-Bezeichner.
pub const RUBRIC_GBNF: &str = r#"
root       ::= "{\"scores\":" scores "}"
scores     ::= "[]" | "[" score ("," score)* "]"
score      ::= "{"
                 "\"criterion_id\":" string ","
                 "\"score\":" score_val ","
                 "\"rationale\":" string ","
                 "\"source\":" source
               "}"
score_val  ::= "1" | "2" | "3" | "4" | "5"
source     ::= "\"user\"" | "\"document\"" | "\"assumption\""
string     ::= "\"" schar* "\""
schar      ::= [^"\\] | "\\" ["\\/bfnrt] | "\\u" [0-9a-fA-F]{4}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn full_sheet(rubric: &Rubric, score: u8) -> RubricScoreSheet {
        RubricScoreSheet {
            scores: rubric
                .criteria
                .iter()
                .map(|c| CriterionScore {
                    criterion_id: c.id.clone(),
                    score,
                    rationale: "Test".into(),
                    source: EvidenceSource::User,
                })
                .collect(),
        }
    }

    #[test]
    fn default_business_rubric_validates() {
        let rubric = Rubric::business_idea_default();
        rubric.validate().unwrap();
    }

    #[test]
    fn missing_fields_are_reported_by_questionnaire() {
        let mut responses = QuestionnaireResponses {
            problem: Some("klar".into()),
            audience: Some("KMU".into()),
            solution: Some("SaaS".into()),
            revenue_model: Some("Abo".into()),
            cost_drivers: Some("Team".into()),
            ..QuestionnaireResponses::default()
        };
        assert!(responses.missing_fields().is_empty());
        responses.solution = None;
        assert_eq!(responses.missing_fields(), vec!["solution"]);
    }

    #[test]
    fn score_sheet_validates_range_and_completeness() {
        let rubric = Rubric::business_idea_default();
        let sheet = full_sheet(&rubric, 3);
        sheet.validate(&rubric).unwrap();
        let mut bad = sheet.clone();
        bad.scores[0].score = 6;
        assert!(bad.validate(&rubric).is_err());
        let mut missing = sheet;
        missing.scores.pop();
        assert!(missing.validate(&rubric).is_err());
    }

    #[test]
    fn weighted_result_matches_arithmetic() {
        let rubric = Rubric::business_idea_default();
        let sheet = full_sheet(&rubric, 4);
        let result = WeightedResult::from(&rubric, &sheet);
        assert!((result.weighted_score - 4.0).abs() < 1e-9);
        assert!((result.normalized - 0.75).abs() < 1e-9);
    }

    #[test]
    fn monte_carlo_is_deterministic_for_the_same_seed() {
        let input = MonteCarloInput {
            price_per_unit: RangeInput {
                min: 10.0,
                max: 20.0,
            },
            units_per_month: RangeInput {
                min: 100.0,
                max: 200.0,
            },
            fixed_cost_per_month: RangeInput {
                min: 500.0,
                max: 1000.0,
            },
            variable_cost_per_unit: RangeInput { min: 2.0, max: 5.0 },
            starting_capital: 10_000.0,
        };
        let a = run_monte_carlo(&input, 5000, 42);
        let b = run_monte_carlo(&input, 5000, 42);
        assert_eq!(a, b);
    }

    #[test]
    fn monte_carlo_reports_positive_probability_for_profitable_ranges() {
        let input = MonteCarloInput {
            price_per_unit: RangeInput {
                min: 100.0,
                max: 120.0,
            },
            units_per_month: RangeInput {
                min: 50.0,
                max: 100.0,
            },
            fixed_cost_per_month: RangeInput {
                min: 500.0,
                max: 800.0,
            },
            variable_cost_per_unit: RangeInput {
                min: 5.0,
                max: 10.0,
            },
            starting_capital: 5_000.0,
        };
        let result = run_monte_carlo(&input, 10_000, 7);
        assert!(result.probability_positive_month > 0.95);
        assert!(result.monthly_profit_p50 > 0.0);
        assert!(result.expected_break_even_months.is_some());
    }

    #[test]
    fn range_sample_clamps_out_of_bound_u01() {
        let r = RangeInput {
            min: 0.0,
            max: 10.0,
        };
        assert_eq!(r.sample(-1.0), 0.0);
        assert_eq!(r.sample(2.0), 10.0);
    }

    #[test]
    fn inverted_range_still_produces_valid_samples() {
        let r = RangeInput {
            min: 10.0,
            max: 0.0,
        };
        let value = r.sample(0.5);
        assert!((0.0..=10.0).contains(&value));
    }
}
