//! Feste, harmlose Sprachbefehle für die Navigation.
//!
//! Erkannt wird nur, wenn die **gesamte** Äußerung (nach Normalisierung) genau
//! einem Satz aus der Liste entspricht. Bei jeder Unsicherheit bleibt der Text
//! als normale, editierbare Eingabe stehen und löst nichts aus. Befehle können
//! nur die Ansicht wechseln – Datei-, Prozess-, Agenten- und Exa-Aktionen
//! haben keinen Sprachbefehl und behalten ihre eigenen Freigaben.

use pa_types::voice::VoiceCommand;

/// Normalisiert eine Äußerung: Kleinbuchstaben, ohne Satzzeichen, einfache Leerzeichen.
fn normalize(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();
    cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

const PHRASES: &[(&str, VoiceCommand, &[&str])] = &[
    (
        "de",
        VoiceCommand::OpenChat,
        &["chat öffnen", "öffne den chat", "zum chat", "chat anzeigen"],
    ),
    (
        "de",
        VoiceCommand::OpenFlow,
        &[
            "flow version öffnen",
            "öffne flow version",
            "flow version",
            "flow version anzeigen",
        ],
    ),
    (
        "de",
        VoiceCommand::OpenWorkflows,
        &[
            "workflows anzeigen",
            "workflows öffnen",
            "zeige die workflows",
            "öffne die workflows",
        ],
    ),
    (
        "de",
        VoiceCommand::OpenHome,
        &[
            "startseite",
            "zur startseite",
            "startseite öffnen",
            "nach hause",
        ],
    ),
    (
        "en",
        VoiceCommand::OpenChat,
        &["open chat", "show chat", "go to chat"],
    ),
    (
        "en",
        VoiceCommand::OpenFlow,
        &["open flow version", "flow version", "show flow version"],
    ),
    (
        "en",
        VoiceCommand::OpenWorkflows,
        &["show workflows", "open workflows"],
    ),
    (
        "en",
        VoiceCommand::OpenHome,
        &["home", "go home", "open home", "start page"],
    ),
    (
        "es",
        VoiceCommand::OpenChat,
        &["abrir chat", "abre el chat", "ir al chat"],
    ),
    (
        "es",
        VoiceCommand::OpenFlow,
        &["abrir flow version", "abre flow version"],
    ),
    (
        "es",
        VoiceCommand::OpenWorkflows,
        &["mostrar workflows", "abrir workflows"],
    ),
    (
        "es",
        VoiceCommand::OpenHome,
        &["inicio", "ir al inicio", "página de inicio"],
    ),
    (
        "fr",
        VoiceCommand::OpenChat,
        &["ouvrir le chat", "ouvre le chat", "aller au chat"],
    ),
    (
        "fr",
        VoiceCommand::OpenFlow,
        &["ouvrir flow version", "ouvre flow version"],
    ),
    (
        "fr",
        VoiceCommand::OpenWorkflows,
        &["afficher les workflows", "ouvrir les workflows"],
    ),
    (
        "fr",
        VoiceCommand::OpenHome,
        &["accueil", "page d accueil", "aller à l accueil"],
    ),
    (
        "ja",
        VoiceCommand::OpenChat,
        &["チャットを開く", "チャットを開いて"],
    ),
    (
        "ja",
        VoiceCommand::OpenFlow,
        &["フロー バージョンを開く", "フローバージョンを開く"],
    ),
    (
        "ja",
        VoiceCommand::OpenWorkflows,
        &["ワークフローを表示", "ワークフローを開く"],
    ),
    ("ja", VoiceCommand::OpenHome, &["ホーム", "ホームを開く"]),
];

/// Erkennt einen Befehl in der Sprache der Erkennung. Nur exakte Treffer.
pub fn parse(text: &str, language: &str) -> Option<VoiceCommand> {
    let normalized = normalize(text);
    if normalized.is_empty() {
        return None;
    }
    PHRASES
        .iter()
        .filter(|(lang, _, _)| *lang == language)
        .find(|(_, _, phrases)| phrases.iter().any(|p| normalize(p) == normalized))
        .map(|(_, command, _)| *command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_phrases_are_recognized_ignoring_case_and_punctuation() {
        assert_eq!(parse("Chat öffnen.", "de"), Some(VoiceCommand::OpenChat));
        assert_eq!(
            parse(" Flow-Version öffnen! ", "de"),
            Some(VoiceCommand::OpenFlow)
        );
        assert_eq!(
            parse("Show workflows", "en"),
            Some(VoiceCommand::OpenWorkflows)
        );
        assert_eq!(parse("Startseite", "de"), Some(VoiceCommand::OpenHome));
    }

    #[test]
    fn longer_or_partial_sentences_are_never_commands() {
        assert_eq!(
            parse("Bitte den Chat öffnen und dann Dateien löschen", "de"),
            None
        );
        assert_eq!(parse("chat", "de"), None);
        assert_eq!(parse("öffne den chat und lösche alles", "de"), None);
    }

    #[test]
    fn dangerous_actions_have_no_voice_command() {
        for text in [
            "lösche alle dateien",
            "starte den workflow",
            "sende die suche an exa",
            "führe den test aus",
            "übernimm den gewinner",
        ] {
            assert_eq!(parse(text, "de"), None, "{text}");
        }
    }

    #[test]
    fn phrase_of_another_language_does_not_match() {
        assert_eq!(parse("open chat", "de"), None);
        assert_eq!(parse("", "de"), None);
    }
}
