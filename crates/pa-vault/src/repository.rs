use std::path::Path;

use pa_types::chat::{Conversation, Message, MessageRole, MessageStatus};
use rand_core::{OsRng, RngCore};
use rusqlite::{params, types::ValueRef, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

use crate::{
    agent_run::{self, AgentRunRow},
    audit::{self, AuditRow, AuditWrite},
    key::VaultKey,
    open_encrypted, open_encrypted_read_only, schema, VaultError,
};

const GENERATION_KEY: &str = "__pa_vault_generation";
const REVISION_KEY: &str = "__pa_vault_revision";

/// Identifiziert einen logischen Vaultstand auch bei gleich hohen, verzweigten Zählern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VaultIdentity {
    pub generation: u64,
    pub revision: String,
}

/// Hält die entschlüsselte SQLCipher-Verbindung innerhalb einer klaren Besitzgrenze.
pub struct VaultRepository {
    connection: Connection,
}

impl VaultRepository {
    /// Öffnet, authentifiziert und migriert erst nach einer echten verschlüsselten Schemaabfrage.
    pub fn open(path: &Path, key: &VaultKey) -> Result<Self, VaultError> {
        let connection = open_encrypted(path, key)?;
        schema::migrate(&connection)?;
        Ok(Self { connection })
    }

