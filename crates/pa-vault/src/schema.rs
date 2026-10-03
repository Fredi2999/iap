use rusqlite::Connection;

use crate::VaultError;

/// Aktuelle Schemaversion. Migrationen laufen additiv; bereits vorhandene
/// Vaults werden beim ersten Öffnen automatisch auf diese Version gebracht.
pub const SCHEMA_VERSION: i64 = 7;

/// Legt Basistabellen an und führt Migrationen aus. Fremdschlüssel bleiben an.
///
/// Version 1 (Phase 1): conversations, messages, settings.
/// Version 2 (Phase 2): audit_log als append-only Tabelle mit Hash-Verkettung
/// — Speicherort für den pa-policy-Audit, damit AGENTS-Invariante 6 (keine
/// Zustandsdaten außerhalb des Vaults) auch für den Werkzeugpfad hält.
/// Version 4 (Phase 3, M2): agent_runs speichert den Zustand eines
/// Eskalationslaufs (Konzept 7.1/7.3). Damit überlebt der Agentenbaum eine
/// Session-Rotation. Payload liegt als serialisiertes JSON — der Runner
/// überschreibt nach jeder Rolle den ganzen Datensatz.
/// Version 5: Projekte (`projects`, `conversations.project_id`) und
/// Dokumente für den Chat mit Quellen (`documents`, `conversation_documents`).
/// Version 6: Workflows und Laufprotokolle, Agent-Flow-Aufgaben und
/// freigegebene Host-Projekte (Flow Version).
pub(crate) fn migrate(connection: &Connection) -> Result<(), VaultError> {
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS schema_version (
             version INTEGER NOT NULL
         );
         INSERT INTO schema_version(version)
             SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM schema_version);
         CREATE TABLE IF NOT EXISTS conversations (
             id TEXT PRIMARY KEY NOT NULL,
             title TEXT NOT NULL,
             created_at_unix_ms INTEGER NOT NULL,
             updated_at_unix_ms INTEGER NOT NULL
         ) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS messages (
             id TEXT PRIMARY KEY NOT NULL,
             conversation_id TEXT NOT NULL,
             position INTEGER NOT NULL,
             role TEXT NOT NULL CHECK(role IN ('system', 'user', 'assistant')),
             content TEXT NOT NULL,
             status TEXT NOT NULL CHECK(status IN ('complete', 'streaming', 'aborted')),
             created_at_unix_ms INTEGER NOT NULL,
             UNIQUE(conversation_id, position),
             FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
         ) WITHOUT ROWID;
         CREATE TABLE IF NOT EXISTS settings (
             key TEXT PRIMARY KEY NOT NULL,
             value TEXT NOT NULL
         ) WITHOUT ROWID;",
    )?;

    let mut version: i64 =
        connection.query_row("SELECT version FROM schema_version", [], |row| row.get(0))?;
    if !(1..=SCHEMA_VERSION).contains(&version) {
        return Err(VaultError::Integrity(format!(
            "nicht unterstützte Schemaversion {version}"
        )));
    }

    if version < 2 {
        // Spaltendefinition ist bewusst spiegelbildlich zum pa-policy::AuditStore,
        // damit ein späterer Bridge-AuditStore denselben Datensatz sieht.
        // Keine Fremdschlüssel, weil das Log auch ohne verlinkte Konversation
        // (z. B. reine Werkzeug-Sitzung) einträge braucht.
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_log (
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
             UPDATE schema_version SET version = 2;",
        )?;
        version = 2;
    }

    if version < 3 {
        // Phase-2-Erweiterung: Memory-System nach Konzept 6.2. sqlite-vec ist
        // in unserem Bundle nicht verfügbar; wir speichern Embeddings als
        // BLOB (768*4=3072 Byte little-endian) und suchen per Rust-Cosinus
        // in pa-memory. Die FTS5-Tabelle ist die konzeptvorgegebene
        // lexikalische Suchbasis.
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS facts (
                 id TEXT PRIMARY KEY NOT NULL,
                 text TEXT NOT NULL,
                 category TEXT NOT NULL,
                 confidence REAL NOT NULL,
                 source_message_id TEXT,
                 valid_from_unix_ms INTEGER NOT NULL,
                 valid_until_unix_ms INTEGER,
                 superseded_by TEXT,
                 user_verified INTEGER NOT NULL DEFAULT 0,
                 access_count INTEGER NOT NULL DEFAULT 0,
                 last_accessed_unix_ms INTEGER
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS facts_category ON facts(category);
             CREATE INDEX IF NOT EXISTS facts_superseded ON facts(superseded_by);

             CREATE TABLE IF NOT EXISTS chunks (
                 id TEXT PRIMARY KEY NOT NULL,
                 doc_id TEXT NOT NULL,
                 ordinal INTEGER NOT NULL,
                 text TEXT NOT NULL,
                 heading_path TEXT,
                 tokens INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS chunks_doc ON chunks(doc_id);

             CREATE TABLE IF NOT EXISTS vec_items (
                 item_id TEXT PRIMARY KEY NOT NULL,
                 item_type TEXT NOT NULL,
                 embedding BLOB NOT NULL
             ) WITHOUT ROWID;

             CREATE VIRTUAL TABLE IF NOT EXISTS fts_items USING fts5(
                 text,
                 item_type UNINDEXED,
                 item_id UNINDEXED,
                 tokenize='unicode61'
             );
             UPDATE schema_version SET version = 3;",
        )?;
        version = 3;
    }

    if version < 4 {
        // Phase 3, M2: Persistenter Speicher für Agentenläufe.
        // `payload` trägt den vollständigen `AgentRun` als JSON — der Runner
        // überschreibt den Datensatz nach jeder Rolle (Konzept 7.3). Die
        // ausgelagerten Spalten `stage`, `updated_unix_ms`, `aborted` und
        // `early_stopped` dienen Filter- und Sortierabfragen ohne JSON-Parsing.
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_runs (
                 id TEXT PRIMARY KEY NOT NULL,
                 stage TEXT NOT NULL,
                 created_unix_ms INTEGER NOT NULL,
                 updated_unix_ms INTEGER NOT NULL,
                 aborted INTEGER NOT NULL DEFAULT 0,
                 early_stopped INTEGER NOT NULL DEFAULT 0,
                 payload TEXT NOT NULL
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS agent_runs_updated
                 ON agent_runs(updated_unix_ms DESC);
             UPDATE schema_version SET version = 4;",
        )?;
        version = 4;
    }

    if version < 5 {
        // Feature 5 (Projekte) und Feature 1 (Chat mit eigenen Dokumenten).
        // Beides in einer Migration, damit ein Tresor nur einen Versionssprung
        // erlebt. `project_id` darf NULL sein; beim Löschen eines Projekts
        // bleiben die Unterhaltungen erhalten und verlieren nur die Zuordnung.
        // Dokument-Abschnitte liegen weiter in `chunks` (doc_id = documents.id).
        // Die Spalte wird nur ergänzt, wenn sie fehlt: So bleibt der Schritt
        // wiederholbar, falls ein früherer Versuch vor dem Versionseintrag abbrach.
        if !has_column(connection, "conversations", "project_id")? {
            connection.execute_batch(
                "ALTER TABLE conversations ADD COLUMN project_id TEXT
                     REFERENCES projects(id) ON DELETE SET NULL;",
            )?;
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL,
                 system_prompt TEXT NOT NULL DEFAULT '',
                 created_at_unix_ms INTEGER NOT NULL,
                 updated_at_unix_ms INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS conversations_project ON conversations(project_id);
             CREATE TABLE IF NOT EXISTS documents (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 size_bytes INTEGER NOT NULL,
                 chunk_count INTEGER NOT NULL,
                 created_at_unix_ms INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS conversation_documents (
                 conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                 document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                 PRIMARY KEY (conversation_id, document_id)
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS message_sources (
                 message_id TEXT NOT NULL,
                 number INTEGER NOT NULL,
                 document_id TEXT NOT NULL,
                 document_name TEXT NOT NULL,
                 chunk_ordinal INTEGER NOT NULL,
                 excerpt TEXT NOT NULL,
                 PRIMARY KEY (message_id, number)
             ) WITHOUT ROWID;
             UPDATE schema_version SET version = 5;",
        )?;
        version = 5;
    }

    if version < 6 {
        // Flow Version: Workflows, Laufprotokolle, Agent-Flow-Aufgaben und
        // freigegebene Host-Projekte. Nur neue Tabellen (`IF NOT EXISTS`), also
        // wiederholbar; die Transaktion sorgt dafür, dass bei einem Fehler
        // die Version 5 samt allen Daten unverändert bleibt.
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS workflows (
                 id TEXT PRIMARY KEY NOT NULL,
                 name TEXT NOT NULL,
                 graph TEXT NOT NULL,
                 updated_at_unix_ms INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS workflow_runs (
                 id TEXT PRIMARY KEY NOT NULL,
                 workflow_id TEXT NOT NULL,
                 workflow_name TEXT NOT NULL,
                 status TEXT NOT NULL,
                 budget TEXT NOT NULL,
                 usage TEXT NOT NULL,
                 result TEXT,
                 started_at_unix_ms INTEGER NOT NULL,
                 finished_at_unix_ms INTEGER
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS workflow_runs_workflow ON workflow_runs(workflow_id, started_at_unix_ms);
             CREATE TABLE IF NOT EXISTS workflow_run_events (
                 run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
                 seq INTEGER NOT NULL,
                 at_unix_ms INTEGER NOT NULL,
                 kind TEXT NOT NULL,
                 node_id TEXT,
                 message TEXT NOT NULL,
                 origin TEXT,
                 PRIMARY KEY (run_id, seq)
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS agent_flow_projects (
                 path TEXT PRIMARY KEY NOT NULL,
                 location TEXT NOT NULL,
                 approved_at_unix_ms INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS agent_flow_tasks (
                 id TEXT PRIMARY KEY NOT NULL,
                 project_path TEXT NOT NULL,
                 created_at_unix_ms INTEGER NOT NULL,
                 payload TEXT NOT NULL
             ) WITHOUT ROWID;
             UPDATE schema_version SET version = 6;
             COMMIT;",
        )?;
        version = 6;
    }

    if version < 7 {
        // Version 7: PCI (Personal Computer Information). Aktivität, die der sichtbare
        // Begleiter auf einem Host-PC gesammelt hat, wird beim Einstecken des Sticks hierher
        // importiert und bleibt damit wie alles andere im Tresor. Nur neue Tabellen.
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS pci_imports (
                 id TEXT PRIMARY KEY NOT NULL,
                 host TEXT NOT NULL,
                 from_unix_ms INTEGER NOT NULL,
                 to_unix_ms INTEGER NOT NULL,
                 total_active_ms INTEGER NOT NULL,
                 imported_at_unix_ms INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS pci_imports_host ON pci_imports(host, imported_at_unix_ms);
             CREATE TABLE IF NOT EXISTS pci_app_usage (
                 import_id TEXT NOT NULL REFERENCES pci_imports(id) ON DELETE CASCADE,
                 app TEXT NOT NULL,
                 total_ms INTEGER NOT NULL,
                 focus_count INTEGER NOT NULL,
                 last_seen_unix_ms INTEGER NOT NULL,
                 sample_titles TEXT NOT NULL,
                 PRIMARY KEY (import_id, app)
             ) WITHOUT ROWID;
             UPDATE schema_version SET version = 7;
             COMMIT;",
        )?;
        version = 7;
    }

    if version != SCHEMA_VERSION {
        return Err(VaultError::Integrity(format!(
            "Schemaversion {version} nach Migration ungleich {SCHEMA_VERSION}"
        )));
    }
    Ok(())
}

/// Prüft, ob eine Tabelle eine Spalte hat; nötig, weil `ALTER TABLE ADD COLUMN`
/// in SQLite nicht wiederholbar ist.
fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, VaultError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}
