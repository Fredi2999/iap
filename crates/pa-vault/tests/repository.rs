use pa_types::chat::{MessageRole, MessageStatus};
use pa_vault::{
    key::derive_key,
    meta::{Argon2Parameters, VaultMeta},
    repository::VaultRepository,
};

fn key() -> pa_vault::key::VaultKey {
    derive_key(
        "test-passphrase",
        &VaultMeta::new(
            [9; 16],
            Argon2Parameters {
                memory_kib: 8 * 1024,
                iterations: 1,
                parallelism: 1,
            },
        ),
    )
    .expect("key")
}

#[test]
fn persists_conversations_ordered_messages_partial_state_and_settings() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("vault.db");
    let mut repository = VaultRepository::open(&path, &key()).expect("repository");

    repository
        .create_conversation("c1", "Privater Titel", 100)
        .expect("conversation");
    repository
        .append_message(
            "m2",
            "c1",
            MessageRole::Assistant,
            "Teil",
            MessageStatus::Streaming,
            102,
        )
        .unwrap_or_else(|error| panic!("second: {error}"));
    repository
        .append_message(
            "m1",
            "c1",
            MessageRole::User,
            "Hallo",
            MessageStatus::Complete,
            101,
        )
        .unwrap_or_else(|error| panic!("first by timestamp, but appended second: {error}"));

    let messages = repository.messages("c1").expect("messages");
    assert_eq!(
        messages.iter().map(|m| m.position).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(messages[0].id, "m2");
    repository
        .finish_message("m2", "Teilantwort", MessageStatus::Aborted)
        .expect("abort state");
    let messages = repository.messages("c1").expect("updated messages");
    assert_eq!(messages[0].content, "Teilantwort");
    assert_eq!(messages[0].status, MessageStatus::Aborted);

    repository
        .rename_conversation("c1", "Neu", 110)
        .expect("rename");
    assert_eq!(repository.conversations().expect("list")[0].title, "Neu");
    repository.set_setting("theme", "dark").expect("setting");
    assert_eq!(
        repository.setting("theme").expect("read"),
        Some("dark".to_owned())
    );
    drop(repository);

    let bytes = std::fs::read(&path).expect("database");
    assert!(!bytes
        .windows("Privater Titel".len())
        .any(|w| w == b"Privater Titel"));

    let mut reopened = VaultRepository::open(&path, &key()).expect("reopen");
    reopened.delete_conversation("c1").expect("delete");
    assert!(reopened.conversations().expect("empty").is_empty());
    assert!(reopened.messages("c1").expect("cascade").is_empty());
}

#[test]
fn wrong_passphrase_is_rejected_by_a_real_schema_read() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("vault.db");
    drop(VaultRepository::open(&path, &key()).expect("create"));
    let wrong = derive_key(
        "wrong",
        &VaultMeta::new(
            [9; 16],
            Argon2Parameters {
                memory_kib: 8192,
                iterations: 1,
                parallelism: 1,
            },
        ),
    )
    .expect("wrong key");

    match VaultRepository::open(&path, &wrong) {
        Err(error) => assert!(matches!(error, pa_vault::VaultError::Authentication)),
        Ok(_) => panic!("wrong passphrase unexpectedly opened the schema"),
    }
}
