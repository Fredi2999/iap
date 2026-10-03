//! Agent-Flow-Aufgaben und freigegebene Host-Projekte im Tresor (Schema-Version 6).
//!
//! Die Worktrees selbst liegen sichtbar beim Repository (Konzept 10.5); hier
//! stehen nur ihre Metadaten, damit Kandidaten nach einem Neustart auffindbar
//! und bewusst bereinigbar bleiben.

use pa_types::agent_flow::{AgentFlowTask, ProjectLocation};
use rusqlite::{params, Connection, OptionalExtension};

use crate::{repository::bump_generation, VaultError};

fn location_text(location: ProjectLocation) -> &'static str {
    match location {
        ProjectLocation::Stick => "stick",
        ProjectLocation::Host => "host",
    }
}

/// Speichert oder aktualisiert eine Aufgabe samt Kandidaten.
pub fn upsert_task(connection: &mut Connection, task: &AgentFlowTask) -> Result<(), VaultError> {
    let payload = serde_json::to_string(task)
        .map_err(|e| VaultError::Integrity(format!("JSON schreiben: {e}")))?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO agent_flow_tasks(id, project_path, created_at_unix_ms, payload)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
        params![task.id, task.project_path, task.created_unix_ms, payload],
    )?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

fn parse(payload: &str) -> Result<AgentFlowTask, VaultError> {
    serde_json::from_str(payload).map_err(|e| VaultError::Integrity(format!("JSON lesen: {e}")))
}

/// Eine Aufgabe nach ID.
pub fn get_task(connection: &Connection, id: &str) -> Result<Option<AgentFlowTask>, VaultError> {
    let payload: Option<String> = connection
        .query_row(
            "SELECT payload FROM agent_flow_tasks WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    payload.as_deref().map(parse).transpose()
}

/// Alle Aufgaben, neueste zuerst.
pub fn list_tasks(connection: &Connection) -> Result<Vec<AgentFlowTask>, VaultError> {
    let mut statement = connection
        .prepare("SELECT payload FROM agent_flow_tasks ORDER BY created_at_unix_ms DESC, id ASC")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(parse(&row?)?);
    }
    Ok(out)
}

/// Entfernt die Metadaten einer Aufgabe (nach bewusster Bereinigung).
pub fn delete_task(connection: &mut Connection, id: &str) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM agent_flow_tasks WHERE id = ?1", [id])?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Merkt sich die ausdrückliche Freigabe eines Projekts.
pub fn approve_project(
    connection: &mut Connection,
    path: &str,
    location: ProjectLocation,
    approved_unix_ms: i64,
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO agent_flow_projects(path, location, approved_at_unix_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(path) DO UPDATE SET location = excluded.location,
             approved_at_unix_ms = excluded.approved_at_unix_ms",
        params![path, location_text(location), approved_unix_ms],
    )?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Ob das Projekt ausdrücklich freigegeben wurde.
pub fn is_project_approved(connection: &Connection, path: &str) -> Result<bool, VaultError> {
    let found: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM agent_flow_projects WHERE path = ?1",
            [path],
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

/// Nimmt die Freigabe zurück.
pub fn revoke_project(connection: &mut Connection, path: &str) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM agent_flow_projects WHERE path = ?1", [path])?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Alle freigegebenen Projekte: `(Pfad, Ort)`.
pub fn list_approved_projects(
    connection: &Connection,
) -> Result<Vec<(String, ProjectLocation)>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT path, location FROM agent_flow_projects ORDER BY approved_at_unix_ms DESC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (path, location) = row?;
        let location = if location == "host" {
            ProjectLocation::Host
        } else {
            ProjectLocation::Stick
        };
        out.push((path, location));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_types::agent_flow::{BaseChoice, BaseInfo};

    fn open() -> Connection {
        let connection = Connection::open_in_memory().expect("Speicher-DB");
        crate::schema::migrate(&connection).expect("Migration");
        connection
    }

    fn task(id: &str, at: i64) -> AgentFlowTask {
        AgentFlowTask {
            id: id.to_owned(),
            project_path: "E:\\proj".to_owned(),
            location: ProjectLocation::Host,
            prompt: "Fehler beheben".to_owned(),
            base: BaseInfo {
                choice: BaseChoice::HeadCommit,
                base_commit: "abc".to_owned(),
                target_head: "abc".to_owned(),
            },
            candidates: vec![],
            created_unix_ms: at,
        }
    }

    #[test]
    fn tasks_round_trip_in_creation_order() {
        let mut connection = open();
        upsert_task(&mut connection, &task("a", 1)).expect("a");
        upsert_task(&mut connection, &task("b", 2)).expect("b");
        let mut changed = task("a", 1);
        changed.prompt = "anders".to_owned();
        upsert_task(&mut connection, &changed).expect("Update");
        let ids: Vec<String> = list_tasks(&connection)
            .expect("Liste")
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, vec!["b", "a"]);
        assert_eq!(get_task(&connection, "a").expect("Lesen"), Some(changed));
        delete_task(&mut connection, "a").expect("Löschen");
        assert!(get_task(&connection, "a").expect("Lesen").is_none());
    }

    #[test]
    fn host_project_approval_is_explicit_and_revocable() {
        let mut connection = open();
        assert!(!is_project_approved(&connection, "E:\\proj").expect("Prüfen"));
        approve_project(&mut connection, "E:\\proj", ProjectLocation::Host, 5).expect("Freigabe");
        assert!(is_project_approved(&connection, "E:\\proj").expect("Prüfen"));
        assert_eq!(
            list_approved_projects(&connection).expect("Liste"),
            vec![("E:\\proj".to_owned(), ProjectLocation::Host)]
        );
        revoke_project(&mut connection, "E:\\proj").expect("Entzug");
        assert!(!is_project_approved(&connection, "E:\\proj").expect("Prüfen"));
    }
}
