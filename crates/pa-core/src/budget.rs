//! Deterministischer Kontextbudget-Manager mit priorisiertem Kürzen.
//!
//! Der Manager wählt nach fester Priorität aus, welche Nachrichten in das
//! begrenzte Kontextfenster fließen:
//!
//! 1. Präfixblöcke (Systemidentität → Nutzerprofil → Werkzeugdefinitionen →
//!    variable Kontexte) bekommen ein hartes Reservat und werden nie gekürzt.
//! 2. Die aktuelle Nutzernachricht (das letzte Element im Verlauf) bekommt
//!    ihr geschätztes Tokenbudget plus eine konfigurierbare Antwortreserve
//!    vorab reserviert.
//! 3. Vom Restbudget werden ältere `(user, assistant)`-Paare von hinten nach
//!    vorn aufgenommen, solange Platz bleibt. Beim Überlauf werden die
//!    ältesten Paare gemeinsam verworfen.
//! 4. Passt schon Präfix plus aktuelle Nutzernachricht nicht mehr in das
//!    Kontextfenster, liefert der Manager [`CoreError::PromptExceedsContext`]
//!    – kein stiller Verlust der Eingabe.

use pa_types::chat::{Message, MessageRole};

use crate::{prompt::PromptPrefix, CoreError};

/// Schätzt Tokens deterministisch aus reinem Text.
///
/// Wird als Trait geführt, damit später ein echter Tokenizer eingehängt werden
/// kann, ohne den Orchestrator anzurühren.
pub trait TokenEstimator {
    fn estimate(&self, text: &str) -> u32;
}

/// Konservative Standardheuristik ohne externe Abhängigkeit: drei Zeichen
/// werden als ein Token gewertet und immer aufgerundet.
///
/// Die Wahl ist als Obergrenze gedacht: bei deutschem oder englischem Text
/// erzeugt Gemma 4 typischerweise deutlich weniger als ein Token pro drei
/// Zeichen. Ein späterer echter Tokenizer darf niedriger schätzen.
pub struct HeuristicTokenEstimator;

impl TokenEstimator for HeuristicTokenEstimator {
    fn estimate(&self, text: &str) -> u32 {
        let chars = text.chars().count() as u64;
        chars.div_ceil(3).min(u32::MAX as u64) as u32
    }
}

/// Fasst die Budget-Vorgaben zusammen, mit denen der Manager arbeitet.
#[derive(Debug, Clone, Copy)]
pub struct BudgetPolicy {
    /// Gesamtes Kontextfenster in Tokens, wie es der Server konfiguriert wurde.
    pub context_tokens: u32,
    /// Tokens, die für die Antwort freigehalten werden; verhindert, dass ein
    /// vollständig ausgefüllter Prompt keinen Platz für Ausgabe lässt.
    pub response_reserve_tokens: u32,
    /// Grober Fixaufschlag pro Nachricht für Chat-Template-Marker (Rollentag,
    /// Umbrüche); vermeidet ein zu knappes Restbudget bei vielen kleinen Turns.
    pub tokens_per_message_overhead: u32,
}

impl BudgetPolicy {
    /// Setzt konservative Werte, die zum in Schritt 3 gemessenen Verhalten passen.
    ///
    /// `response_reserve_tokens` = 512 spiegelt eine typische Antwortlänge auf
    /// T0 wider; `tokens_per_message_overhead` = 8 lässt Raum für Gemmas
    /// `<start_of_turn>`- und `<end_of_turn>`-Marker.
    pub fn new(context_tokens: u32) -> Self {
        Self {
            context_tokens,
            response_reserve_tokens: 512,
            tokens_per_message_overhead: 8,
        }
    }

    /// Überschreibt die Antwortreserve; für UI-Einstellungen oder Tests.
    pub fn with_response_reserve(mut self, tokens: u32) -> Self {
        self.response_reserve_tokens = tokens;
        self
    }

    /// Überschreibt den Nachrichten-Overhead; für Tests mit einem anderen Modell-Template.
    pub fn with_message_overhead(mut self, tokens: u32) -> Self {
        self.tokens_per_message_overhead = tokens;
        self
    }
}

/// Ergebnis des Budget-Managers.
#[derive(Debug, Clone)]
pub struct BudgetedPrompt {
    /// Vollständige, gerenderte Nachrichtenliste in der Reihenfolge, in der
    /// sie an die Engine übergeben wird (Präfix → Verlauf → aktueller Turn).
    pub messages: Vec<Message>,
    /// Summierte, geschätzte Tokenkosten aller enthaltenen Nachrichten inklusive
    /// Präfix und Nachrichten-Overhead. Ohne Antwortreserve.
    pub used_tokens: u32,
    /// Tokens, die für die Antwort reserviert wurden.
    pub reserved_response_tokens: u32,
    /// Zahl der ältesten `(user, assistant)`-Paare, die verworfen wurden.
    pub dropped_older_turns: usize,
}

