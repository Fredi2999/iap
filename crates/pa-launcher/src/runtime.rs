//! Wiederverwendbare Runtime-Bausteine für den CLI-Launcher und die Tauri-App:
//! ein Vault-Hintergrundworker und eine `Conversations`-Fassade, die den
//! Vault-Lock nur pro Repository-Operation nimmt.

use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use pa_core::{conversation::Conversations, CoreError};
use pa_types::chat::{Conversation, Message, MessageRole, MessageStatus};
use pa_vault::{
    hot_copy::{HotCopyState, HotVault, NoFault},
    VaultError,
};

/// Besitzt den einzigen entsperrten `HotVault` und synchronisiert ihn im Hintergrund.
pub struct VaultRuntime {
    shared: Option<Arc<Mutex<HotVault>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl VaultRuntime {
    /// Startet den Fünf-Minuten-Rücksync-Worker; `on_error` wird für spätere
    /// Rücksync-Fehler aufgerufen, damit CLI/UI eine sinnvolle Meldung anzeigen.
    pub fn start<F>(vault: HotVault, mut on_error: F) -> Self
    where
        F: FnMut(&VaultError) + Send + 'static,
    {
        let shared = Arc::new(Mutex::new(vault));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_vault = Arc::clone(&shared);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            let mut retry_at = Instant::now();
            while !worker_stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(250));
                let now = Instant::now();
                let Ok(mut vault) = worker_vault.lock() else {
                    return;
                };
                let retry_pending = matches!(
                    vault.state(),
                    HotCopyState::PendingMedia | HotCopyState::Recoverable
                ) && now >= retry_at;
                if vault.sync_due(now) || retry_pending {
                    let mut no_fault = NoFault;
                    if let Err(error) = vault.sync(&mut no_fault) {
                        on_error(&error);
                        retry_at = now + Duration::from_secs(5);
                    }
                }
            }
        });
        Self {
            shared: Some(shared),
            stop,
            worker: Some(worker),
        }
    }

    /// Startet ohne Rücksync-Fehlermeldung; für Tests und einfache Verwender.
    pub fn start_quiet(vault: HotVault) -> Self {
        Self::start(vault, |_| {})
    }

    /// Liefert eine geteilte Referenz auf den `Arc<Mutex<HotVault>>` für weitere
    /// Fassaden (z. B. `SharedConversations`).
    pub fn shared(&self) -> io::Result<Arc<Mutex<HotVault>>> {
        self.shared
            .as_ref()
            .cloned()
            .ok_or_else(|| io::Error::other("Vault wurde bereits beendet"))
    }

    /// Nimmt kurz den Vault-Lock; erlaubt Ad-hoc-Operationen aus Aufruferseite.
    pub fn lock(&self) -> io::Result<MutexGuard<'_, HotVault>> {
        self.shared
            .as_ref()
            .ok_or_else(|| io::Error::other("Vault wurde bereits beendet"))?
            .lock()
            .map_err(|_| io::Error::other("Vault-Worker ist nach einem Panic vergiftet"))
    }

    /// Beendet den Worker, synchronisiert deterministisch und löscht die Hot-Copy.
    pub fn shutdown(mut self) -> Result<(), io::Error> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("Vault-Worker konnte nicht beendet werden"))?;
        }
        let shared = self
            .shared
            .take()
            .ok_or_else(|| io::Error::other("Vault wurde bereits beendet"))?;
        let vault = Arc::try_unwrap(shared)
            .map_err(|_| io::Error::other("Vault besitzt noch aktive Referenzen"))?
            .into_inner()
            .map_err(|_| io::Error::other("Vault-Mutex ist vergiftet"))?;
        let mut no_fault = NoFault;
        vault
            .shutdown(&mut no_fault)
            .map_err(|error| io::Error::other(error.to_string()))
    }
}

impl Drop for VaultRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Bindet den `pa-core::Conversations`-Trait an einen `Arc<Mutex<HotVault>>`.
///
/// Jede Methode nimmt den Vault-Lock nur so lange, wie eine einzelne
/// Repository-Operation dauert. Das ist dieselbe Sperrgranularität, die
/// Schritt 3 bereits hatte, damit der Hintergrundsynchronisationsworker
/// zwischen zwei Operationen weiter zum Zug kommt.
pub struct SharedConversations {
    vault: Arc<Mutex<HotVault>>,
}

impl SharedConversations {
    /// Bindet die Fassade an einen bereits geöffneten Vault.
    pub fn new(vault: Arc<Mutex<HotVault>>) -> Self {
        Self { vault }
    }

    fn locked(&self) -> Result<MutexGuard<'_, HotVault>, CoreError> {
        self.vault.lock().map_err(|_| CoreError::Poisoned)
    }
}

impl Conversations for SharedConversations {
    fn create(&mut self, id: &str, title: &str, now_unix_ms: i64) -> Result<(), CoreError> {
        let mut vault = self.locked()?;
        vault
            .repository_mut()
            .create_conversation(id, title, now_unix_ms)
            .map_err(CoreError::from)
    }

    fn list(&self) -> Result<Vec<Conversation>, CoreError> {
        let vault = self.locked()?;
        vault.repository().conversations().map_err(CoreError::from)
    }

    fn messages(&self, conversation_id: &str) -> Result<Vec<Message>, CoreError> {
        let vault = self.locked()?;
        vault
            .repository()
            .messages(conversation_id)
            .map_err(CoreError::from)
    }

    fn rename(&mut self, id: &str, title: &str, now_unix_ms: i64) -> Result<(), CoreError> {
        let mut vault = self.locked()?;
        vault
            .repository_mut()
            .rename_conversation(id, title, now_unix_ms)
            .map_err(CoreError::from)
    }

    fn delete(&mut self, id: &str) -> Result<(), CoreError> {
        let mut vault = self.locked()?;
        vault
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
        let mut vault = self.locked()?;
        vault
            .repository_mut()
            .append_message(id, conversation_id, role, content, status, now_unix_ms)
            .map_err(CoreError::from)
    }

    fn finish(&mut self, id: &str, content: &str, status: MessageStatus) -> Result<(), CoreError> {
        let mut vault = self.locked()?;
        vault
            .repository_mut()
            .finish_message(id, content, status)
            .map_err(CoreError::from)
    }
}
