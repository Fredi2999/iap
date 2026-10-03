//! Bridge zwischen `pa_policy::AuditSink` und dem persistenten
//! `audit_log` im Vault (Schema V2).
//!
//! Lebt in `pa-launcher`, weil hier `pa-policy` und `pa-vault` sich schon
//! kennen. Die Bridge übersetzt zwischen den pa-policy-Enums und den
//! stabilen Byte-Strings, die pa-vault direkt in die Hash-Kette schreibt —
//! die Kette bleibt damit identisch zu einer, die pa-policy selbst
//! aufgebaut hätte.

use std::sync::{Arc, Mutex};

use pa_policy::{
    action_to_str, mode_to_str, outcome_to_str, AuditLog, AuditRecord, AuditSink, PolicyError,
};
use pa_vault::{audit::AuditWrite, hot_copy::HotVault};

/// Persistiert Audit-Einträge im Vault; nimmt die Vault-Sperre nur pro
/// Aufruf, damit der 5-Minuten-Rücksync-Worker zwischendrin arbeiten kann.
pub struct VaultAuditSink {
    vault: Arc<Mutex<HotVault>>,
}

impl VaultAuditSink {
    /// Bindet den Sink an einen bereits geöffneten `HotVault`.
    pub fn new(vault: Arc<Mutex<HotVault>>) -> Self {
        Self { vault }
    }
}

impl AuditSink for VaultAuditSink {
    fn append(&mut self, entry: AuditLog, now_unix_ms: i64) -> Result<AuditRecord, PolicyError> {
        let mut vault = self
            .vault
            .lock()
            .map_err(|_| PolicyError::Storage("Vault-Mutex vergiftet".to_owned()))?;
        let row = vault
            .repository_mut()
            .append_audit(
                AuditWrite {
                    mode: mode_to_str(entry.mode),
                    action: action_to_str(entry.action),
                    target: entry.target.as_deref(),
                    outcome: outcome_to_str(entry.outcome),
                    reason: &entry.reason,
                },
                now_unix_ms,
            )
            .map_err(|error| PolicyError::Storage(error.to_string()))?;
        Ok(AuditRecord {
            id: row.id,
            created_unix_ms: row.created_unix_ms,
            mode: entry.mode,
            action: entry.action,
            target: row.target,
            outcome: entry.outcome,
            reason: row.reason,
            prev_hash: row.prev_hash,
            hash: row.hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_policy::{AuditOutcome, AuditStore, CapabilityAction, Mode};
    use pa_vault::{
        hot_copy::HotVault,
        key::derive_key,
        meta::{Argon2Parameters, VaultMeta},
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

    fn open_vault(dir: &std::path::Path, name: &str) -> HotVault {
        let portable = dir.join(format!("{name}-vault.db"));
        let hot = dir.join(format!("{name}-hot"));
        let meta = fast_meta([3; 16]);
        let key = derive_key("bridge", &meta).unwrap();
        HotVault::start(&portable, &hot, key).unwrap()
    }

    #[test]
    fn vault_bridge_produces_the_same_hashes_as_in_memory_store_for_the_same_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let vault = Arc::new(Mutex::new(open_vault(temp.path(), "sink")));
        let mut bridge = VaultAuditSink::new(Arc::clone(&vault));
        let mut memory = AuditStore::open_in_memory().unwrap();
        for (index, (action, outcome, target, reason)) in [
            (
                CapabilityAction::FileRead,
                AuditOutcome::Allow,
                Some("hello.txt".to_owned()),
                "erster Lesezugriff".to_owned(),
            ),
            (
                CapabilityAction::FileWrite,
                AuditOutcome::Prompt,
                Some("nested/dir.txt".to_owned()),
                "bewusste Rückfrage nötig".to_owned(),
            ),
            (
                CapabilityAction::Network,
                AuditOutcome::Deny,
                None,
                "Netzwerk immer aus".to_owned(),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let now = 1_000 + index as i64;
            let entry = AuditLog {
                mode: Mode::M1Workspace,
                action,
                target,
                outcome,
                reason,
            };
            let a = bridge.append(entry.clone(), now).unwrap();
            let b = memory.append(entry, now).unwrap();
            assert_eq!(a.hash, b.hash, "Hash weicht bei Eintrag {index} ab");
            assert_eq!(a.prev_hash, b.prev_hash, "prev_hash weicht ab");
        }
        let stored = vault
            .lock()
            .unwrap()
            .repository()
            .audit_entries(None)
            .unwrap();
        assert_eq!(stored.len(), 3);
    }
}
