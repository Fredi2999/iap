//! Konversationsverwaltung als schmale Fassade über `pa-vault`.
//!
//! `pa-core` schreibt keine Chatdaten selbst; jede Operation läuft durch den
//! bereits durch SQLCipher, Hot-Copy-Sync und Recovery abgesicherten Pfad in
//! `pa-vault`. Der Trait [`Conversations`] existiert, damit der Orchestrator
//! ohne einen realen Vault getestet werden kann.

use pa_types::chat::{Conversation, Message, MessageRole, MessageStatus};
use pa_vault::hot_copy::HotVault;

use crate::CoreError;

/// Minimaler Vertrag, den `pa-core` an einen Konversationsspeicher stellt.
///
/// Er spiegelt nur die im MVP verwendeten Repository-Methoden von
/// `pa-vault::VaultRepository`; jede Erweiterung erfordert einen bewussten
/// Trait-Ausbau, damit spätere UI-/Agent-Bereiche keine Abkürzungen bekommen.
pub trait Conversations {
    /// Legt eine neue Konversation an.
    fn create(&mut self, id: &str, title: &str, now_unix_ms: i64) -> Result<(), CoreError>;
    /// Listet Konversationen für die UI.
    fn list(&self) -> Result<Vec<Conversation>, CoreError>;
    /// Liefert alle Nachrichten einer Konversation in stabiler Position.
    fn messages(&self, conversation_id: &str) -> Result<Vec<Message>, CoreError>;
    /// Ändert den Anzeigetitel einer Konversation.
    fn rename(&mut self, id: &str, title: &str, now_unix_ms: i64) -> Result<(), CoreError>;
    /// Löscht eine Konversation inklusive aller Nachrichten (Fremdschlüssel-Cascade).
    fn delete(&mut self, id: &str) -> Result<(), CoreError>;
    /// Hängt eine Nachricht an; Position vergibt der Vault in derselben Transaktion.
    fn append(
        &mut self,
        id: &str,
        conversation_id: &str,
        role: MessageRole,
        content: &str,
        status: MessageStatus,
        now_unix_ms: i64,
    ) -> Result<(), CoreError>;
    /// Schreibt Streaming-Endzustand und finalen Text atomar in dieselbe Zeile.
    fn finish(&mut self, id: &str, content: &str, status: MessageStatus) -> Result<(), CoreError>;
}

/// Reale Implementierung von [`Conversations`] auf einem geöffneten `HotVault`.
///
/// Der Zugriff erfolgt ausschließlich über `HotVault::repository_mut()`, damit
/// die Dirty-Markierung des Hot-Copy-Zustands nicht umgangen wird.
pub struct HotVaultConversations<'vault> {
    vault: &'vault mut HotVault,
}

impl<'vault> HotVaultConversations<'vault> {
    /// Verbindet die Fassade mit einem bereits entsperrten Vault.
    pub fn new(vault: &'vault mut HotVault) -> Self {
        Self { vault }
    }
}

impl Conversations for HotVaultConversations<'_> {
    fn create(&mut self, id: &str, title: &str, now_unix_ms: i64) -> Result<(), CoreError> {
        self.vault
            .repository_mut()
            .create_conversation(id, title, now_unix_ms)
            .map_err(CoreError::from)
    }

    fn list(&self) -> Result<Vec<Conversation>, CoreError> {
        self.vault
            .repository()
            .conversations()
            .map_err(CoreError::from)
    }

    fn messages(&self, conversation_id: &str) -> Result<Vec<Message>, CoreError> {
        self.vault
            .repository()
            .messages(conversation_id)
            .map_err(CoreError::from)
    }

    fn rename(&mut self, id: &str, title: &str, now_unix_ms: i64) -> Result<(), CoreError> {
        self.vault
            .repository_mut()
            .rename_conversation(id, title, now_unix_ms)
            .map_err(CoreError::from)
    }

    fn delete(&mut self, id: &str) -> Result<(), CoreError> {
        self.vault
            .repository_mut()
            .delete_conversation(id)
            .map_err(CoreError::from)
    }

    fn append(
        &mut self,
        id: &str,
        conversation_id: &str,
        role: MessageRole,
        content: &str,
        status: MessageStatus,
        now_unix_ms: i64,
    ) -> Result<(), CoreError> {
        self.vault
            .repository_mut()
            .append_message(id, conversation_id, role, content, status, now_unix_ms)
            .map_err(CoreError::from)
    }

    fn finish(&mut self, id: &str, content: &str, status: MessageStatus) -> Result<(), CoreError> {
        self.vault
            .repository_mut()
            .finish_message(id, content, status)
            .map_err(CoreError::from)
    }
}
