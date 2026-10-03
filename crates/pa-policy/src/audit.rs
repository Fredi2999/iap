//! Append-only Audit-Log mit Hash-Verkettung nach Konzept 10.2.
//!
//! Jede Policy-Entscheidung (auch `Deny`) wird persistiert. `prev_hash`
//! verkettet die Einträge: nachträgliches Umschreiben eines Eintrags
//! zerstört die Kette und wird beim Verifizieren erkannt.

use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{capability::CapabilityAction, mode::Mode, PolicyError};

const CREATE_SQL: &str = r"
CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_unix_ms INTEGER NOT NULL,
    mode TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT,
    outcome TEXT NOT NULL,
    reason TEXT NOT NULL,
    prev_hash BLOB NOT NULL,
    hash BLOB NOT NULL
);
";

/// Ergebnis der Prüfung, wie es im Log landet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Allow,
    Prompt,
    Deny,
}

/// Ein persistierter Log-Eintrag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    pub id: i64,
    pub created_unix_ms: i64,
    pub mode: Mode,
    pub action: CapabilityAction,
    pub target: Option<String>,
    pub outcome: AuditOutcome,
    pub reason: String,
    pub prev_hash: Vec<u8>,
    pub hash: Vec<u8>,
}

/// Kapselt die Persistierung eines einzelnen Eintrags.
#[derive(Debug, Clone)]
pub struct AuditLog {
    pub mode: Mode,
    pub action: CapabilityAction,
    pub target: Option<String>,
    pub outcome: AuditOutcome,
    pub reason: String,
}

/// Hält die Verbindung zum SQLite-Log und die letzte Kettenspitze.
pub struct AuditStore {
    connection: Connection,
    last_hash: Vec<u8>,
}

