use pa_core::conversation::{Conversations, HotVaultConversations};
use pa_types::chat::{MessageRole, MessageStatus};
use pa_vault::{
    hot_copy::HotVault,
    key::{derive_key, VaultKey},
    meta::{Argon2Parameters, VaultMeta},
};

fn test_key() -> VaultKey {
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
fn append_and_finish_flow_updates_status_and_preserves_partial_text() {
    let temp = tempfile::tempdir().expect("temp");
    let portable = temp.path().join("vault.db");
    let host_dir = temp.path().join("host");
    let mut vault = HotVault::start(&portable, &host_dir, test_key()).expect("vault");
    let mut conversations = HotVaultConversations::new(&mut vault);

    conversations.create("c1", "Neu", 100).expect("create");
    conversations
        .append(
            "u1",
            "c1",
            MessageRole::User,
            "Hi",
            MessageStatus::Complete,
            101,
        )
        .expect("append user");
    conversations
        .append(
            "a1",
            "c1",
            MessageRole::Assistant,
            "",
            MessageStatus::Streaming,
            102,
        )
        .expect("append assistant placeholder");
    conversations
        .finish("a1", "Teilantwort", MessageStatus::Aborted)
        .expect("finish");

    let messages = conversations.messages("c1").expect("messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, MessageRole::User);
    assert_eq!(messages[1].role, MessageRole::Assistant);
    assert_eq!(messages[1].content, "Teilantwort");
    assert_eq!(messages[1].status, MessageStatus::Aborted);
}

#[test]
fn rename_updates_title_and_delete_removes_conversation_and_messages() {
    let temp = tempfile::tempdir().expect("temp");
    let portable = temp.path().join("vault.db");
    let host_dir = temp.path().join("host");
    let mut vault = HotVault::start(&portable, &host_dir, test_key()).expect("vault");
    let mut conversations = HotVaultConversations::new(&mut vault);

    conversations
        .create("c1", "Erste Session", 100)
        .expect("create");
    conversations
        .append(
            "u1",
            "c1",
            MessageRole::User,
            "Hi",
            MessageStatus::Complete,
            101,
        )
        .expect("append");
    conversations
        .rename("c1", "Umbenannt", 110)
        .expect("rename");
    let list = conversations.list().expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "Umbenannt");

    conversations.delete("c1").expect("delete");
    let list = conversations.list().expect("list");
    assert!(list.is_empty());
    let messages = conversations.messages("c1").expect("messages");
    assert!(messages.is_empty());
}
