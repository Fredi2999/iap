//! Vollexport in offene Formate (Konzept 15).
//!
//! Ziel: „Alles in offenen Formaten — das ist die Versicherung gegen den
//! Tag, an dem das Projekt nicht mehr weiterentwickelt wird." Das Modul
//! definiert die Verzeichnisstruktur und die Serialisierung; die
//! eigentliche Daten-Beschaffung übernimmt die Tauri-App, weil sie die
//! entsperrten Vault-Handles hält.
//!
//! Layout eines Exports:
//!
//! ```text
//! export-<unix_ms>/
//! ├── README.txt              # Warnhinweis + Inventar
//! ├── conversations/          # ein `.md` je Konversation, plus `all.json`
//! │   ├── all.json
//! │   └── <id>.md
//! ├── memory/
//! │   └── facts.json
//! ├── calendar.ics            # Termine + Aufgaben (via pa-scheduler)
//! ├── skills/                 # kopierte Skill-Ordner
//! ├── workspace/              # unveränderte Nutzerdateien
//! └── audit/audit.jsonl       # Zeilenweise JSON, Hash-Kette bleibt gültig
//! ```

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::Serialize;
use thiserror::Error;

pub use pa_scheduler::export_ics;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("io: {source}")]
    Io {
        #[source]
        source: std::io::Error,
    },
    #[error("serde: {0}")]
    Serde(String),
    #[error("Export-Zielverzeichnis existiert bereits und ist nicht leer: {0}")]
    NotEmpty(PathBuf),
}

impl From<std::io::Error> for ExportError {
    fn from(source: std::io::Error) -> Self {
        ExportError::Io { source }
    }
}

/// Legt ein leeres Export-Verzeichnis mit README und Grundstruktur an.
///
/// `now_unix_ms` speist den Ordnernamen (`export-<ms>`), damit sich zwei
/// Exporte am selben Tag nicht überschreiben.
pub fn create_export_root(base: &Path, now_unix_ms: i64) -> Result<PathBuf, ExportError> {
    let root = base.join(format!("export-{now_unix_ms}"));
    if root.exists() {
        let has_content = fs::read_dir(&root)?.next().is_some();
        if has_content {
            return Err(ExportError::NotEmpty(root));
        }
    } else {
        fs::create_dir_all(&root)?;
    }
    fs::create_dir_all(root.join("conversations"))?;
    fs::create_dir_all(root.join("memory"))?;
    fs::create_dir_all(root.join("skills"))?;
    fs::create_dir_all(root.join("workspace"))?;
    fs::create_dir_all(root.join("audit"))?;
    write_readme(&root, now_unix_ms)?;
    Ok(root)
}

fn write_readme(root: &Path, now_unix_ms: i64) -> Result<(), ExportError> {
    let mut file = fs::File::create(root.join("README.txt"))?;
    writeln!(
        file,
        "PortableAI — Vollexport (Konzept 15).\n\n\
         Erzeugt am (unix ms UTC): {now_unix_ms}\n\n\
         Dieses Verzeichnis enthält ENTSCHLÜSSELTE Kopien deiner Daten in\n\
         offenen Formaten. Bewahre es sicher auf — Zugriff auf diesen Ordner\n\
         ist gleichbedeutend mit Zugriff auf deinen Vault-Inhalt.\n\n\
         Inhalte:\n\
         - conversations/     Unterhaltungen als Markdown, plus all.json\n\
         - memory/facts.json  Aktive Fakten der Memory-Schicht\n\
         - calendar.ics       Termine und Aufgaben (iCalendar)\n\
         - skills/            Installierte WASM-Skills (Manifest + .wasm)\n\
         - workspace/         Unveränderte Nutzerdateien\n\
         - audit/audit.jsonl  Zeilenweise JSON, mit Hash-Kette\n"
    )?;
    Ok(())
}

/// Serialisiert eine Konversation als schlichte Markdown-Datei.
pub fn conversation_markdown<S: AsRef<str>>(
    title: &str,
    messages: impl IntoIterator<Item = (S, S)>,
) -> String {
    let mut buffer = String::new();
    buffer.push_str("# ");
    buffer.push_str(title);
    buffer.push_str("\n\n");
    for (role, content) in messages {
        buffer.push_str("**");
        buffer.push_str(role.as_ref());
        buffer.push_str(":**\n\n");
        buffer.push_str(content.as_ref());
        buffer.push_str("\n\n");
    }
    buffer
}

/// Schreibt einen JSON-Wert in eine Datei mit `{}` als Standard bei `None`.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ExportError> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|error| ExportError::Serde(error.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

/// Hängt eine Zeile an eine JSONL-Datei (für Audit-Log).
pub fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<(), ExportError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line =
        serde_json::to_string(value).map_err(|error| ExportError::Serde(error.to_string()))?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_full_layout() {
        let temp = TempDir::new().unwrap();
        let root = create_export_root(temp.path(), 100).unwrap();
        assert!(root.join("conversations").is_dir());
        assert!(root.join("memory").is_dir());
        assert!(root.join("audit").is_dir());
        assert!(root.join("README.txt").is_file());
    }

    #[test]
    fn conversation_markdown_lists_messages() {
        let md = conversation_markdown("Chat", vec![("user", "Hallo"), ("assistant", "Servus")]);
        assert!(md.contains("**user:**"));
        assert!(md.contains("Servus"));
    }

    #[test]
    fn jsonl_appends_lines() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("audit.jsonl");
        append_jsonl(&path, &serde_json::json!({"a":1})).unwrap();
        append_jsonl(&path, &serde_json::json!({"a":2})).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
    }
}
