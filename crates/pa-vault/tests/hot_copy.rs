use std::time::Duration;

use pa_types::chat::MessageRole;
use pa_vault::{
    hot_copy::{HotCopyState, HotVault, NoFault, RecoveryMode, SyncFault, SyncFaultInjector},
    key::derive_key,
    meta::{Argon2Parameters, VaultMeta},
    open_encrypted,
    repository::VaultRepository,
    VaultError,
};

fn key() -> pa_vault::key::VaultKey {
    derive_key(
        "portable",
        &VaultMeta::new(
            [7; 16],
            Argon2Parameters {
                memory_kib: 8192,
                iterations: 1,
                parallelism: 1,
            },
        ),
    )
    .expect("key")
}

struct FailAt(SyncFault);

impl SyncFaultInjector for FailAt {
    fn check(&mut self, point: SyncFault) -> Result<(), VaultError> {
        if point == self.0 {
            Err(VaultError::InjectedFault(point.name()))
        } else {
            Ok(())
        }
    }
}

#[test]
fn works_on_a_hot_copy_and_atomically_syncs_an_encrypted_snapshot() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut vault = HotVault::start(&portable, &host, key()).expect("start");
    vault
        .repository_mut()
        .create_conversation("c1", "On hot copy", 1)
        .expect("write");
    vault.mark_dirty();

    assert_eq!(vault.state(), HotCopyState::Dirty);
    let deadline = vault.next_sync_deadline();
    assert!(!vault.sync_due(deadline - Duration::from_millis(1)));
    assert!(vault.sync_due(deadline));
    vault.sync(&mut NoFault).expect("sync");
    assert_eq!(vault.state(), HotCopyState::Clean);
    assert!(portable.exists());
    assert!(!host.join("recovery.db").exists());

    let reopened = VaultRepository::open(&portable, &key()).expect("portable snapshot");
    assert_eq!(
        reopened.conversations().expect("conversations")[0].title,
        "On hot copy"
    );
}

#[test]
fn interrupted_sync_keeps_a_valid_host_recovery_and_never_publishes_partial_data() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut vault = HotVault::start(&portable, &host, key()).expect("start");
    vault
        .repository_mut()
        .create_conversation("c1", "Recover me", 1)
        .expect("write");
    vault.mark_dirty();

    let error = vault
        .sync(&mut FailAt(SyncFault::BeforeRename))
        .expect_err("fault");
    assert!(matches!(error, VaultError::InjectedFault(_)));
    assert_eq!(vault.state(), HotCopyState::Recoverable);
    assert!(host.join("recovery.db").exists());
    assert!(!portable.exists());

    drop(vault);
    let recovered = HotVault::start(&portable, &host, key()).expect("recover hot database");
    assert_eq!(
        recovered.repository().conversations().expect("data")[0].title,
        "Recover me"
    );
}

#[test]
fn missing_usb_becomes_pending_media_without_losing_the_host_snapshot() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("missing-usb/vault.db");
    let host = temp.path().join("host");
    let mut vault = HotVault::start(&portable, &host, key()).expect("start");
    vault
        .repository_mut()
        .create_conversation("c1", "Still here", 1)
        .expect("write");
    vault.mark_dirty();
    vault.set_media_available(false);

    vault.sync(&mut NoFault).expect_err("media missing");
    assert_eq!(vault.state(), HotCopyState::PendingMedia);
    assert!(host.join("recovery.db").exists());
}

#[test]
fn preserves_aborted_partial_assistant_content_in_snapshot() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut vault = HotVault::start(&portable, &host, key()).expect("start");
    vault
        .repository_mut()
        .create_conversation("c", "Chat", 1)
        .expect("c");
    vault
        .repository_mut()
        .append_message(
            "m",
            "c",
            MessageRole::Assistant,
            "partial",
            pa_types::chat::MessageStatus::Aborted,
            2,
        )
        .expect("message");
    vault.mark_dirty();
    vault.sync(&mut NoFault).expect("sync");

    let reopened = VaultRepository::open(&portable, &key()).expect("reopen");
    assert_eq!(
        reopened.messages("c").expect("messages")[0].content,
        "partial"
    );
}

#[test]
fn mutable_repository_access_marks_the_hot_copy_dirty_automatically() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut vault = HotVault::start(&portable, &host, key()).expect("start");

    vault
        .repository_mut()
        .create_conversation("automatic", "Must survive", 1)
        .expect("write");
    assert_eq!(vault.state(), HotCopyState::Dirty);
    vault.shutdown(&mut NoFault).expect("shutdown sync");

    let reopened = VaultRepository::open(&portable, &key()).expect("reopen");
    assert_eq!(
        reopened.conversations().expect("conversations")[0].title,
        "Must survive"
    );
}

#[test]
fn explicitly_chosen_newer_portable_snapshot_replaces_a_divergent_hot_copy() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut first = HotVault::start(&portable, &host, key()).expect("start");
    first
        .repository_mut()
        .create_conversation("old", "Old", 1)
        .expect("old write");
    first.sync(&mut NoFault).expect("initial sync");
    drop(first);

    std::thread::sleep(Duration::from_millis(20));
    let mut portable_repository = VaultRepository::open(&portable, &key()).expect("portable open");
    portable_repository
        .create_conversation("new", "New", 2)
        .expect("new write");
    drop(portable_repository);

    let recovered =
        HotVault::start_with_recovery(&portable, &host, key(), RecoveryMode::UsePortable)
            .expect("choose portable");
    let conversations = recovered
        .repository()
        .conversations()
        .expect("conversations");
    assert!(conversations.iter().any(|entry| entry.id == "new"));
}

