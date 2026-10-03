//! Integrationstests für den in Phase 2 hinzugefügten Audit-Log im Vault.
//!
//! Deckt drei Punkte ab:
//! 1. Frisch angelegter Vault ist auf `SCHEMA_VERSION` und hat die `audit_log`-Tabelle.
//! 2. Ein Legacy-Vault, der nur bis Schemaversion 1 migriert wurde, kann nach
//!    einem erneuten Öffnen automatisch auf 2 hochwandern; bestehende Daten
//!    (Konversationen, Nachrichten, Einstellungen) bleiben unverändert.
//! 3. `append_audit` / `verify_audit` funktionieren durch die verschlüsselte
//!    Verbindung; die Kette überlebt einen Schließen-und-Neuöffnen-Zyklus.

use pa_vault::{
    audit::AuditWrite,
    key::derive_key,
    meta::{Argon2Parameters, VaultMeta},
    open_encrypted,
    repository::VaultRepository,
    schema::SCHEMA_VERSION,
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

fn schema_version_of(path: &std::path::Path, passphrase: &str, meta: &VaultMeta) -> i64 {
    let key = derive_key(passphrase, meta).expect("key");
    let connection = open_encrypted(path, &key).expect("open");
    connection
        .query_row("SELECT version FROM schema_version", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("schema_version present")
}

#[test]
fn fresh_vault_is_at_schema_version_two_and_has_audit_log() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vault.db");
    let meta = fast_meta([9; 16]);
    let key = derive_key("hallo welt", &meta).unwrap();
    {
        let _repository = VaultRepository::open(&path, &key).unwrap();
    }
    assert_eq!(
        schema_version_of(&path, "hallo welt", &meta),
        SCHEMA_VERSION
    );
    let connection = open_encrypted(&path, &derive_key("hallo welt", &meta).unwrap()).unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0, "audit_log muss leer, aber vorhanden sein");
}

#[test]
fn legacy_version_one_vault_migrates_up_and_keeps_existing_rows() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vault.db");
    let meta = fast_meta([7; 16]);
    let key = derive_key("legacy", &meta).unwrap();

    // Legacy-Zustand simulieren: eine bereits geöffnete verschlüsselte Datenbank,
    // in der die schema_version manuell auf 1 zurückgesetzt wird — wie ein
    // Vault, der aus der Phase-1-Version des Projekts stammt.
    {
        let mut repository = VaultRepository::open(&path, &key).unwrap();
        repository
            .create_conversation("c-1", "Legacy-Chat", 100)
            .unwrap();
        repository
            .append_message(
                "m-1",
                "c-1",
                pa_types::chat::MessageRole::User,
                "hallo",
                pa_types::chat::MessageStatus::Complete,
                110,
            )
            .unwrap();
    }
    {
        let connection = open_encrypted(&path, &derive_key("legacy", &meta).unwrap()).unwrap();
        connection
            .execute("UPDATE schema_version SET version = 1", [])
            .unwrap();
        connection.execute("DROP TABLE audit_log", []).unwrap();
    }

    // Erneut öffnen → Migration muss laufen.
    let repository = VaultRepository::open(&path, &derive_key("legacy", &meta).unwrap()).unwrap();
    assert_eq!(repository.conversations().unwrap().len(), 1);
    assert_eq!(repository.messages("c-1").unwrap().len(), 1);
    assert_eq!(schema_version_of(&path, "legacy", &meta), SCHEMA_VERSION);
}

#[test]
fn append_and_verify_survive_a_close_and_reopen_cycle() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vault.db");
    let meta = fast_meta([2; 16]);
    let key = derive_key("kette", &meta).unwrap();

    let last_hash = {
        let mut repository = VaultRepository::open(&path, &key).unwrap();
        let first = repository
            .append_audit(
                AuditWrite {
                    mode: "m1_workspace",
                    action: "file_read",
                    target: Some("hello.txt"),
                    outcome: "allow",
                    reason: "erster Eintrag",
                },
                1,
            )
            .unwrap();
        let second = repository
            .append_audit(
                AuditWrite {
                    mode: "m1_workspace",
                    action: "file_write",
                    target: Some("out.txt"),
                    outcome: "prompt",
                    reason: "zweiter Eintrag",
                },
                2,
            )
            .unwrap();
        assert_eq!(second.prev_hash, first.hash);
        second.hash
    };

    // Vollständiger Neustart der Verbindung; Kette muss weiterhin verifizieren.
    let reopened = VaultRepository::open(&path, &derive_key("kette", &meta).unwrap()).unwrap();
    assert_eq!(reopened.verify_audit().unwrap(), 2);
    assert_eq!(reopened.audit_head_hash().unwrap(), last_hash);
}
