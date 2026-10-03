//! Speicher für PCI (Personal Computer Information): Aktivität, die der sichtbare Begleiter auf
//! einem Host-PC gesammelt hat, liegt nach dem Import hier im Tresor.
//!
//! Die Oberfläche fragt sie „nach PC“ ab: [`list_computers`] nennt die PCs, [`activity_for`]
//! verdichtet alle Importe eines PCs je Anwendung. [`delete_host`] entfernt alles eines PCs.

use rusqlite::{params, Connection};

use pa_types::ipc::{PciActivity, PciAppUsage, PciComputer};

use crate::VaultError;

/// Eine verdichtete Aufzeichnung (entspricht `pa_pci::PciImport`), wie sie importiert wird.
#[derive(Debug, Clone)]
pub struct ImportInput {
    pub host: String,
    pub from_unix_ms: i64,
    pub to_unix_ms: i64,
    pub total_active_ms: i64,
    pub apps: Vec<PciAppUsage>,
}

/// Legt einen Import ab (Kopf plus Nutzung je Anwendung) in einer Transaktion.
pub fn insert_import(
    connection: &mut Connection,
    id: &str,
    imported_at_unix_ms: i64,
    import: &ImportInput,
) -> Result<(), VaultError> {
    let tx = connection.transaction()?;
    tx.execute(
        "INSERT INTO pci_imports(id, host, from_unix_ms, to_unix_ms, total_active_ms, imported_at_unix_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            import.host,
            import.from_unix_ms,
            import.to_unix_ms,
            import.total_active_ms,
            imported_at_unix_ms
        ],
    )?;
    for usage in &import.apps {
        let titles = serde_json::to_string(&usage.sample_titles)
            .map_err(|e| VaultError::Integrity(e.to_string()))?;
        tx.execute(
            "INSERT INTO pci_app_usage(import_id, app, total_ms, focus_count, last_seen_unix_ms, sample_titles)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, usage.app, usage.total_ms, usage.focus_count, usage.last_seen_unix_ms, titles],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Alle PCs mit gespeicherter Aktivität, zuletzt gesehener zuerst.
pub fn list_computers(connection: &Connection) -> Result<Vec<PciComputer>, VaultError> {
    let mut stmt = connection.prepare(
        "SELECT host, COUNT(*), COALESCE(SUM(total_active_ms), 0), COALESCE(MIN(from_unix_ms), 0), COALESCE(MAX(to_unix_ms), 0)
         FROM pci_imports GROUP BY host ORDER BY MAX(to_unix_ms) DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PciComputer {
            host: row.get(0)?,
            import_count: u32::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
            total_active_ms: row.get(2)?,
            first_unix_ms: row.get(3)?,
            last_unix_ms: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Verdichtet alle Importe eines PCs je Anwendung, nach aktiver Zeit sortiert.
pub fn activity_for(connection: &Connection, host: &str) -> Result<PciActivity, VaultError> {
    let (total, first, last): (i64, i64, i64) = connection
        .query_row(
            "SELECT COALESCE(SUM(total_active_ms), 0), COALESCE(MIN(from_unix_ms), 0), COALESCE(MAX(to_unix_ms), 0)
             FROM pci_imports WHERE host = ?1",
            params![host],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap_or((0, 0, 0));

    let mut stmt = connection.prepare(
        "SELECT u.app, SUM(u.total_ms), SUM(u.focus_count), MAX(u.last_seen_unix_ms), GROUP_CONCAT(u.sample_titles, '\u{1}')
         FROM pci_app_usage u JOIN pci_imports i ON i.id = u.import_id
         WHERE i.host = ?1 GROUP BY u.app ORDER BY SUM(u.total_ms) DESC",
    )?;
    let rows = stmt.query_map(params![host], |row| {
        let joined: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
        Ok(PciAppUsage {
            app: row.get(0)?,
            total_ms: row.get(1)?,
            focus_count: u32::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
            last_seen_unix_ms: row.get(3)?,
            sample_titles: merge_titles(&joined),
        })
    })?;
    let apps = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(PciActivity {
        host: host.to_owned(),
        total_active_ms: total,
        first_unix_ms: first,
        last_unix_ms: last,
        apps,
    })
}

/// Führt die je Import als JSON gespeicherten Titellisten zu einer kurzen, doppelfreien Liste
/// zusammen (die SQL-Verkettung trennt die JSON-Stücke mit \u{1}).
fn merge_titles(joined: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for chunk in joined.split('\u{1}') {
        if let Ok(titles) = serde_json::from_str::<Vec<String>>(chunk) {
            for title in titles {
                if !title.is_empty() && !out.contains(&title) {
                    out.push(title);
                    if out.len() >= 12 {
                        return out;
                    }
                }
            }
        }
    }
    out
}

/// Entfernt alle Aufzeichnungen eines PCs. Die zugehörige Nutzung fällt per Fremdschlüssel weg.
pub fn delete_host(connection: &mut Connection, host: &str) -> Result<usize, VaultError> {
    Ok(connection.execute("DELETE FROM pci_imports WHERE host = ?1", params![host])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&c).unwrap();
        let _ = &mut c;
        c
    }

    fn usage(app: &str, ms: i64, titles: &[&str]) -> PciAppUsage {
        PciAppUsage {
            app: app.into(),
            total_ms: ms,
            focus_count: 1,
            last_seen_unix_ms: ms,
            sample_titles: titles.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn imports_group_by_computer_and_aggregate_apps() {
        let mut c = mem();
        insert_import(
            &mut c,
            "i1",
            1000,
            &ImportInput {
                host: "BÜRO".into(),
                from_unix_ms: 0,
                to_unix_ms: 100,
                total_active_ms: 100,
                apps: vec![
                    usage("chrome.exe", 60, &["Wikipedia"]),
                    usage("code.exe", 40, &["main.rs"]),
                ],
            },
        )
        .unwrap();
        insert_import(
            &mut c,
            "i2",
            2000,
            &ImportInput {
                host: "BÜRO".into(),
                from_unix_ms: 100,
                to_unix_ms: 250,
                total_active_ms: 150,
                apps: vec![usage("chrome.exe", 150, &["Wetter", "Wikipedia"])],
            },
        )
        .unwrap();
        insert_import(
            &mut c,
            "i3",
            3000,
            &ImportInput {
                host: "LAPTOP".into(),
                from_unix_ms: 0,
                to_unix_ms: 30,
                total_active_ms: 30,
                apps: vec![usage("slack.exe", 30, &[])],
            },
        )
        .unwrap();

        let computers = list_computers(&c).unwrap();
        assert_eq!(computers.len(), 2);
        let buro = computers.iter().find(|c| c.host == "BÜRO").unwrap();
        assert_eq!(buro.import_count, 2);
        assert_eq!(buro.total_active_ms, 250);

        let activity = activity_for(&c, "BÜRO").unwrap();
        assert_eq!(activity.total_active_ms, 250);
        // chrome hat über beide Importe die meiste Zeit (60 + 150) und steht vorn.
        assert_eq!(activity.apps[0].app, "chrome.exe");
        assert_eq!(activity.apps[0].total_ms, 210);
        assert_eq!(activity.apps[0].focus_count, 2);
        // Titel aus beiden Importen zusammengeführt, doppelte „Wikipedia“ nur einmal.
        assert!(activity.apps[0]
            .sample_titles
            .contains(&"Wikipedia".to_owned()));
        assert!(activity.apps[0]
            .sample_titles
            .contains(&"Wetter".to_owned()));
        assert_eq!(
            activity.apps[0]
                .sample_titles
                .iter()
                .filter(|t| *t == "Wikipedia")
                .count(),
            1
        );
    }

    #[test]
    fn deleting_a_host_removes_its_imports_and_usage() {
        let mut c = mem();
        insert_import(
            &mut c,
            "i1",
            1,
            &ImportInput {
                host: "PC".into(),
                from_unix_ms: 0,
                to_unix_ms: 10,
                total_active_ms: 10,
                apps: vec![usage("a.exe", 10, &["x"])],
            },
        )
        .unwrap();
        assert_eq!(delete_host(&mut c, "PC").unwrap(), 1);
        assert!(list_computers(&c).unwrap().is_empty());
        // Nutzung ist per Fremdschlüssel ebenfalls weg.
        let count: i64 = c
            .query_row("SELECT COUNT(*) FROM pci_app_usage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        // Aktivität eines unbekannten PCs ist leer, kein Fehler.
        assert_eq!(activity_for(&c, "PC").unwrap().apps.len(), 0);
    }

    #[test]
    fn an_old_vault_migrates_to_version_7() {
        let c = Connection::open_in_memory().unwrap();
        // Eine „alte“ Datenbank auf Version 6 simulieren.
        crate::schema::migrate(&c).unwrap();
        let version: i64 = c
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, crate::schema::SCHEMA_VERSION);
    }
}
