//! Workflow-Definitionen und Laufprotokolle im Tresor (Schema-Version 6).
//!
//! Warum im Tresor: AGENTS-Invariante 6 – kein Zustand außerhalb. Das Protokoll
//! enthält nur, was die Oberfläche ohnehin zeigt (Schritte, Datenübergänge mit
//! Herkunft, Verbrauch); Schlüssel und rohe Webinhalte gehören nicht hinein.

use pa_types::flow::{
    RunBudget, RunEvent, RunEventKind, RunStatus, RunUsage, WorkflowDefinition, WorkflowGraph,
    WorkflowResult, WorkflowRunReport,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use crate::{repository::bump_generation, VaultError};

fn to_json<T: Serialize>(value: &T) -> Result<String, VaultError> {
    serde_json::to_string(value).map_err(|e| VaultError::Integrity(format!("JSON schreiben: {e}")))
}

fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, VaultError> {
    serde_json::from_str(text).map_err(|e| VaultError::Integrity(format!("JSON lesen: {e}")))
}

fn enum_text<T: Serialize>(value: &T) -> Result<String, VaultError> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => Ok(text),
        _ => Err(VaultError::Integrity("Aufzählung ist kein Text".to_owned())),
    }
}

fn enum_from_text<T: DeserializeOwned>(text: &str) -> Result<T, VaultError> {
    from_json(&format!("\"{text}\""))
}

/// Legt eine Definition an oder überschreibt sie.
pub fn save_workflow(
    connection: &mut Connection,
    definition: &WorkflowDefinition,
) -> Result<(), VaultError> {
    let graph = to_json(&definition.graph)?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO workflows(id, name, graph, updated_at_unix_ms) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, graph = excluded.graph,
             updated_at_unix_ms = excluded.updated_at_unix_ms",
        params![
            definition.id,
            definition.name,
            graph,
            definition.updated_unix_ms
        ],
    )?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

fn workflow_from_row(
    id: String,
    name: String,
    graph: String,
    updated: i64,
) -> Result<WorkflowDefinition, VaultError> {
    Ok(WorkflowDefinition {
        id,
        name,
        graph: from_json::<WorkflowGraph>(&graph)?,
        updated_unix_ms: updated,
    })
}

/// Alle Definitionen, zuletzt geänderte zuerst.
pub fn list_workflows(connection: &Connection) -> Result<Vec<WorkflowDefinition>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT id, name, graph, updated_at_unix_ms FROM workflows
         ORDER BY updated_at_unix_ms DESC, id ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name, graph, updated) = row?;
        out.push(workflow_from_row(id, name, graph, updated)?);
    }
    Ok(out)
}