/// Baut die budgetierte Nachrichtenliste für einen einzelnen Engine-Aufruf.
///
/// `history` muss mit einer Nachricht der Rolle `User` enden; das ist die
/// aktuell zu beantwortende Nutzereingabe und darf nie gekürzt werden. Ältere
/// Turns davor müssen der Reihenfolge `user, assistant, user, assistant, …`
/// folgen, wie sie im Schritt-3-CLI und in `pa-vault` persistiert wird.
pub fn build_prompt(
    prefix: &PromptPrefix,
    history: &[Message],
    policy: BudgetPolicy,
    estimator: &dyn TokenEstimator,
) -> Result<BudgetedPrompt, CoreError> {
    if policy.context_tokens == 0 {
        return Err(CoreError::InvalidConfiguration(
            "context_tokens muss größer als null sein".to_owned(),
        ));
    }
    let (current, older) = history.split_last().ok_or_else(|| {
        CoreError::InvalidConfiguration(
            "Verlauf muss mindestens die aktuelle Nachricht enthalten".to_owned(),
        )
    })?;
    if current.role != MessageRole::User {
        return Err(CoreError::InvalidConfiguration(
            "letzte Nachricht muss die Rolle User haben".to_owned(),
        ));
    }
    let prefix_messages = prefix.rendered_messages();
    let prefix_cost = message_cost(
        &prefix_messages,
        policy.tokens_per_message_overhead,
        estimator,
    );
    let current_cost = single_message_cost(current, policy.tokens_per_message_overhead, estimator);
    let reserved = policy.response_reserve_tokens;

    let required = prefix_cost
        .saturating_add(current_cost)
        .saturating_add(reserved);
    if required > policy.context_tokens {
        return Err(CoreError::PromptExceedsContext {
            needed_tokens: required,
            available_tokens: policy.context_tokens,
            context_tokens: policy.context_tokens,
        });
    }

    let mut remaining = policy.context_tokens - required;
    let mut kept_reversed: Vec<Message> = Vec::new();
    let mut dropped_older_turns = 0_usize;
    let mut iter = older.iter().rev().peekable();

    while let Some(assistant) = iter.next() {
        // Ohne strikte Paarung wird ein einzeln stehender assistant/user-Rest
        // verworfen, damit das erste behaltene Element ein User-Turn bleibt.
        let Some(user) = iter.next() else {
            dropped_older_turns += 1;
            break;
        };
        if user.role != MessageRole::User || assistant.role == MessageRole::User {
            // Bricht ab, sobald das erwartete (User, Assistant)-Muster reißt;
            // spätere Runden im MVP folgen strikt diesem Muster.
            dropped_older_turns += 1;
            break;
        }
        let pair_cost =
            single_message_cost(assistant, policy.tokens_per_message_overhead, estimator)
                .saturating_add(single_message_cost(
                    user,
                    policy.tokens_per_message_overhead,
                    estimator,
                ));
        if pair_cost > remaining {
            // Nicht mehr genug Platz für weiteres Paar: verbleibende Runden verwerfen.
            dropped_older_turns += 1;
            dropped_older_turns += iter.by_ref().count() / 2;
            break;
        }
        remaining -= pair_cost;
        kept_reversed.push(assistant.clone());
        kept_reversed.push(user.clone());
    }

    let mut messages = prefix_messages;
    messages.extend(kept_reversed.into_iter().rev());
    messages.push(current.clone());

    let used_tokens = message_cost(&messages, policy.tokens_per_message_overhead, estimator);
    Ok(BudgetedPrompt {
        messages,
        used_tokens,
        reserved_response_tokens: reserved,
        dropped_older_turns,
    })
}

fn single_message_cost(message: &Message, overhead: u32, estimator: &dyn TokenEstimator) -> u32 {
    estimator
        .estimate(&message.content)
        .saturating_add(overhead)
}