impl AuditStore {
    /// Öffnet oder legt die Datenbank am Zielpfad an.
    pub fn open(path: &Path) -> Result<Self, PolicyError> {
        let connection = Connection::open(path)?;
        connection.execute_batch(CREATE_SQL)?;
        let last_hash: Option<Vec<u8>> = connection
            .query_row(
                "SELECT hash FROM audit_log ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(Self {
            connection,
            last_hash: last_hash.unwrap_or_else(|| vec![0_u8; 32]),
        })
    }

    /// Für Tests: In-Memory-Store.
    pub fn open_in_memory() -> Result<Self, PolicyError> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(CREATE_SQL)?;
        Ok(Self {
            connection,
            last_hash: vec![0_u8; 32],
        })
    }

    /// Hängt einen Eintrag an; berechnet Hash aus vorherigem Hash + Feldern.
    pub fn append(
        &mut self,
        entry: AuditLog,
        now_unix_ms: i64,
    ) -> Result<AuditRecord, PolicyError> {
        let hash = compute_hash(&self.last_hash, &entry, now_unix_ms);
        self.connection.execute(
            "INSERT INTO audit_log (created_unix_ms, mode, action, target, outcome, reason, prev_hash, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                now_unix_ms,
                mode_to_str(entry.mode),
                action_to_str(entry.action),
                entry.target,
                outcome_to_str(entry.outcome),
                entry.reason,
                self.last_hash.clone(),
                hash.clone(),
            ],
        )?;
        let id = self.connection.last_insert_rowid();
        let record = AuditRecord {
            id,
            created_unix_ms: now_unix_ms,
            mode: entry.mode,
            action: entry.action,
            target: entry.target,
            outcome: entry.outcome,
            reason: entry.reason,
            prev_hash: self.last_hash.clone(),
            hash: hash.clone(),
        };
        self.last_hash = hash;
        Ok(record)
    }

    /// Liest alle Einträge in Einfügungsreihenfolge.
    pub fn all(&self) -> Result<Vec<AuditRecord>, PolicyError> {
        let mut statement = self.connection.prepare(
            "SELECT id, created_unix_ms, mode, action, target, outcome, reason, prev_hash, hash
             FROM audit_log ORDER BY id ASC",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(AuditRecord {
                    id: row.get(0)?,
                    created_unix_ms: row.get(1)?,
                    mode: str_to_mode(&row.get::<_, String>(2)?),
                    action: str_to_action(&row.get::<_, String>(3)?),
                    target: row.get(4)?,
                    outcome: str_to_outcome(&row.get::<_, String>(5)?),
                    reason: row.get(6)?,
                    prev_hash: row.get(7)?,
                    hash: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Prüft die Hashkette und liefert die Anzahl geprüfter Einträge zurück.
    pub fn verify(&self) -> Result<usize, PolicyError> {
        let records = self.all()?;
        let mut previous = vec![0_u8; 32];
        for (index, record) in records.iter().enumerate() {
            if record.prev_hash != previous {
                return Err(PolicyError::Storage(format!(
                    "Kette bei Eintrag {} unterbrochen: prev_hash weicht ab",
                    index
                )));
            }
            let expected = compute_hash(
                &previous,
                &AuditLog {
                    mode: record.mode,
                    action: record.action,
                    target: record.target.clone(),
                    outcome: record.outcome,
                    reason: record.reason.clone(),
                },
                record.created_unix_ms,
            );
            if expected != record.hash {
                return Err(PolicyError::Storage(format!(
                    "Hash von Eintrag {} passt nicht zu seinem Inhalt",
                    index
                )));
            }
            previous = record.hash.clone();
        }
        Ok(records.len())
    }

    /// Direkter Zugriff auf die Verbindung; nur für Tests, die eine Manipulation
    /// vortäuschen wollen.
    #[cfg(test)]
    pub(crate) fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

fn compute_hash(prev: &[u8], entry: &AuditLog, now: i64) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(prev);
    hasher.update(now.to_le_bytes());
    hasher.update(mode_to_str(entry.mode).as_bytes());
    hasher.update([0]);
    hasher.update(action_to_str(entry.action).as_bytes());
    hasher.update([0]);
    hasher.update(entry.target.as_deref().unwrap_or("").as_bytes());
    hasher.update([0]);
    hasher.update(outcome_to_str(entry.outcome).as_bytes());
    hasher.update([0]);
    hasher.update(entry.reason.as_bytes());
    hasher.finalize().to_vec()
}

/// Stabile Byte-Bezeichnung für den `Mode`, wie sie in der Hash-Kette landet.
///
/// Wird auch von `pa-vault::audit` und der `VaultAuditSink`-Bridge genutzt,
/// damit dieselben Bytes gehasht werden – ein Wechsel der Speicherimplementierung
/// darf die Kette nicht brechen.
pub fn mode_to_str(mode: Mode) -> &'static str {
    match mode {
        Mode::M0Observe => "m0_observe",
        Mode::M1Workspace => "m1_workspace",
        Mode::M2Extended => "m2_extended",
        Mode::M3Autonomous => "m3_autonomous",
    }
}

fn str_to_mode(value: &str) -> Mode {
    match value {
        "m0_observe" => Mode::M0Observe,
        "m1_workspace" => Mode::M1Workspace,
        "m2_extended" => Mode::M2Extended,
        "m3_autonomous" => Mode::M3Autonomous,
        _ => Mode::M0Observe,
    }
}

/// Stabile Byte-Bezeichnung für die `CapabilityAction`.
pub fn action_to_str(action: CapabilityAction) -> &'static str {
    match action {
        CapabilityAction::FileRead => "file_read",
        CapabilityAction::FileWrite => "file_write",
        CapabilityAction::FileList => "file_list",
        CapabilityAction::Network => "network",
        CapabilityAction::Exec => "exec",
        CapabilityAction::Pure => "pure",
        CapabilityAction::ExaRequest => "exa_request",
        CapabilityAction::WebRequest => "web_request",
        CapabilityAction::MailRead => "mail_read",
        CapabilityAction::MailSend => "mail_send",
        CapabilityAction::CalendarRead => "calendar_read",
        CapabilityAction::CalendarWrite => "calendar_write",
        CapabilityAction::MicCapture => "mic_capture",
        CapabilityAction::ScreenCapture => "screen_capture",
        CapabilityAction::GitOp => "git_op",
        CapabilityAction::ProcessRun => "process_run",
    }
}

