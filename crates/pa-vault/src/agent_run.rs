//! Persistenter Speicher für Agentenläufe (Schemaversion 4).
//!
//! Wie `audit.rs` bewusst frei von pa-agents-Typen (Kreisabhängigkeit): der
//! Aufrufer übergibt bereits serialisiertes JSON in `payload` und bekommt es
//! genauso zurück. Die ausgelagerten Spalten (`stage`, `updated_unix_ms`,
//! `aborted`, `early_stopped`) erlauben die spätere UI-Sortierung ohne
//! JSON-Parsing.

use rusqlite::{params, Connection, OptionalExtension};

use crate::VaultError;

/// Ein einzelner persistierter Agentenlauf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunRow {
    pub id: String,
    pub stage: String,
    pub created_unix_ms: i64,
    pub updated_unix_ms: i64,
    pub aborted: bool,
    pub early_stopped: bool,
    pub payload: String,
}

/// Fügt einen Lauf ein oder überschreibt ihn vollständig (Upsert per PK).
pub fn upsert(connection: &mut Connection, row: &AgentRunRow) -> Result<(), VaultError> {
    connection.execute(
        "INSERT INTO agent_runs (id, stage, created_unix_ms, updated_unix_ms,
                                 aborted, early_stopped, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
             stage = excluded.stage,
             updated_unix_ms = excluded.updated_unix_ms,
             aborted = excluded.aborted,
             early_stopped = excluded.early_stopped,
             payload = excluded.payload",
        params![
            row.id,
            row.stage,
            row.created_unix_ms,
            row.updated_unix_ms,
            row.aborted as i64,
            row.early_stopped as i64,
            row.payload,
        ],
    )?;
    Ok(())
}

/// Liest einen Lauf per ID; `None` wenn er nicht existiert.
pub fn get(connection: &Connection, id: &str) -> Result<Option<AgentRunRow>, VaultError> {
    connection
        .query_row(
            "SELECT id, stage, created_unix_ms, updated_unix_ms,
                    aborted, early_stopped, payload
             FROM agent_runs WHERE id = ?1",
            [id],
            map_row,
        )
        .optional()
        .map_err(VaultError::from)
}

/// Liste aller Läufe, neueste zuerst (nach `updated_unix_ms`).
pub fn list(connection: &Connection, limit: Option<u32>) -> Result<Vec<AgentRunRow>, VaultError> {
    let base = "SELECT id, stage, created_unix_ms, updated_unix_ms,
                       aborted, early_stopped, payload
                FROM agent_runs ORDER BY updated_unix_ms DESC, id ASC";
    let mut statement = match limit {
        Some(_) => connection.prepare(&format!("{base} LIMIT ?1"))?,
        None => connection.prepare(base)?,
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

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentRunRow> {
    Ok(AgentRunRow {
        id: row.get(0)?,
        stage: row.get(1)?,
        created_unix_ms: row.get(2)?,
        updated_unix_ms: row.get(3)?,
        aborted: row.get::<_, i64>(4)? != 0,
        early_stopped: row.get::<_, i64>(5)? != 0,
        payload: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_migrated() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&connection).unwrap();
        connection
    }

    fn sample(id: &str, updated: i64) -> AgentRunRow {
        AgentRunRow {
            id: id.to_owned(),
            stage: "l2_critique".to_owned(),
            created_unix_ms: 1000,
            updated_unix_ms: updated,
            aborted: false,
            early_stopped: false,
            payload: format!(r#"{{"id":"{id}","stage":"l2_critique"}}"#),
        }
    }

    #[test]
    fn upsert_inserts_then_replaces_by_id() {
        let mut connection = open_migrated();
        upsert(&mut connection, &sample("run-a", 10)).unwrap();
        assert_eq!(
            get(&connection, "run-a").unwrap().unwrap().updated_unix_ms,
            10
        );
        let mut updated = sample("run-a", 20);
        updated.aborted = true;
        upsert(&mut connection, &updated).unwrap();
        let fetched = get(&connection, "run-a").unwrap().unwrap();
        assert_eq!(fetched.updated_unix_ms, 20);
        assert!(fetched.aborted);
        assert_eq!(list(&connection, None).unwrap().len(), 1);
    }

    #[test]
    fn list_orders_newest_first() {
        let mut connection = open_migrated();
        upsert(&mut connection, &sample("older", 100)).unwrap();
        upsert(&mut connection, &sample("newer", 200)).unwrap();
        let listed = list(&connection, None).unwrap();
        assert_eq!(listed[0].id, "newer");
        assert_eq!(listed[1].id, "older");
    }

    #[test]
    fn get_returns_none_for_missing() {
        let connection = open_migrated();
        assert!(get(&connection, "nirgends").unwrap().is_none());
    }
}