fn message_cost(messages: &[Message], overhead: u32, estimator: &dyn TokenEstimator) -> u32 {
    messages.iter().fold(0_u32, |total, message| {
        total.saturating_add(single_message_cost(message, overhead, estimator))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_types::chat::MessageStatus;

    fn user_message(position: i64, content: &str) -> Message {
        Message {
            id: format!("m-{position}"),
            conversation_id: "c".to_owned(),
            position,
            role: MessageRole::User,
            content: content.to_owned(),
            status: MessageStatus::Complete,
            created_at_unix_ms: position * 1000,
        }
    }

    fn assistant_message(position: i64, content: &str) -> Message {
        Message {
            id: format!("m-{position}"),
            conversation_id: "c".to_owned(),
            position,
            role: MessageRole::Assistant,
            content: content.to_owned(),
            status: MessageStatus::Complete,
            created_at_unix_ms: position * 1000,
        }
    }

    #[test]
    fn heuristic_estimator_is_monotone_and_rounds_up() {
        let estimator = HeuristicTokenEstimator;
        assert_eq!(estimator.estimate(""), 0);
        assert_eq!(estimator.estimate("abc"), 1);
        assert_eq!(estimator.estimate("abcd"), 2);
        assert!(estimator.estimate("ein längerer Text") >= estimator.estimate("ein"));
    }

    #[test]
    fn only_current_user_message_fits_prefix_but_no_history_returns_no_older_turns() {
        let prefix = PromptPrefix::empty().with_system_identity("Systemidentität");
        let history = vec![user_message(0, "Hallo")];
        let policy = BudgetPolicy::new(8192).with_response_reserve(256);
        let estimator = HeuristicTokenEstimator;
        let prompt = build_prompt(&prefix, &history, policy, &estimator).unwrap();
        assert_eq!(prompt.dropped_older_turns, 0);
        assert_eq!(prompt.messages.len(), 2); // prefix + current
        assert_eq!(prompt.messages.last().unwrap().content, "Hallo");
        assert_eq!(prompt.reserved_response_tokens, 256);
    }

    #[test]
    fn budget_drops_oldest_pairs_first() {
        let prefix = PromptPrefix::empty();
        let long = "a".repeat(3 * 100); // 100 Tokens laut Heuristik
        let history = vec![
            user_message(0, &long),
            assistant_message(1, &long),
            user_message(2, &long),
            assistant_message(3, &long),
            user_message(4, "aktuell"), // 1 Token
        ];
        // Wenige Tokens Platz nach Präfix und Reserve → nur die neueste Runde plus
        // ein älteres Paar sollte passen.
        let policy = BudgetPolicy::new(300)
            .with_response_reserve(20)
            .with_message_overhead(0);
        let estimator = HeuristicTokenEstimator;
        let prompt = build_prompt(&prefix, &history, policy, &estimator).unwrap();
        assert_eq!(prompt.dropped_older_turns, 1);
        // 1 (aktuell) + 100 (user2) + 100 (assistant1) = 201 – passt
        assert!(prompt.used_tokens <= 300 - 20);
        let contents: Vec<&str> = prompt.messages.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(contents.last(), Some(&"aktuell"));
        // Ältestes Paar (user0, assistant1) wurde verworfen; user2 und assistant3 bleiben.
        assert!(contents.contains(&long.as_str()));
    }

    #[test]
    fn current_message_over_context_returns_hard_error() {
        let prefix = PromptPrefix::empty().with_system_identity("x".repeat(300).as_str());
        let history = vec![user_message(0, "y".repeat(3000).as_str())];
        let policy = BudgetPolicy::new(64)
            .with_message_overhead(0)
            .with_response_reserve(0);
        let estimator = HeuristicTokenEstimator;
        let error = build_prompt(&prefix, &history, policy, &estimator).unwrap_err();
        match error {
            CoreError::PromptExceedsContext {
                needed_tokens,
                available_tokens,
                context_tokens,
            } => {
                assert!(needed_tokens > available_tokens);
                assert_eq!(available_tokens, 64);
                assert_eq!(context_tokens, 64);
            }
            other => panic!("unerwarteter Fehler: {other:?}"),
        }
    }

    #[test]
    fn empty_history_is_rejected_because_no_current_message_exists() {
        let prefix = PromptPrefix::empty();
        let policy = BudgetPolicy::new(1024);
        let estimator = HeuristicTokenEstimator;
        let error = build_prompt(&prefix, &[], policy, &estimator).unwrap_err();
        assert!(matches!(error, CoreError::InvalidConfiguration(_)));
    }

    #[test]
    fn last_message_must_be_user() {
        let prefix = PromptPrefix::empty();
        let history = vec![assistant_message(0, "irgendetwas")];
        let policy = BudgetPolicy::new(1024);
        let estimator = HeuristicTokenEstimator;
        let error = build_prompt(&prefix, &history, policy, &estimator).unwrap_err();
        assert!(matches!(error, CoreError::InvalidConfiguration(_)));
    }

    #[test]
    fn zero_context_configuration_is_rejected() {
        let prefix = PromptPrefix::empty();
        let history = vec![user_message(0, "Hi")];
        let policy = BudgetPolicy::new(0);
        let estimator = HeuristicTokenEstimator;
        let error = build_prompt(&prefix, &history, policy, &estimator).unwrap_err();
        assert!(matches!(error, CoreError::InvalidConfiguration(_)));
    }

    #[test]
    fn prefix_bytes_stay_identical_after_repeated_builds() {
        let prefix = PromptPrefix::empty()
            .with_system_identity("PortableAI")
            .with_user_profile("Nutzer arbeitet mit Rust");
        let history = vec![user_message(0, "Frage")];
        let policy = BudgetPolicy::new(2048);
        let estimator = HeuristicTokenEstimator;
        let first = build_prompt(&prefix, &history, policy, &estimator).unwrap();
        let second = build_prompt(&prefix, &history, policy, &estimator).unwrap();
        assert_eq!(first.messages.len(), second.messages.len());
        for (a, b) in first.messages.iter().zip(second.messages.iter()) {
            assert_eq!(a.role, b.role);
            assert_eq!(a.content, b.content);
        }
    }
}
