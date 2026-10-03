//! Persistenter Audit-Log im Vault (Schemaversion 2).
//!
//! Diese Datei ist der Vault-seitige Gegenstück zu `pa_policy::audit`. Sie hält
//! bewusst keine `pa-policy`-Typen (Kreisabhängigkeit), sondern akzeptiert und
//! liefert kurze String-Felder. Der Aufrufer (typisch `pa-launcher` /
//! `pa-tools`) übersetzt zwischen pa-policy-Enums und den hier verwendeten
//! stabilen Byte-Repräsentationen — die Bezeichner sind mit
//! `pa_policy::audit::{mode_to_str, action_to_str, outcome_to_str}` identisch,
//! damit dieselbe Hash-Verkettung entsteht und ein späterer Bridge-AuditStore
//! Byte-für-Byte kompatibel bleibt.

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use crate::VaultError;

/// Rohzeile aus `audit_log`; Enum-Auflösung passiert außerhalb dieser Crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    pub id: i64,
    pub created_unix_ms: i64,
    pub mode: String,
    pub action: String,
    pub target: Option<String>,
    pub outcome: String,
    pub reason: String,
    pub prev_hash: Vec<u8>,
    pub hash: Vec<u8>,
}

/// Neuer Eintrag, wie ihn der Aufrufer übergibt.
///
/// Alle String-Felder werden 1:1 in den Hash aufgenommen. Der Aufrufer muss
/// die pa-policy-Bezeichner verwenden (`m0_observe`, `file_read`, `allow`, …),
/// damit die Kette später verifizierbar bleibt.
#[derive(Debug, Clone)]
pub struct AuditWrite<'a> {
    pub mode: &'a str,
    pub action: &'a str,
    pub target: Option<&'a str>,
    pub outcome: &'a str,
    pub reason: &'a str,
}

/// Hängt einen Eintrag an; die Verkettung folgt derselben Formel wie
/// `pa_policy::audit::compute_hash`.
///
/// Der Aufruf läuft in einer eigenen Transaktion; parallele Writes einer
/// zweiten Verbindung sind über SQLite-Row-Locks serialisiert.
pub fn append(
    connection: &mut Connection,
    write: AuditWrite<'_>,
    now_unix_ms: i64,
) -> Result<AuditRow, VaultError> {
    let transaction = connection.transaction()?;
    let prev_hash: Vec<u8> = transaction
        .query_row(
            "SELECT hash FROM audit_log ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .unwrap_or_else(|_| vec![0_u8; 32]);
    let hash = compute_hash(&prev_hash, &write, now_unix_ms);
    transaction.execute(
        "INSERT INTO audit_log (created_unix_ms, mode, action, target, outcome, reason, prev_hash, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            now_unix_ms,
            write.mode,
            write.action,
            write.target,
            write.outcome,
            write.reason,
            prev_hash,
            hash,
        ],
    )?;
    let id = transaction.last_insert_rowid();
    transaction.commit()?;
    Ok(AuditRow {
        id,
        created_unix_ms: now_unix_ms,
        mode: write.mode.to_owned(),
        action: write.action.to_owned(),
        target: write.target.map(str::to_owned),
        outcome: write.outcome.to_owned(),
        reason: write.reason.to_owned(),
        prev_hash,
        hash,
    })
}

