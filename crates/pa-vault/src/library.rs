//! Projekte und Dokumente im Tresor (Schema-Version 5).
//!
//! Projekte bündeln Unterhaltungen und tragen eine gemeinsame Grundanweisung.
//! Dokumente sind Texte, deren Abschnitte (Tabelle `chunks`) im Chat als
//! nummerierte Quellen dienen; sie werden je Unterhaltung angehängt.
//! Wie `memory.rs` arbeiten die Funktionen direkt auf der Verbindung, damit
//! `pa-memory` und die App sie ohne zusätzliche Hülle nutzen können.

use pa_types::ipc::{DocumentInfo, MessageSource, Project};
use rusqlite::{params, Connection, OptionalExtension};

use crate::{repository::bump_generation, VaultError};

/// Alle Projekte, zuletzt geänderte zuerst.
pub fn list_projects(connection: &Connection) -> Result<Vec<Project>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT id, name, system_prompt, created_at_unix_ms, updated_at_unix_ms
         FROM projects ORDER BY updated_at_unix_ms DESC, id ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            system_prompt: row.get(2)?,
            created_at_unix_ms: row.get(3)?,
            updated_at_unix_ms: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(VaultError::from)
}

/// Ein Projekt nach ID, etwa um seine Grundanweisung in den Prompt zu legen.
pub fn get_project(connection: &Connection, id: &str) -> Result<Option<Project>, VaultError> {
    connection
        .query_row(
            "SELECT id, name, system_prompt, created_at_unix_ms, updated_at_unix_ms
             FROM projects WHERE id = ?1",
            [id],
            |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    system_prompt: row.get(2)?,
                    created_at_unix_ms: row.get(3)?,
                    updated_at_unix_ms: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(VaultError::from)
}

/// Legt ein Projekt an oder ändert Name und Grundanweisung eines bestehenden.
pub fn upsert_project(connection: &mut Connection, project: &Project) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO projects(id, name, system_prompt, created_at_unix_ms, updated_at_unix_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
             name = excluded.name,
             system_prompt = excluded.system_prompt,
             updated_at_unix_ms = excluded.updated_at_unix_ms",
        params![
            project.id,
            project.name,
            project.system_prompt,
            project.created_at_unix_ms,
            project.updated_at_unix_ms
        ],
    )?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Löscht ein Projekt. Die Unterhaltungen bleiben erhalten und stehen danach
/// wieder ohne Projekt in der Liste (Fremdschlüssel `ON DELETE SET NULL`).
pub fn delete_project(connection: &mut Connection, id: &str) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM projects WHERE id = ?1", [id])?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Ordnet eine Unterhaltung einem Projekt zu oder löst die Zuordnung (`None`).
pub fn assign_conversation(
    connection: &mut Connection,
    conversation_id: &str,
    project_id: Option<&str>,
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    let changed = transaction.execute(
        "UPDATE conversations SET project_id = ?2 WHERE id = ?1",
        params![conversation_id, project_id],
    )?;
    if changed == 0 {
        return Err(VaultError::Integrity(format!(
            "Unterhaltung {conversation_id} existiert nicht"
        )));
    }
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Projekt einer Unterhaltung, falls zugeordnet.
pub fn conversation_project(
    connection: &Connection,
    conversation_id: &str,
) -> Result<Option<Project>, VaultError> {
    let project_id: Option<String> = connection
        .query_row(
            "SELECT project_id FROM conversations WHERE id = ?1",
            [conversation_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    match project_id {
        Some(id) => get_project(connection, &id),
        None => Ok(None),
    }
}

/// Merkt sich ein Dokument, nachdem seine Abschnitte gespeichert wurden.
pub fn insert_document(
    connection: &mut Connection,
    document: &DocumentInfo,
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO documents(id, name, kind, size_bytes, chunk_count, created_at_unix_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            document.id,
            document.name,
            document.kind,
            i64::try_from(document.size_bytes).unwrap_or(i64::MAX),
            document.chunk_count,
            document.created_at_unix_ms
        ],
    )?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Alle Dokumente, neueste zuerst.
pub fn list_documents(connection: &Connection) -> Result<Vec<DocumentInfo>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT id, name, kind, size_bytes, chunk_count, created_at_unix_ms
         FROM documents ORDER BY created_at_unix_ms DESC, id ASC",
    )?;
    let rows = statement.query_map([], |row| {
        let size: i64 = row.get(3)?;
        Ok(DocumentInfo {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            size_bytes: u64::try_from(size).unwrap_or(0),
            chunk_count: row.get(4)?,
            created_at_unix_ms: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(VaultError::from)
}

/// Entfernt ein Dokument samt Abschnitten, Such- und Vektorindex in einer Transaktion,
/// damit keine verwaisten Abschnitte mehr in Antworten auftauchen.
pub fn delete_document(connection: &mut Connection, id: &str) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "DELETE FROM fts_items WHERE item_type = 'chunk'
             AND item_id IN (SELECT id FROM chunks WHERE doc_id = ?1)",
        [id],
    )?;
    transaction.execute(
        "DELETE FROM vec_items WHERE item_type = 'chunk'
             AND item_id IN (SELECT id FROM chunks WHERE doc_id = ?1)",
        [id],
    )?;
    transaction.execute("DELETE FROM chunks WHERE doc_id = ?1", [id])?;
    transaction.execute("DELETE FROM documents WHERE id = ?1", [id])?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Hängt ein Dokument an eine Unterhaltung; mehrfaches Anhängen ist harmlos.
pub fn attach_document(
    connection: &mut Connection,
    conversation_id: &str,
    document_id: &str,
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT OR IGNORE INTO conversation_documents(conversation_id, document_id) VALUES (?1, ?2)",
        params![conversation_id, document_id],
    )?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Nimmt ein Dokument wieder von einer Unterhaltung ab.
pub fn detach_document(
    connection: &mut Connection,
    conversation_id: &str,
    document_id: &str,
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "DELETE FROM conversation_documents WHERE conversation_id = ?1 AND document_id = ?2",
        params![conversation_id, document_id],
    )?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// IDs der an eine Unterhaltung angehängten Dokumente.
pub fn conversation_document_ids(
    connection: &Connection,
    conversation_id: &str,
) -> Result<Vec<String>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT document_id FROM conversation_documents WHERE conversation_id = ?1 ORDER BY document_id",
    )?;
    let rows = statement.query_map([conversation_id], |row| row.get(0))?;
    rows.collect::<Result<Vec<String>, _>>()
        .map_err(VaultError::from)
}

/// Speichert die Quellen einer Antwort; ersetzt frühere Einträge derselben Nachricht.
pub fn save_message_sources(
    connection: &mut Connection,
    message_id: &str,
    sources: &[MessageSource],
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "DELETE FROM message_sources WHERE message_id = ?1",
        [message_id],
    )?;
    for source in sources {
        transaction.execute(
            "INSERT INTO message_sources(message_id, number, document_id, document_name, chunk_ordinal, excerpt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                message_id,
                source.number,
                source.document_id,
                source.document_name,
                source.chunk_ordinal,
                source.excerpt
            ],
        )?;
    }
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Alle Quellen der Antworten einer Unterhaltung, geordnet nach Nachricht und Nummer.
pub fn conversation_sources(
    connection: &Connection,
    conversation_id: &str,
) -> Result<Vec<MessageSource>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT s.message_id, s.number, s.document_id, s.document_name, s.chunk_ordinal, s.excerpt
         FROM message_sources s JOIN messages m ON m.id = s.message_id
         WHERE m.conversation_id = ?1 ORDER BY m.position, s.number",
    )?;
    let rows = statement.query_map([conversation_id], |row| {
        Ok(MessageSource {
            message_id: row.get(0)?,
            number: row.get(1)?,
            document_id: row.get(2)?,
            document_name: row.get(3)?,
            chunk_ordinal: row.get(4)?,
            excerpt: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(VaultError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_types::memory::Chunk;

    fn open_migrated() -> Connection {
        let connection = Connection::open_in_memory().expect("Speicher-DB");
        crate::schema::migrate(&connection).expect("Migration");
        connection
    }

    fn project(id: &str, name: &str, at: i64) -> Project {
        Project {
            id: id.to_owned(),
            name: name.to_owned(),
            system_prompt: "Antworte kurz.".to_owned(),
            created_at_unix_ms: at,
            updated_at_unix_ms: at,
        }
    }

    #[test]
    fn projects_group_conversations_and_survive_deletion_as_unassigned() {
        let mut connection = open_migrated();
        connection
            .execute("INSERT INTO conversations(id, title, created_at_unix_ms, updated_at_unix_ms) VALUES ('c1', 'A', 1, 1)", [])
            .expect("Unterhaltung");
        upsert_project(&mut connection, &project("p1", "Schule", 5)).expect("Projekt");
        assign_conversation(&mut connection, "c1", Some("p1")).expect("Zuordnung");
        assert_eq!(
            conversation_project(&connection, "c1")
                .expect("Lesen")
                .map(|p| p.name),
            Some("Schule".to_owned())
        );

        delete_project(&mut connection, "p1").expect("Löschen");
        assert!(conversation_project(&connection, "c1")
            .expect("Lesen")
            .is_none());
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get(0))
            .expect("Zählen");
        assert_eq!(count, 1, "Unterhaltung bleibt erhalten");
    }

    #[test]
    fn version_four_vault_keeps_conversations_after_upgrade() {
        // Stand eines bestehenden Sticks: Version 4 mit einer Unterhaltung ohne Projektspalte.
        let connection = Connection::open_in_memory().expect("Speicher-DB");
        connection
            .execute_batch(
                "CREATE TABLE schema_version (version INTEGER NOT NULL);
                 INSERT INTO schema_version(version) VALUES (4);
                 CREATE TABLE conversations (
                     id TEXT PRIMARY KEY NOT NULL, title TEXT NOT NULL,
                     created_at_unix_ms INTEGER NOT NULL, updated_at_unix_ms INTEGER NOT NULL
                 ) WITHOUT ROWID;
                 INSERT INTO conversations VALUES ('alt', 'Bestehend', 1, 2);",
            )
            .expect("Altbestand");
        crate::schema::migrate(&connection).expect("Migration");
        let version: i64 = connection
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .expect("Version");
        assert_eq!(version, crate::schema::SCHEMA_VERSION);
        let (title, project): (String, Option<String>) = connection
            .query_row(
                "SELECT title, project_id FROM conversations WHERE id = 'alt'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("Unterhaltung");
        assert_eq!((title.as_str(), project), ("Bestehend", None));
    }

    #[test]
    fn assigning_unknown_conversation_fails() {
        let mut connection = open_migrated();
        assert!(assign_conversation(&mut connection, "fehlt", None).is_err());
    }

    #[test]
    fn deleting_document_removes_chunks_and_attachments() {
        let mut connection = open_migrated();
        connection
            .execute("INSERT INTO conversations(id, title, created_at_unix_ms, updated_at_unix_ms) VALUES ('c1', 'A', 1, 1)", [])
            .expect("Unterhaltung");
        insert_document(
            &mut connection,
            &DocumentInfo {
                id: "d1".into(),
                name: "Plan.md".into(),
                kind: "md".into(),
                size_bytes: 10,
                chunk_count: 1,
                created_at_unix_ms: 2,
            },
        )
        .expect("Dokument");
        crate::memory::upsert_chunk(
            &mut connection,
            &Chunk {
                id: "d1#0".into(),
                doc_id: "d1".into(),
                ordinal: 0,
                text: "Inhalt".into(),
                heading_path: None,
                tokens: 2,
            },
        )
        .expect("Abschnitt");
        crate::memory::upsert_fts(
            &mut connection,
            pa_types::memory::ItemType::Chunk,
            "d1#0",
            "Inhalt",
        )
        .expect("Index");
        attach_document(&mut connection, "c1", "d1").expect("Anhängen");
        attach_document(&mut connection, "c1", "d1").expect("doppelt anhängen");
        assert_eq!(
            conversation_document_ids(&connection, "c1").expect("Lesen"),
            vec!["d1".to_owned()]
        );

        delete_document(&mut connection, "d1").expect("Löschen");
        assert!(list_documents(&connection).expect("Liste").is_empty());
        assert!(conversation_document_ids(&connection, "c1")
            .expect("Lesen")
            .is_empty());
        let chunks: i64 = connection
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .expect("Zählen");
        let fts: i64 = connection
            .query_row("SELECT COUNT(*) FROM fts_items", [], |row| row.get(0))
            .expect("Zählen");
        assert_eq!((chunks, fts), (0, 0));
    }
}
