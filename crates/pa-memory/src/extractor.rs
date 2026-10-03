//! Trait-Port für Faktenextraktion.
//!
//! Die eigentliche LLM-Anfrage lebt außerhalb dieser Crate — hier wird nur
//! beschrieben, wann Extraktion ausgelöst wird (Konzept 6.4) und in
//! welchem Format sie ihr Ergebnis abliefert. Das ist wichtig, damit
//! `pa-memory` die Duplikatserkennung ohne LLM-Aufruf testen kann.

use pa_types::memory::FactCategory;
use thiserror::Error;

/// Warum die Extraktion angestoßen wurde. Wird in Audit und Extraktor-Prompt
/// mit übergeben, damit der Nutzer im Memory-Bereich später nachvollziehen
/// kann, wieso ein Fakt aufgenommen wurde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerReason {
    /// Konzept 6.4: „bei Session-Ende oder nach 10 Nachrichten"
    SessionEnd,
    /// „sofort, wenn der Nutzer explizit ‚merk dir' sagt"
    ExplicitMemorize,
    /// „im Leerlauf, wenn der Nutzer nichts eingibt"
    IdleQueue,
}

/// Ein Kandidat, den der Extraktor zurückgibt. Die spätere Duplikats-/
/// Widerspruchsprüfung wird gegen bestehende Fakten laufen; der Extraktor
/// selbst darf ID- und Confidence-Werte vergeben, die pa-memory nur bei
/// Bedarf überschreibt.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedFact {
    pub text: String,
    pub category: FactCategory,
    pub confidence: f32,
    /// ID der Nachricht, aus der der Fakt abgeleitet wurde.
    pub source_message_id: Option<String>,
}

/// Fehler eines Extraktor-Aufrufs; werden 1:1 durchgereicht.
#[derive(Debug, Error)]
pub enum FactExtractorError {
    #[error("LLM nicht erreichbar: {0}")]
    Inference(String),
    #[error("Antwort konnte nicht als Faktenliste geparst werden: {0}")]
    Parse(String),
    #[error("Extraktor lieferte unerwarteten Fehler: {0}")]
    Other(String),
}

/// Wird von `pa-inference` implementiert. Der Aufrufer übergibt einen
/// gerenderten Verlauf (z. B. die letzten N Nachrichten) und den Anlass
/// für die Extraktion; die Implementation liefert die Kandidatenliste.
pub trait FactExtractor: Send + Sync {
    /// Extrahiert Faktenkandidaten aus dem übergebenen Verlauf.
    fn extract(
        &self,
        conversation_snippet: &str,
        trigger: TriggerReason,
    ) -> Result<Vec<ExtractedFact>, FactExtractorError>;
}

/// Deterministischer Test-Extraktor: findet Zeilen der Form
/// „merk dir: <text>" und trägt sie als `Preference`-Kandidaten ein.
/// Kein Ersatz für ein echtes Modell, aber ausreichend, um die
/// Duplikatspfade in pa-memory ohne LLM zu testen.
pub struct RegexMemorizeExtractor;

impl FactExtractor for RegexMemorizeExtractor {
    fn extract(
        &self,
        conversation_snippet: &str,
        _trigger: TriggerReason,
    ) -> Result<Vec<ExtractedFact>, FactExtractorError> {
        let mut extracted = Vec::new();
        for line in conversation_snippet.lines() {
            let trimmed = line.trim();
            if let Some(remainder) = trimmed.strip_prefix("merk dir: ") {
                if !remainder.is_empty() {
                    extracted.push(ExtractedFact {
                        text: remainder.to_owned(),
                        category: FactCategory::Preference,
                        confidence: 1.0,
                        source_message_id: None,
                    });
                }
            }
        }
        Ok(extracted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_extractor_picks_out_memorize_lines_only() {
        let extractor = RegexMemorizeExtractor;
        let snippet = "merk dir: Ich mag Kaffee\n\
                       das war nur so ein Gedanke\n\
                       merk dir: Der Server heißt gemma-e2b";
        let facts = extractor
            .extract(snippet, TriggerReason::ExplicitMemorize)
            .unwrap();
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].text, "Ich mag Kaffee");
        assert_eq!(facts[1].text, "Der Server heißt gemma-e2b");
        for fact in facts {
            assert_eq!(fact.category, FactCategory::Preference);
            assert!((fact.confidence - 1.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn regex_extractor_returns_empty_on_no_matches() {
        let extractor = RegexMemorizeExtractor;
        let facts = extractor
            .extract("nur normaler Chat", TriggerReason::IdleQueue)
            .unwrap();
        assert!(facts.is_empty());
    }
}