/// Eine Definition nach ID.
pub fn get_workflow(
    connection: &Connection,
    id: &str,
) -> Result<Option<WorkflowDefinition>, VaultError> {
    let row = connection
        .query_row(
            "SELECT id, name, graph, updated_at_unix_ms FROM workflows WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;
    row.map(|(id, name, graph, updated)| workflow_from_row(id, name, graph, updated))
        .transpose()
}

/// Löscht eine Definition. Bisherige Läufe bleiben als Protokoll erhalten.
pub fn delete_workflow(connection: &mut Connection, id: &str) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM workflows WHERE id = ?1", [id])?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Legt den Lauf an (Status `running`), bevor der erste Schritt startet.
pub fn create_run(
    connection: &mut Connection,
    run_id: &str,
    workflow: &WorkflowDefinition,
    budget: &RunBudget,
    started_unix_ms: i64,
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO workflow_runs(id, workflow_id, workflow_name, status, budget, usage,
             result, started_at_unix_ms, finished_at_unix_ms)
         VALUES (?1, ?2, ?3, 'running', ?4, ?5, NULL, ?6, NULL)",
        params![
            run_id,
            workflow.id,
            workflow.name,
            to_json(budget)?,
            to_json(&RunUsage::default())?,
            started_unix_ms
        ],
    )?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Hängt einen Protokolleintrag an.
pub fn append_run_event(
    connection: &mut Connection,
    run_id: &str,
    event: &RunEvent,
) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO workflow_run_events(run_id, seq, at_unix_ms, kind, node_id, message, origin)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            run_id,
            event.seq,
            event.at_unix_ms,
            enum_text(&event.kind)?,
            event.node_id,
            event.message,
            event.origin
        ],
    )?;
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Aktualisiert Verbrauch, Status und (am Ende) Ergebnis eines Laufs.
pub fn update_run(
    connection: &mut Connection,
    run_id: &str,
    status: RunStatus,
    usage: &RunUsage,
    result: Option<&WorkflowResult>,
    finished_unix_ms: Option<i64>,
) -> Result<(), VaultError> {
    let result_json = match result {
        Some(result) => Some(to_json(result)?),
        None => None,
    };
    let transaction = connection.transaction()?;
    let changed = transaction.execute(
        "UPDATE workflow_runs SET status = ?2, usage = ?3, result = ?4, finished_at_unix_ms = ?5
         WHERE id = ?1",
        params![
            run_id,
            enum_text(&status)?,
            to_json(usage)?,
            result_json,
            finished_unix_ms
        ],
    )?;
    if changed == 0 {
        return Err(VaultError::Integrity(format!(
            "Lauf {run_id} existiert nicht"
        )));
    }
    bump_generation(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Markiert beim Start alle noch als laufend geführten Läufe als abgebrochen.
///
/// Warum: Es gibt keine automatische Wiederaufnahme. Ein Lauf, der beim
/// Beenden noch `running` war, wäre sonst für immer „laufend“ (Absturz).
pub fn mark_orphaned_runs_cancelled(
    connection: &mut Connection,
    now_unix_ms: i64,
) -> Result<usize, VaultError> {
    let transaction = connection.transaction()?;
    let changed = transaction.execute(
        "UPDATE workflow_runs SET status = 'cancelled', finished_at_unix_ms = ?1
         WHERE status = 'running'",
        [now_unix_ms],
    )?;
    if changed > 0 {
        bump_generation(&transaction)?;
    }
    transaction.commit()?;
    Ok(changed)
}

/// Bericht eines Laufs samt Protokoll.
pub fn get_run_report(
    connection: &Connection,
    run_id: &str,
) -> Result<Option<WorkflowRunReport>, VaultError> {
    type RunRow = (
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
        Option<i64>,
    );
    let row: Option<RunRow> = connection
        .query_row(
            "SELECT workflow_id, status, budget, usage, result, started_at_unix_ms, finished_at_unix_ms
             FROM workflow_runs WHERE id = ?1",
            [run_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?;
    let Some((workflow_id, status, budget, usage, result, started, finished)) = row else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT seq, at_unix_ms, kind, node_id, message, origin FROM workflow_run_events
         WHERE run_id = ?1 ORDER BY seq ASC",
    )?;
    let rows = statement.query_map([run_id], |row| {
        Ok((
            row.get::<_, u32>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    let mut events = Vec::new();
    for row in rows {
        let (seq, at, kind, node_id, message, origin) = row?;
        events.push(RunEvent {
            seq,
            at_unix_ms: at,
            kind: enum_from_text::<RunEventKind>(&kind)?,
            node_id,
            message,
            origin,
        });
    }
    Ok(Some(WorkflowRunReport {
        run_id: run_id.to_owned(),
        workflow_id,
        status: enum_from_text(&status)?,
        usage: from_json(&usage)?,
        budget: from_json(&budget)?,
        started_unix_ms: started,
        finished_unix_ms: finished,
        current_node: None,
        result: match result {
            Some(text) => Some(from_json(&text)?),
            None => None,
        },
        events,
    }))
}

/// Kurzliste der Läufe eines Workflows (neueste zuerst): `(run_id, status, started)`.
pub fn list_runs(
    connection: &Connection,
    workflow_id: &str,
) -> Result<Vec<(String, RunStatus, i64)>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT id, status, started_at_unix_ms FROM workflow_runs
         WHERE workflow_id = ?1 ORDER BY started_at_unix_ms DESC, id ASC",
    )?;
    let rows = statement.query_map([workflow_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, status, started) = row?;
        out.push((id, enum_from_text(&status)?, started));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_types::flow::{NodeKind, WorkflowNode, WORKFLOW_SCHEMA_VERSION};

    fn open() -> Connection {
        let connection = Connection::open_in_memory().expect("Speicher-DB");
        crate::schema::migrate(&connection).expect("Migration");
        connection
    }

    fn definition(id: &str) -> WorkflowDefinition {
        WorkflowDefinition {
            id: id.to_owned(),
            name: "Recherche".to_owned(),
            graph: WorkflowGraph {
                version: WORKFLOW_SCHEMA_VERSION,
                nodes: vec![WorkflowNode {
                    id: "s".into(),
                    kind: NodeKind::ManualStart,
                    x: 1.0,
                    y: 2.0,
                }],
                edges: vec![],
            },
            updated_unix_ms: 10,
        }
    }

    fn budget() -> RunBudget {
        RunBudget {
            max_searches: 4,
            max_iterations: 2,
            max_seconds: 120,
            max_tokens: 4000,
            max_cost_usd: 0.1,
        }
    }

    #[test]
    fn workflow_definitions_round_trip_and_delete() {
        let mut connection = open();
        save_workflow(&mut connection, &definition("w1")).expect("speichern");
        let mut changed = definition("w1");
        changed.name = "Neu".to_owned();
        changed.updated_unix_ms = 20;
        save_workflow(&mut connection, &changed).expect("überschreiben");
        assert_eq!(
            list_workflows(&connection).expect("Liste"),
            vec![changed.clone()]
        );
        assert_eq!(
            get_workflow(&connection, "w1").expect("Lesen"),
            Some(changed)
        );
        delete_workflow(&mut connection, "w1").expect("Löschen");
        assert!(get_workflow(&connection, "w1").expect("Lesen").is_none());
    }

    #[test]
    fn run_log_keeps_order_usage_and_survives_workflow_deletion() {
        let mut connection = open();
        let workflow = definition("w1");
        save_workflow(&mut connection, &workflow).expect("speichern");
        create_run(&mut connection, "r1", &workflow, &budget(), 100).expect("Lauf");
        for (seq, kind) in [(1, RunEventKind::Started), (2, RunEventKind::NodeStarted)] {
            append_run_event(
                &mut connection,
                "r1",
                &RunEvent {
                    seq,
                    at_unix_ms: 100 + i64::from(seq),
                    kind,
                    node_id: Some("s".into()),
                    message: "m".into(),
                    origin: Some("user_public".into()),
                },
            )
            .expect("Ereignis");
        }
        let usage = RunUsage {
            searches: 1,
            iterations: 0,
            seconds: 1.5,
            tokens: 10,
            cost_usd: 0.007,
        };
        update_run(
            &mut connection,
            "r1",
            RunStatus::BudgetExhausted,
            &usage,
            None,
            Some(200),
        )
        .expect("Ende");
        delete_workflow(&mut connection, "w1").expect("Löschen");

        let report = get_run_report(&connection, "r1")
            .expect("Lesen")
            .expect("vorhanden");
        assert_eq!(report.status, RunStatus::BudgetExhausted);
        assert_eq!(report.usage, usage);
        assert_eq!(
            report.events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(report.finished_unix_ms, Some(200));
        assert_eq!(list_runs(&connection, "w1").expect("Liste").len(), 1);
    }

    #[test]
    fn orphaned_running_runs_are_marked_cancelled_not_resumed() {
        let mut connection = open();
        let workflow = definition("w1");
        create_run(&mut connection, "r1", &workflow, &budget(), 100).expect("Lauf");
        assert_eq!(
            mark_orphaned_runs_cancelled(&mut connection, 300).expect("Markieren"),
            1
        );
        let report = get_run_report(&connection, "r1")
            .expect("Lesen")
            .expect("vorhanden");
        assert_eq!(report.status, RunStatus::Cancelled);
        assert_eq!(
            mark_orphaned_runs_cancelled(&mut connection, 301).expect("Erneut"),
            0
        );
    }

    #[test]
    fn version_five_vault_upgrades_to_six_keeps_data_and_migration_is_repeatable() {
        let mut connection = open();
        connection
            .execute("INSERT INTO conversations(id, title, created_at_unix_ms, updated_at_unix_ms) VALUES ('c1', 'Alt', 1, 2)", [])
            .expect("Unterhaltung");
        // Stand eines Sticks vor dem Update: Version 5 ohne die neuen Tabellen.
        connection
            .execute_batch(
                "DROP TABLE workflow_run_events; DROP TABLE workflow_runs; DROP TABLE workflows;
                 DROP TABLE agent_flow_projects; DROP TABLE agent_flow_tasks;
                 UPDATE schema_version SET version = 5;",
            )
            .expect("Rückbau");
        crate::schema::migrate(&connection).expect("Migration");
        crate::schema::migrate(&connection).expect("wiederholte Migration");
        let version: i64 = connection
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .expect("Version");
        // Migration bringt den alten Stand auf die aktuelle Version (inzwischen 7).
        assert_eq!(version, crate::schema::SCHEMA_VERSION);
        let title: String = connection
            .query_row("SELECT title FROM conversations WHERE id = 'c1'", [], |r| {
                r.get(0)
            })
            .expect("Titel");
        assert_eq!(title, "Alt");
        save_workflow(&mut connection, &definition("w1")).expect("neue Tabellen nutzbar");
    }

    #[test]
    fn updating_unknown_run_is_an_error() {
        let mut connection = open();
        assert!(update_run(
            &mut connection,
            "fehlt",
            RunStatus::Failed,
            &RunUsage::default(),
            None,
            None
        )
        .is_err());
    }
}