/// Liest bis zu `limit` Einträge in Einfügungsreihenfolge; ohne `limit` alle.
pub fn list(connection: &Connection, limit: Option<u32>) -> Result<Vec<AuditRow>, VaultError> {
    let base = "SELECT id, created_unix_ms, mode, action, target, outcome, reason, prev_hash, hash
                FROM audit_log ORDER BY id ASC";
    let mut statement = match limit {
        Some(_) => connection.prepare(&format!("{base} LIMIT ?1"))?,
        None => connection.prepare(base)?,
    };
    let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<AuditRow> {
        Ok(AuditRow {
            id: row.get(0)?,
            created_unix_ms: row.get(1)?,
            mode: row.get(2)?,
            action: row.get(3)?,
            target: row.get(4)?,
            outcome: row.get(5)?,
            reason: row.get(6)?,
            prev_hash: row.get(7)?,
            hash: row.get(8)?,
        })
    };
    let rows = if let Some(limit) = limit {
        statement
            .query_map(params![limit], map_row)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        statement
            .query_map([], map_row)?
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(rows)
}

/// Prüft die Hashkette; liefert die Anzahl geprüfter Einträge oder einen
/// [`VaultError::Integrity`] beim ersten Bruch.
pub fn verify(connection: &Connection) -> Result<usize, VaultError> {
    let entries = list(connection, None)?;
    let mut previous = vec![0_u8; 32];
    for (index, entry) in entries.iter().enumerate() {
        if entry.prev_hash != previous {
            return Err(VaultError::Integrity(format!(
                "Audit-Kette bei Eintrag {index} unterbrochen (prev_hash weicht ab)"
            )));
        }
        let expected = compute_hash(
            &previous,
            &AuditWrite {
                mode: &entry.mode,
                action: &entry.action,
                target: entry.target.as_deref(),
                outcome: &entry.outcome,
                reason: &entry.reason,
            },
            entry.created_unix_ms,
        );
        if expected != entry.hash {
            return Err(VaultError::Integrity(format!(
                "Audit-Hash von Eintrag {index} passt nicht zu seinem Inhalt"
            )));
        }
        previous = entry.hash.clone();
    }
    Ok(entries.len())
}

/// Aktuelle Kettenspitze (32-Byte-SHA-256, oder Nullbytes wenn leer).
///
/// Wird u. a. beim Sync-Vorabgleich verwendet, damit die UI sofort erkennt,
/// ob die Hostkopie noch mit dem Stick übereinstimmt.
pub fn head_hash(connection: &Connection) -> Result<Vec<u8>, VaultError> {
    let head: Option<Vec<u8>> = connection
        .query_row(
            "SELECT hash FROM audit_log ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    Ok(head.unwrap_or_else(|| vec![0_u8; 32]))
}

fn compute_hash(prev: &[u8], write: &AuditWrite<'_>, now: i64) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(prev);
    hasher.update(now.to_le_bytes());
    hasher.update(write.mode.as_bytes());
    hasher.update([0]);
    hasher.update(write.action.as_bytes());
    hasher.update([0]);
    hasher.update(write.target.unwrap_or("").as_bytes());
    hasher.update([0]);
    hasher.update(write.outcome.as_bytes());
    hasher.update([0]);
    hasher.update(write.reason.as_bytes());
    hasher.finalize().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_migrated() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&connection).unwrap();
        connection
    }

    #[test]
    fn append_then_list_returns_the_same_row() {
        let mut connection = open_migrated();
        let row = append(
            &mut connection,
            AuditWrite {
                mode: "m1_workspace",
                action: "file_read",
                target: Some("hello.txt"),
                outcome: "allow",
                reason: "Testlauf",
            },
            42,
        )
        .unwrap();
        assert_eq!(row.id, 1);
        assert_eq!(row.prev_hash, vec![0_u8; 32]);
        assert_eq!(row.hash.len(), 32);
        let listed = list(&connection, None).unwrap();
        assert_eq!(listed, vec![row]);
    }

    #[test]
    fn hash_chain_links_consecutive_entries() {
        let mut connection = open_migrated();
        let a = append(
            &mut connection,
            AuditWrite {
                mode: "m1_workspace",
                action: "file_read",
                target: Some("a.txt"),
                outcome: "allow",
                reason: "a",
            },
            1,
        )
        .unwrap();
        let b = append(
            &mut connection,
            AuditWrite {
                mode: "m1_workspace",
                action: "file_write",
                target: Some("b.txt"),
                outcome: "prompt",
                reason: "b",
            },
            2,
        )
        .unwrap();
        assert_eq!(b.prev_hash, a.hash);
        assert_eq!(verify(&connection).unwrap(), 2);
    }

    #[test]
    fn tampering_breaks_verification() {
        let mut connection = open_migrated();
        append(
            &mut connection,
            AuditWrite {
                mode: "m1_workspace",
                action: "file_read",
                target: Some("a.txt"),
                outcome: "allow",
                reason: "original",
            },
            1,
        )
        .unwrap();
        connection
            .execute("UPDATE audit_log SET reason = 'gefälscht' WHERE id = 1", [])
            .unwrap();
        let err = verify(&connection).unwrap_err();
        assert!(matches!(err, VaultError::Integrity(_)));
    }

    #[test]
    fn head_hash_is_zero_when_empty_and_matches_last_after_write() {
        let mut connection = open_migrated();
        assert_eq!(head_hash(&connection).unwrap(), vec![0_u8; 32]);
        let row = append(
            &mut connection,
            AuditWrite {
                mode: "m0_observe",
                action: "file_write",
                target: Some("blocked.txt"),
                outcome: "deny",
                reason: "M0 verbietet Schreiben",
            },
            99,
        )
        .unwrap();
        assert_eq!(head_hash(&connection).unwrap(), row.hash);
    }
}