#[test]
fn newer_host_copy_requires_confirmation_before_it_can_replace_portable_data() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut first = HotVault::start(&portable, &host, key()).expect("start");
    first
        .repository_mut()
        .create_conversation("portable", "On USB", 1)
        .expect("portable write");
    first.sync(&mut NoFault).expect("initial sync");
    first
        .repository_mut()
        .create_conversation("host", "After crash", 2)
        .expect("host write");
    drop(first);

    let error = HotVault::start(&portable, &host, key())
        .err()
        .expect("confirmation required");
    assert!(matches!(
        error,
        VaultError::RecoveryChoiceRequired {
            portable_generation: 1,
            host_generation: 2
        }
    ));

    let recovered =
        HotVault::start_with_recovery(&portable, &host, key(), RecoveryMode::UseNewestHost)
            .expect("confirmed host recovery");
    assert!(recovered
        .repository()
        .conversations()
        .expect("conversations")
        .iter()
        .any(|entry| entry.id == "host"));
}

#[test]
fn choosing_portable_keeps_the_newer_host_recovery_for_a_later_decision() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut first = HotVault::start(&portable, &host, key()).expect("start");
    first
        .repository_mut()
        .create_conversation("portable", "On USB", 1)
        .expect("portable write");
    first.sync(&mut NoFault).expect("initial sync");
    first
        .repository_mut()
        .create_conversation("host", "Unsynced", 2)
        .expect("host write");
    drop(first);

    let portable_choice =
        HotVault::start_with_recovery(&portable, &host, key(), RecoveryMode::UsePortable)
            .expect("portable choice");
    assert!(!portable_choice
        .repository()
        .conversations()
        .expect("conversations")
        .iter()
        .any(|entry| entry.id == "host"));
    let mut portable_choice = portable_choice;
    portable_choice
        .repository_mut()
        .create_conversation("continued", "Portable branch continued", 3)
        .expect("continued write");
    portable_choice
        .shutdown(&mut NoFault)
        .expect("portable branch sync");

    assert!(matches!(
        HotVault::start(&portable, &host, key()).err(),
        Some(VaultError::RecoveryChoiceRequired {
            portable_generation: 2,
            host_generation: 2
        })
    ));
}

#[test]
fn equal_generation_with_different_revisions_requires_confirmation() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut first = HotVault::start(&portable, &host, key()).expect("start");
    first
        .repository_mut()
        .create_conversation("base", "Base", 1)
        .expect("base write");
    first.sync(&mut NoFault).expect("initial sync");

    let mut portable_repository = VaultRepository::open(&portable, &key()).expect("portable open");
    portable_repository
        .create_conversation("portable", "Portable branch", 2)
        .expect("portable branch");
    drop(portable_repository);
    first
        .repository_mut()
        .create_conversation("host", "Host branch", 2)
        .expect("host branch");
    drop(first);

    // Simuliert einen mit der frühen Schritt-3-Version geschriebenen Vault,
    // der bereits Generationen, aber noch keine Revisions-ID enthielt.
    for path in [&portable, &host.join("hot.db")] {
        let connection = open_encrypted(path, &key()).expect("legacy open");
        connection
            .execute("DELETE FROM settings WHERE key = '__pa_vault_revision'", [])
            .expect("remove revision");
    }

    assert!(matches!(
        HotVault::start(&portable, &host, key()).err(),
        Some(VaultError::RecoveryChoiceRequired {
            portable_generation: 2,
            host_generation: 2
        })
    ));
}

#[test]
fn choosing_one_branch_preserves_every_other_divergent_identity() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let hot = host.join("hot.db");
    let recovery = host.join("recovery.db");
    let mut first = HotVault::start(&portable, &host, key()).expect("start");
    first
        .repository_mut()
        .create_conversation("base", "Base", 1)
        .expect("base write");
    first.sync(&mut NoFault).expect("initial sync");
    std::fs::copy(&hot, &recovery).expect("recovery branch seed");

    let mut portable_repository = VaultRepository::open(&portable, &key()).expect("portable open");
    portable_repository
        .create_conversation("portable", "Portable branch", 2)
        .expect("portable branch");
    drop(portable_repository);
    first
        .repository_mut()
        .create_conversation("hot", "Hot branch", 2)
        .expect("hot branch");
    drop(first);
    let mut recovery_repository = VaultRepository::open(&recovery, &key()).expect("recovery open");
    recovery_repository
        .create_conversation("recovery", "Recovery branch", 2)
        .expect("recovery branch");
    drop(recovery_repository);

    let chosen = HotVault::start_with_recovery(&portable, &host, key(), RecoveryMode::UsePortable)
        .expect("choose portable");
    assert!(chosen
        .repository()
        .conversations()
        .expect("active conversations")
        .iter()
        .any(|conversation| conversation.id == "portable"));
    drop(chosen);

    let conflict_paths = std::fs::read_dir(host.join("conflicts"))
        .expect("conflict directory")
        .map(|entry| entry.expect("entry").path())
        .collect::<Vec<_>>();
    assert_eq!(conflict_paths.len(), 2);
    let mut recovered_ids = Vec::new();
    for path in conflict_paths {
        recovered_ids.extend(
            VaultRepository::open(&path, &key())
                .expect("conflict open")
                .conversations()
                .expect("conflict conversations")
                .into_iter()
                .map(|conversation| conversation.id),
        );
    }
    assert!(recovered_ids.iter().any(|id| id == "hot"));
    assert!(recovered_ids.iter().any(|id| id == "recovery"));
}
