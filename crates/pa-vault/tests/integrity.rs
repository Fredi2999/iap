use pa_vault::{
    key::derive_key,
    meta::{Argon2Parameters, VaultMeta},
    repository::VaultRepository,
};

#[test]
fn healthy_database_passes_cipher_and_sqlite_integrity_checks() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("vault.db");
    let key = derive_key(
        "pass",
        &VaultMeta::new(
            [8; 16],
            Argon2Parameters {
                memory_kib: 8192,
                iterations: 1,
                parallelism: 1,
            },
        ),
    )
    .expect("key");
    let repository = VaultRepository::open(&path, &key).expect("repository");

    repository.check_integrity().expect("both checks");
}