    /// Legt eine vom Core vergebene stabile ID ohne versteckte Beispieldaten an.
    pub fn create_conversation(
        &mut self,
        id: &str,
        title: &str,
        now_unix_ms: i64,
    ) -> Result<(), VaultError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO conversations(id, title, created_at_unix_ms, updated_at_unix_ms)
             VALUES (?1, ?2, ?3, ?3)",
            params![id, title, now_unix_ms],
        )?;
        bump_generation(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    /// Aktualisiert Titel und Sortierzeitpunkt zusammen, damit die UI-Reihenfolge stabil bleibt.
    pub fn rename_conversation(
        &mut self,
        id: &str,
        title: &str,
        now_unix_ms: i64,
    ) -> Result<(), VaultError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE conversations SET title = ?2, updated_at_unix_ms = ?3 WHERE id = ?1",
            params![id, title, now_unix_ms],
        )?;
        bump_generation(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    /// Löscht durch den Fremdschlüssel auch alle Nachrichten derselben Konversation.
    pub fn delete_conversation(&mut self, id: &str) -> Result<(), VaultError> {
        let transaction = self.connection.transaction()?;
        // Quellen hängen an Nachrichten ohne Fremdschlüssel; darum hier mit entfernen.
        transaction.execute(
            "DELETE FROM message_sources WHERE message_id IN (SELECT id FROM messages WHERE conversation_id = ?1)",
            [id],
        )?;
        transaction.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
        bump_generation(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    /// Sortiert explizit nach letzter Änderung und nicht nach zufälliger Tabellenreihenfolge.
    pub fn conversations(&self) -> Result<Vec<Conversation>, VaultError> {
        let mut statement = self.connection.prepare(
            "SELECT id, title, created_at_unix_ms, updated_at_unix_ms, project_id
             FROM conversations ORDER BY updated_at_unix_ms DESC, id ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at_unix_ms: row.get(2)?,
                updated_at_unix_ms: row.get(3)?,
                project_id: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(VaultError::from)
    }

    /// Vergibt die nächste Position in derselben Transaktion wie das Einfügen.
    pub fn append_message(
        &mut self,
        id: &str,
        conversation_id: &str,
        role: MessageRole,
        content: &str,
        status: MessageStatus,
        now_unix_ms: i64,
    ) -> Result<(), VaultError> {
        let transaction = self.connection.transaction()?;
        let position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM messages WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO messages(id, conversation_id, position, role, content, status, created_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                conversation_id,
                position,
                role_text(role),
                content,
                status_text(status),
                now_unix_ms
            ],
        )?;
        transaction.execute(
            "UPDATE conversations SET updated_at_unix_ms = ?2 WHERE id = ?1",
            params![conversation_id, now_unix_ms],
        )?;
        bump_generation(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    /// Schreibt Streaming-Endzustand und letzten Inhalt atomar in dieselbe Zeile.
    pub fn finish_message(
        &mut self,
        id: &str,
        content: &str,
        status: MessageStatus,
    ) -> Result<(), VaultError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE messages SET content = ?2, status = ?3 WHERE id = ?1",
            params![id, content, status_text(status)],
        )?;
        bump_generation(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    /// Liest ausschließlich über die persistierte Position, damit Teilantworten erhalten bleiben.
    pub fn messages(&self, conversation_id: &str) -> Result<Vec<Message>, VaultError> {
        let mut statement = self.connection.prepare(
            "SELECT id, conversation_id, position, role, content, status, created_at_unix_ms
             FROM messages WHERE conversation_id = ?1 ORDER BY position ASC",
        )?;
        let rows = statement.query_map([conversation_id], |row| {
            let role: String = row.get(3)?;
            let status: String = row.get(5)?;
            Ok(Message {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                position: row.get(2)?,
                role: parse_role(&role)?,
                content: row.get(4)?,
                status: parse_status(&status)?,
                created_at_unix_ms: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(VaultError::from)
    }

    /// Speichert MVP-Einstellungen als typneutralen JSON- oder Textwert ohne spätere Tabellen vorwegzunehmen.
    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<(), VaultError> {
        if matches!(key, GENERATION_KEY | REVISION_KEY) {
            return Err(VaultError::Integrity(
                "reservierter Vault-Einstellungsschlüssel".to_owned(),
            ));
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        bump_generation(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    /// Unterscheidet einen fehlenden Wert von einem leeren gespeicherten Wert.
    pub fn setting(&self, key: &str) -> Result<Option<String>, VaultError> {
        self.connection
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(VaultError::from)
    }

    /// Hängt einen Audit-Eintrag an; Hash-Kette wird identisch zu
    /// [`pa_policy::audit`] geführt, damit die Kette überprüfbar bleibt,
    /// wenn beide Wege je genutzt werden. `bump_generation` läuft nicht
    /// mit, weil der Audit-Log seinen eigenen Fortschritt über die
    /// Hash-Kette dokumentiert.
    pub fn append_audit(
        &mut self,
        write: AuditWrite<'_>,
        now_unix_ms: i64,
    ) -> Result<AuditRow, VaultError> {
        audit::append(&mut self.connection, write, now_unix_ms)
    }

    /// Alle Einträge in Einfügungsreihenfolge, optional begrenzt.
    pub fn audit_entries(&self, limit: Option<u32>) -> Result<Vec<AuditRow>, VaultError> {
        audit::list(&self.connection, limit)
    }

    /// Verifiziert die gesamte Kette und liefert die Zahl geprüfter Einträge.
    pub fn verify_audit(&self) -> Result<usize, VaultError> {
        audit::verify(&self.connection)
    }

    /// Aktuelle Kettenspitze (32 Byte); Nullbytes wenn leer.
    pub fn audit_head_hash(&self) -> Result<Vec<u8>, VaultError> {
        audit::head_hash(&self.connection)
    }

    /// Persistiert einen Agentenlauf (Schema V4). Der Payload trägt die
    /// vollständige serialisierte `AgentRun`; der Aufrufer entscheidet über
    /// die JSON-Kodierung. `bump_generation` bleibt aus, weil Agentenläufe
    /// eine eigene Kette per `updated_unix_ms` haben.
    pub fn upsert_agent_run(&mut self, row: &AgentRunRow) -> Result<(), VaultError> {
        agent_run::upsert(&mut self.connection, row)
    }

    /// Liefert einen Agentenlauf anhand seiner ID, falls vorhanden.
    pub fn agent_run(&self, id: &str) -> Result<Option<AgentRunRow>, VaultError> {
        agent_run::get(&self.connection, id)
    }

    /// Liefert alle Agentenläufe, neueste zuerst (optionales Limit).
    pub fn agent_runs(&self, limit: Option<u32>) -> Result<Vec<AgentRunRow>, VaultError> {
        agent_run::list(&self.connection, limit)
    }

    /// Führt MAC-/Seitenprüfung von SQLCipher und danach die logische SQLite-Prüfung aus.
    pub fn check_integrity(&self) -> Result<(), VaultError> {
        let mut cipher_statement = self.connection.prepare("PRAGMA cipher_integrity_check")?;
        let cipher_rows = cipher_statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if !cipher_rows.is_empty() && cipher_rows.iter().any(|row| row != "ok") {
            return Err(VaultError::Integrity(cipher_rows.join("; ")));
        }
        let sqlite: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if sqlite != "ok" {
            return Err(VaultError::Integrity(sqlite));
        }
        Ok(())
    }

    /// Prüft einen Recovery-Kandidaten ohne Migration oder sonstigen Schreibzugriff.
    pub(crate) fn inspect_identity(
        path: &Path,
        key: &VaultKey,
    ) -> Result<VaultIdentity, VaultError> {
        let connection = open_encrypted_read_only(path, key)?;
        let repository = Self { connection };
        repository.check_integrity()?;
        repository.identity()
    }

    /// Liest den verschlüsselten logischen Stand statt unzuverlässiger Dateizeitstempel.
    pub fn generation(&self) -> Result<u64, VaultError> {
        let value = self
            .connection
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [GENERATION_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        value.map_or(Ok(0), |generation| {
            generation
                .parse::<u64>()
                .map_err(|_| VaultError::Integrity("ungültige Vault-Generation".to_owned()))
        })
    }

    fn identity(&self) -> Result<VaultIdentity, VaultError> {
        let generation = self.generation()?;
        let revision = match self.setting(REVISION_KEY)? {
            Some(revision) if !revision.is_empty() => revision,
            Some(_) => {
                return Err(VaultError::Integrity("Vault-Revision ist leer".to_owned()));
            }
            None => legacy_fingerprint(&self.connection)?,
        };
        Ok(VaultIdentity {
            generation,
            revision,
        })
    }

    /// Ermöglicht dem Snapshot-Modul kontrollierten Zugriff auf SQLCipher-Backup-APIs.
    ///
    /// Ist außerdem der offizielle Zugriffspunkt für die pa-memory-Persistenz
    /// (BM25/Vektor/Fakten), die keine eigene Verbindung öffnen darf.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Mutabler Zugriff für pa-memory-Operationen, die eine
    /// Transaktion starten müssen; die Sperre nimmt der Aufrufer bereits
    /// über die `HotVault`-Fassade.
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }
}

fn legacy_fingerprint(connection: &Connection) -> Result<String, VaultError> {
    let mut hasher = Sha256::new();
    for (table, query, columns) in [
        (
            "schema_version",
            "SELECT version FROM schema_version ORDER BY version",
            1_usize,
        ),
        (
            "conversations",
            "SELECT id, title, created_at_unix_ms, updated_at_unix_ms FROM conversations ORDER BY id",
            4,
        ),
        (
            "messages",
            "SELECT id, conversation_id, position, role, content, status, created_at_unix_ms FROM messages ORDER BY id",
            7,
        ),
        (
            "settings",
            "SELECT key, value FROM settings
             WHERE key <> '__pa_vault_generation' AND key <> '__pa_vault_revision'
             ORDER BY key",
            2,
        ),
    ] {
        hasher.update(table.as_bytes());
        let mut statement = connection.prepare(query)?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            for index in 0..columns {
                match row.get_ref(index)? {
                    ValueRef::Null => hasher.update([0]),
                    ValueRef::Integer(value) => {
                        hasher.update([1]);
                        hasher.update(value.to_le_bytes());
                    }
                    ValueRef::Real(value) => {
                        hasher.update([2]);
                        hasher.update(value.to_bits().to_le_bytes());
                    }
                    ValueRef::Text(value) => {
                        hasher.update([3]);
                        update_length_prefixed(&mut hasher, value);
                    }
                    ValueRef::Blob(value) => {
                        hasher.update([4]);
                        update_length_prefixed(&mut hasher, value);
                    }
                }
            }
        }
    }
    Ok(format!("legacy-{}", hex::encode(hasher.finalize())))
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

pub(crate) fn bump_generation(transaction: &Transaction<'_>) -> Result<(), VaultError> {
    transaction.execute(
        "INSERT INTO settings(key, value) VALUES (?1, '1')
         ON CONFLICT(key) DO UPDATE SET value = CAST(value AS INTEGER) + 1",
        [GENERATION_KEY],
    )?;
    let mut revision = [0_u8; 16];
    OsRng.fill_bytes(&mut revision);
    transaction.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![REVISION_KEY, hex::encode(revision)],
    )?;
    Ok(())
}

fn role_text(role: MessageRole) -> &'static str {
    match role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
    }
}

fn status_text(status: MessageStatus) -> &'static str {
    match status {
        MessageStatus::Complete => "complete",
        MessageStatus::Streaming => "streaming",
        MessageStatus::Aborted => "aborted",
    }
}

fn parse_role(value: &str) -> rusqlite::Result<MessageRole> {
    match value {
        "system" => Ok(MessageRole::System),
        "user" => Ok(MessageRole::User),
        "assistant" => Ok(MessageRole::Assistant),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_status(value: &str) -> rusqlite::Result<MessageStatus> {
    match value {
        "complete" => Ok(MessageStatus::Complete),
        "streaming" => Ok(MessageStatus::Streaming),
        "aborted" => Ok(MessageStatus::Aborted),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}
