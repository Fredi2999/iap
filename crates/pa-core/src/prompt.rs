//! Stabile Struktur des Prompt-Präfixes nach Konzept Kap. 5.3.
//!
//! Die Reihenfolge (Systemidentität → Nutzerprofil → Werkzeugdefinitionen →
//! Cache-Grenze → variable Kontexte → Verlauf → aktuelle Rolle) sorgt dafür,
//! dass llama.cpp den KV-Cache für den Präfix zwischen Aufrufen wiederverwenden
//! kann, solange die stabilen Blöcke inhaltlich unverändert bleiben. Änderungen
//! hinter der Cache-Grenze kosten dann nur die Verarbeitung ihres eigenen
//! Blocks statt des kompletten Prompts.

use pa_types::chat::{Message, MessageRole, MessageStatus};

/// Trennt die stabilen Präfixblöcke von den je nach Auftrag variablen Kontexten.
///
/// Alle Felder sind optional und im MVP zumeist leer; sie sind bewusst getrennt
/// aufgeführt, damit spätere Phasen sie befüllen können, ohne die
/// Cache-Präfix-Reihenfolge zu brechen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptPrefix {
    system_identity: Option<String>,
    user_profile: Option<String>,
    tool_definitions: Option<String>,
    variable_contexts: Vec<String>,
}

impl PromptPrefix {
    /// Erzeugt einen leeren Präfix, wie ihn der Schritt-3-CLI verwendet.
    ///
    /// Das entspricht dem Verhalten vor der Einführung von `pa-core` und
    /// überlässt dem Modelladapter die Wahl eines minimalen Systemtextes.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Setzt den stabilen Systemidentitätsblock; er sollte über Sessions hinweg
    /// konstant bleiben, damit der KV-Cache-Präfix wiederverwendet wird.
    pub fn with_system_identity(mut self, text: impl Into<String>) -> Self {
        self.system_identity = normalize(text.into());
        self
    }

    /// Setzt das persistente Nutzerprofil (was das System über den Nutzer weiß).
    /// Im MVP leer.
    pub fn with_user_profile(mut self, text: impl Into<String>) -> Self {
        self.user_profile = normalize(text.into());
        self
    }

    /// Setzt die pro Session stabilen Werkzeugdefinitionen. Im MVP leer.
    pub fn with_tool_definitions(mut self, text: impl Into<String>) -> Self {
        self.tool_definitions = normalize(text.into());
        self
    }

    /// Setzt die abgerufenen Kontexte hinter der Cache-Grenze. Im MVP leer.
    pub fn with_variable_contexts<I, S>(mut self, contexts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.variable_contexts = contexts
            .into_iter()
            .map(|content| content.into())
            .filter(|content| !content.trim().is_empty())
            .collect();
        self
    }

    /// Sichert den Systemidentitätsblock nach außen ohne Klon.
    pub fn system_identity(&self) -> Option<&str> {
        self.system_identity.as_deref()
    }

    /// Sichert den Nutzerprofilblock nach außen ohne Klon.
    pub fn user_profile(&self) -> Option<&str> {
        self.user_profile.as_deref()
    }

    /// Sichert die Werkzeugdefinitionen nach außen ohne Klon.
    pub fn tool_definitions(&self) -> Option<&str> {
        self.tool_definitions.as_deref()
    }

    /// Sichert die variablen Kontexte nach außen ohne Klon.
    pub fn variable_contexts(&self) -> &[String] {
        &self.variable_contexts
    }

    /// Liefert die Blöcke vor der Cache-Grenze als synthetische System-Nachrichten.
    ///
    /// Die Felder `id`, `conversation_id`, `position` und
    /// `created_at_unix_ms` sind rein typenkompatibel und werden vom
    /// llama.cpp-Adapter ignoriert; er nutzt nur `role` und `content`.
    pub fn cache_stable_messages(&self) -> Vec<Message> {
        let mut messages = Vec::new();
        for (text, tag) in [
            (self.system_identity.as_deref(), "system-identity"),
            (self.user_profile.as_deref(), "user-profile"),
            (self.tool_definitions.as_deref(), "tool-definitions"),
        ] {
            if let Some(content) = text {
                let position = messages.len() as i64;
                messages.push(synthetic_message(tag, position, content));
            }
        }
        messages
    }

    /// Liefert die Blöcke hinter der Cache-Grenze als synthetische System-Nachrichten.
    pub fn variable_messages(&self) -> Vec<Message> {
        self.variable_contexts
            .iter()
            .enumerate()
            .map(|(index, content)| synthetic_message("variable-context", index as i64, content))
            .collect()
    }

    /// Fügt stabile und variable Blöcke in der Konzeptreihenfolge zusammen.
    pub fn rendered_messages(&self) -> Vec<Message> {
        let mut all = self.cache_stable_messages();
        all.extend(self.variable_messages());
        all
    }
}

fn normalize(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn synthetic_message(tag: &str, position: i64, content: &str) -> Message {
    Message {
        id: format!("prefix-{tag}-{position}"),
        conversation_id: String::new(),
        position,
        role: MessageRole::System,
        content: content.to_owned(),
        status: MessageStatus::Complete,
        created_at_unix_ms: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_prefix_renders_no_messages() {
        let prefix = PromptPrefix::empty();
        assert!(prefix.cache_stable_messages().is_empty());
        assert!(prefix.variable_messages().is_empty());
        assert!(prefix.rendered_messages().is_empty());
    }

    #[test]
    fn stable_blocks_precede_variable_contexts_in_declared_order() {
        let prefix = PromptPrefix::empty()
            .with_system_identity("Systemidentität")
            .with_user_profile("Nutzerprofil")
            .with_tool_definitions("Werkzeuge")
            .with_variable_contexts(["Kontext A", "Kontext B"]);
        let rendered = prefix.rendered_messages();
        let contents: Vec<&str> = rendered.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(
            contents,
            vec![
                "Systemidentität",
                "Nutzerprofil",
                "Werkzeuge",
                "Kontext A",
                "Kontext B"
            ]
        );
        for message in &rendered {
            assert_eq!(message.role, MessageRole::System);
        }
    }

    #[test]
    fn cache_stable_messages_ignore_variable_contexts() {
        let prefix = PromptPrefix::empty()
            .with_system_identity("PortableAI")
            .with_variable_contexts(["dynamisch"]);
        let stable = prefix.cache_stable_messages();
        assert_eq!(stable.len(), 1);
        assert_eq!(stable[0].content, "PortableAI");
    }

    #[test]
    fn stable_prefix_bytes_do_not_change_between_calls() {
        // Cache-Wiederverwendung setzt Byte-Gleichheit voraus. Zwei aufeinander
        // folgende Aufrufe müssen identische Nachrichten liefern.
        let prefix = PromptPrefix::empty()
            .with_system_identity("PortableAI")
            .with_user_profile("Nutzer arbeitet mit Rust");
        assert_eq!(
            prefix.cache_stable_messages(),
            prefix.cache_stable_messages()
        );
    }

    #[test]
    fn blank_slots_are_dropped_so_cache_is_not_broken_by_empty_lines() {
        let prefix = PromptPrefix::empty()
            .with_system_identity("   ")
            .with_user_profile("Nutzerprofil")
            .with_variable_contexts(["  ", "echt"]);
        let stable = prefix.cache_stable_messages();
        assert_eq!(stable.len(), 1);
        assert_eq!(stable[0].content, "Nutzerprofil");
        let variable = prefix.variable_messages();
        assert_eq!(variable.len(), 1);
        assert_eq!(variable[0].content, "echt");
    }
}