fn str_to_action(value: &str) -> CapabilityAction {
    match value {
        "file_read" => CapabilityAction::FileRead,
        "file_write" => CapabilityAction::FileWrite,
        "file_list" => CapabilityAction::FileList,
        "network" => CapabilityAction::Network,
        "exec" => CapabilityAction::Exec,
        "exa_request" => CapabilityAction::ExaRequest,
        "web_request" => CapabilityAction::WebRequest,
        "mail_read" => CapabilityAction::MailRead,
        "mail_send" => CapabilityAction::MailSend,
        "calendar_read" => CapabilityAction::CalendarRead,
        "calendar_write" => CapabilityAction::CalendarWrite,
        "mic_capture" => CapabilityAction::MicCapture,
        "screen_capture" => CapabilityAction::ScreenCapture,
        "git_op" => CapabilityAction::GitOp,
        "process_run" => CapabilityAction::ProcessRun,
        _ => CapabilityAction::Pure,
    }
}

/// Stabile Byte-Bezeichnung für das `AuditOutcome`.
pub fn outcome_to_str(outcome: AuditOutcome) -> &'static str {
    match outcome {
        AuditOutcome::Allow => "allow",
        AuditOutcome::Prompt => "prompt",
        AuditOutcome::Deny => "deny",
    }
}

fn str_to_outcome(value: &str) -> AuditOutcome {
    match value {
        "allow" => AuditOutcome::Allow,
        "prompt" => AuditOutcome::Prompt,
        _ => AuditOutcome::Deny,
    }
}

/// Abstraktion des Audit-Speichers.
///
/// `AuditStore` (SQLite/In-Memory) implementiert diesen Trait direkt; die
/// Bridge in `pa-launcher::vault_audit` implementiert ihn über eine
/// gemeinsame `HotVault`-Sperre, damit der Vault-Log dieselben Byte-Werte
/// enthält.
pub trait AuditSink {
    /// Persistiert einen Eintrag und liefert den vollständigen `AuditRecord` zurück.
    fn append(&mut self, entry: AuditLog, now_unix_ms: i64) -> Result<AuditRecord, PolicyError>;
}

impl AuditSink for AuditStore {
    fn append(&mut self, entry: AuditLog, now_unix_ms: i64) -> Result<AuditRecord, PolicyError> {
        AuditStore::append(self, entry, now_unix_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_chain_verifies_when_untouched() {
        let mut store = AuditStore::open_in_memory().unwrap();
        for i in 0..5 {
            store
                .append(
                    AuditLog {
                        mode: Mode::M1Workspace,
                        action: CapabilityAction::FileRead,
                        target: Some(format!("/tmp/file{i}")),
                        outcome: AuditOutcome::Allow,
                        reason: format!("test {i}"),
                    },
                    1_000_000 + i,
                )
                .unwrap();
        }
        assert_eq!(store.verify().unwrap(), 5);
    }

    #[test]
    fn tampered_content_breaks_hash_chain() {
        let mut store = AuditStore::open_in_memory().unwrap();
        store
            .append(
                AuditLog {
                    mode: Mode::M1Workspace,
                    action: CapabilityAction::FileWrite,
                    target: Some("/tmp/a".into()),
                    outcome: AuditOutcome::Allow,
                    reason: "eins".into(),
                },
                1,
            )
            .unwrap();
        store
            .append(
                AuditLog {
                    mode: Mode::M1Workspace,
                    action: CapabilityAction::FileWrite,
                    target: Some("/tmp/b".into()),
                    outcome: AuditOutcome::Allow,
                    reason: "zwei".into(),
                },
                2,
            )
            .unwrap();
        // Simuliert eine Manipulation: reason des ersten Eintrags wird geändert.
        store
            .connection_mut()
            .execute("UPDATE audit_log SET reason = 'gefälscht' WHERE id = 1", [])
            .unwrap();
        let err = store.verify().unwrap_err();
        assert!(matches!(err, PolicyError::Storage(_)));
    }

    #[test]
    fn deny_entries_are_persisted_too() {
        let mut store = AuditStore::open_in_memory().unwrap();
        store
            .append(
                AuditLog {
                    mode: Mode::M0Observe,
                    action: CapabilityAction::FileWrite,
                    target: Some("/tmp/x".into()),
                    outcome: AuditOutcome::Deny,
                    reason: "M0 verbietet Schreiben".into(),
                },
                10,
            )
            .unwrap();
        let all = store.all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].outcome, AuditOutcome::Deny);
    }
}
