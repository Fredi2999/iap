//! Das Journal-Verzeichnis auf dem Host: sichere Übernahme beim Import.
//!
//! Der Begleiter hängt Zeilen an `journal.jsonl` an, solange IAP den Import ausführt. Würde
//! der Import die Datei erst lesen und dann leeren, ginge eine Zeile verloren, die dazwischen
//! geschrieben wird. Deshalb wird die Datei zuerst **umbenannt** (der Begleiter legt beim
//! nächsten Schreiben eine neue an), dann gelesen und erst nach erfolgreichem Ablegen im Tresor
//! gelöscht. Schlägt das Ablegen fehl, bleibt die umbenannte Datei liegen und wird beim nächsten
//! Import mit übernommen.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{read_records, summarize, Record};

/// Name der Journaldatei, in die der Begleiter schreibt.
pub const JOURNAL_FILE: &str = "journal.jsonl";

/// Endung einer übernommenen, noch nicht endgültig abgelegten Journaldatei.
const TAKEN_EXTENSION: &str = "import";

/// Journalzeilen, die für den Import aus dem Host-Ordner genommen wurden.
#[derive(Debug)]
pub struct Taken {
    files: Vec<PathBuf>,
    text: String,
}

impl Taken {
    /// Alle lesbaren Zeilen (kaputte werden übersprungen).
    pub fn records(&self) -> Vec<Record> {
        read_records(&self.text)
    }

    /// Löscht die übernommenen Dateien. Erst aufrufen, wenn der Tresor die Daten hat.
    pub fn commit(self) -> io::Result<()> {
        for file in self.files {
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

fn taken_files(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == TAKEN_EXTENSION))
            .collect(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    files.sort();
    Ok(files)
}

fn read_lossy(path: &Path) -> io::Result<String> {
    Ok(String::from_utf8_lossy(&std::fs::read(path)?).into_owned())
}

/// Nimmt das aktuelle Journal samt liegengebliebenen Übernahmen aus dem Ordner.
pub fn take(dir: &Path) -> io::Result<Taken> {
    let live = dir.join(JOURNAL_FILE);
    if live.exists() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        std::fs::rename(
            &live,
            dir.join(format!("journal-{stamp}.{TAKEN_EXTENSION}")),
        )?;
    }
    let files = taken_files(dir)?;
    let mut text = String::new();
    for file in &files {
        text.push_str(&read_lossy(file)?);
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    Ok(Taken { files, text })
}

/// Aktive Zeit, die im Journal auf den Import wartet (0 = nichts zu importieren). Ändert nichts.
pub fn pending_active_ms(dir: &Path) -> i64 {
    let mut text = read_lossy(&dir.join(JOURNAL_FILE)).unwrap_or_default();
    for file in taken_files(dir).unwrap_or_default() {
        text.push('\n');
        text.push_str(&read_lossy(&file).unwrap_or_default());
    }
    summarize(&read_records(&text), "")
        .map(|import| import.total_active_ms)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{write_record, JOURNAL_VERSION};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pa-pci-test-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn append(dir: &Path, record: &Record) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(JOURNAL_FILE))
            .unwrap();
        write_record(&mut file, record).unwrap();
        file.flush().unwrap();
    }

    fn session() -> Record {
        Record::Session {
            host: "PC".into(),
            started_unix_ms: 1,
            version: JOURNAL_VERSION,
        }
    }

    fn window(at: i64, ms: i64) -> Record {
        Record::Window {
            at_unix_ms: at,
            app: "a.exe".into(),
            title: "x".into(),
            ms,
            cont: false,
        }
    }

    #[test]
    fn a_journal_with_only_the_header_has_nothing_pending() {
        // Regression: die Kopfzeile allein machte das Journal „nicht leer“, der Import
        // meldete dann aber „noch keine Aktivität“.
        let dir = temp_dir("header");
        append(&dir, &session());
        assert_eq!(pending_active_ms(&dir), 0);
        append(&dir, &window(0, 7_000));
        assert_eq!(pending_active_ms(&dir), 7_000);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn taking_moves_the_journal_so_new_lines_are_not_lost() {
        let dir = temp_dir("take");
        append(&dir, &session());
        append(&dir, &window(0, 5_000));
        let taken = take(&dir).unwrap();
        // Der Begleiter schreibt weiter, während der Import läuft.
        append(&dir, &window(5_000, 3_000));
        assert_eq!(taken.records().len(), 2);
        taken.commit().unwrap();
        // Die neue Zeile steht in einer frischen Datei und ist beim nächsten Import da.
        assert_eq!(pending_active_ms(&dir), 3_000);
        let next = take(&dir).unwrap();
        assert_eq!(next.records().len(), 1);
        next.commit().unwrap();
        assert_eq!(pending_active_ms(&dir), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_import_keeps_the_data_for_the_next_try() {
        let dir = temp_dir("retry");
        append(&dir, &session());
        append(&dir, &window(0, 5_000));
        let taken = take(&dir).unwrap();
        // Ablegen im Tresor schlägt fehl: kein commit, die Datei bleibt.
        drop(taken);
        assert_eq!(pending_active_ms(&dir), 5_000);
        append(&dir, &window(5_000, 1_000));
        let again = take(&dir).unwrap();
        let import = summarize(&again.records(), "PC").unwrap();
        assert_eq!(import.total_active_ms, 6_000);
        again.commit().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_folder_is_simply_empty() {
        let dir = std::env::temp_dir().join("pa-pci-does-not-exist-xyz");
        assert_eq!(pending_active_ms(&dir), 0);
        let taken = take(&dir).unwrap();
        assert!(taken.records().is_empty());
        taken.commit().unwrap();
    }
}
