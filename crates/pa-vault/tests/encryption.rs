use pa_vault::{
    key::derive_key,
    meta::{Argon2Parameters, VaultMeta},
    open_encrypted,
};

fn fast_meta(salt: [u8; 16]) -> VaultMeta {
    VaultMeta::new(
        salt,
        Argon2Parameters {
            memory_kib: 8 * 1024,
            iterations: 1,
            parallelism: 1,
        },
    )
}

#[test]
fn links_real_sqlcipher_and_encrypts_the_database_header() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("vault.db");
    let key = derive_key("correct horse", &fast_meta([1; 16])).expect("key");
    let connection = open_encrypted(&path, &key).expect("encrypted connection");

    let cipher_version: String = connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .expect("SQLCipher must answer");
    assert!(!cipher_version.trim().is_empty());
    connection
        .execute_batch(
            "CREATE TABLE secret(value TEXT); INSERT INTO secret VALUES ('PA_SECRET_MARKER');",
        )
        .expect("schema");
    drop(connection);

    let bytes = std::fs::read(path).expect("database bytes");
    assert!(!bytes.starts_with(b"SQLite format 3"));
    assert!(!bytes
        .windows(b"PA_SECRET_MARKER".len())
        .any(|window| window == b"PA_SECRET_MARKER"));
}

#[test]
fn argon2id_is_reproducible_only_with_the_same_salt() {
    let first = derive_key("passphrase", &fast_meta([2; 16])).expect("first");
    let again = derive_key("passphrase", &fast_meta([2; 16])).expect("again");
    let other = derive_key("passphrase", &fast_meta([3; 16])).expect("other");

    assert_eq!(first.expose_for_sqlcipher(), again.expose_for_sqlcipher());
    assert_ne!(first.expose_for_sqlcipher(), other.expose_for_sqlcipher());
    assert_eq!(first.expose_for_sqlcipher().len(), 32);
}

#[test]
fn metadata_round_trips_versioned_parameters() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("vault.meta");
    let meta = fast_meta([4; 16]);
    meta.save_atomic(&path).expect("save");

    let loaded = VaultMeta::load(&path).expect("load");
    assert_eq!(loaded, meta);
    assert_eq!(loaded.kdf, "argon2id-v19");
}
