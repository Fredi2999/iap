use pa_vault::cleanup::best_effort_secure_delete;
use pa_vault::{
    hot_copy::{HotVault, NoFault},
    key::derive_key,
    meta::{Argon2Parameters, VaultMeta},
};

#[test]
fn overwrites_and_removes_a_regular_hot_copy() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("hot.db");
    std::fs::write(&path, vec![0xA5; 64 * 1024]).expect("fixture");

    best_effort_secure_delete(&path).expect("delete");

    assert!(!path.exists());
}

#[test]
fn missing_cleanup_target_is_idempotent_but_directories_are_rejected() {
    let temp = tempfile::tempdir().expect("temp dir");
    best_effort_secure_delete(&temp.path().join("missing.db")).expect("missing is fine");
    let error = best_effort_secure_delete(temp.path()).expect_err("directory rejected");
    assert!(error.to_string().contains("reguläre Datei"));
}

#[test]
fn successful_shutdown_syncs_then_removes_the_hot_database() {
    let temp = tempfile::tempdir().expect("temp dir");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let key = derive_key(
        "pass",
        &VaultMeta::new(
            [6; 16],
            Argon2Parameters {
                memory_kib: 8192,
                iterations: 1,
                parallelism: 1,
            },
        ),
    )
    .expect("key");
    let mut vault = HotVault::start(&portable, &host, key).expect("start");
    vault
        .repository_mut()
        .create_conversation("c", "persisted", 1)
        .expect("write");
    vault.mark_dirty();
    let hot = vault.hot_path().to_owned();

    vault.shutdown(&mut NoFault).expect("shutdown");

    assert!(portable.exists());
    assert!(!hot.exists());
    assert!(!hot.with_extension("db-wal").exists());
    assert!(!hot.with_extension("db-shm").exists());
}
