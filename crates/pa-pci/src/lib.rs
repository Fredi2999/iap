//! PCI (Personal Computer Information): ein sichtbarer, lokaler Aktivitäts-Begleiter.
//!
//! Dieses Crate ist das Herz des Begleitprogramms, das auf einem PC laufen kann, während der
//! IAP-Stick nicht steckt. Es hält fest, **welche Anwendung im Vordergrund ist, deren
//! Fenstertitel (dort steht z. B. die offene Website) und wie lange** – mehr nicht. Es liest
//! **keine Nachrichteninhalte, keine Tastatureingaben und keine Bildschirminhalte** und geht
//! **nie ins Netz**. Die Aufzeichnung ist sichtbar (das Begleitprogramm zeigt ein Fenster) und
//! wird vom Nutzer bewusst gestartet; es versteckt sich nicht.
//!
//! Hier liegt nur die reine, testbare Logik: das Journalformat, das Lesen und Schreiben und die
//! Verdichtung zu einer Übersicht je Anwendung. Die Fenstererfassung steckt in [`collector`]
//! (nur Windows), das Begleitprogramm selbst in `main.rs`.

pub mod collector;
pub mod journal;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Version des Journalformats. Steigt, wenn sich die Zeilenstruktur ändert.
pub const JOURNAL_VERSION: u32 = 1;

/// Längster gespeicherter Fenstertitel in Zeichen. Begrenzt, damit ein übergroßer Titel nichts
/// aufbläht und weniger versehentlich Sensibles in voller Länge mitgeschrieben wird.
pub const MAX_TITLE_CHARS: usize = 160;

/// So viele unterschiedliche Beispieltitel werden je Anwendung in der Übersicht behalten.
pub const MAX_SAMPLE_TITLES: usize = 8;

/// Fehler beim Lesen oder Schreiben des Journals.
#[derive(Debug, Error)]
pub enum PciError {
    #[error("Journal nicht lesbar: {0}")]
    Io(String),
    #[error("Journalzeile unlesbar")]
    BadLine,
}

impl From<std::io::Error> for PciError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Eine Zeile im Journal (eine Zeile = ein JSON-Objekt, JSONL).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    /// Kopf einer Aufzeichnung: welcher PC, wann gestartet, welche Formatversion.
    Session {
        host: String,
        started_unix_ms: i64,
        version: u32,
    },
    /// Ein Vordergrund-Abschnitt: Anwendung, Titel und Dauer.
    Window {
        at_unix_ms: i64,
        /// Name der ausführbaren Datei ohne Pfad, z. B. `chrome.exe`.
        app: String,
        /// Fenstertitel, bereits gekürzt und von Steuerzeichen befreit. Leer, wenn die
        /// Titelerfassung abgeschaltet ist.
        title: String,
        /// Dauer im Vordergrund in Millisekunden.
        ms: i64,
        /// Zwischenstand eines noch laufenden Abschnitts. Er wird fortlaufend gesichert, damit
        /// beim Schließen des Fensters nichts verloren geht, und zählt in der Übersicht nicht
        /// als neuer Wechsel in die Anwendung.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        cont: bool,
    },
}

/// Säubert einen Fenstertitel: Steuerzeichen raus, auf [`MAX_TITLE_CHARS`] gekürzt.
pub fn clean_title(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect()
}

/// Schreibt eine Zeile als JSON plus Zeilenumbruch.
pub fn write_record(out: &mut impl std::io::Write, record: &Record) -> Result<(), PciError> {
    let line = serde_json::to_string(record).map_err(|e| PciError::Io(e.to_string()))?;
    out.write_all(line.as_bytes())?;
    out.write_all(b"\n")?;
    Ok(())
}

/// Liest alle lesbaren Zeilen eines Journals. Kaputte Zeilen werden übersprungen (eine
/// halb geschriebene letzte Zeile darf den Import nie scheitern lassen).
pub fn read_records(text: &str) -> Vec<Record> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Record>(line).ok())
        .collect()
}

/// Nutzung einer einzelnen Anwendung in einem Import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppUsage {
    pub app: String,
    pub total_ms: i64,
    /// Wie oft die Anwendung in den Vordergrund kam.
    pub focus_count: u32,
    pub last_seen_unix_ms: i64,
    /// Einige unterschiedliche Titel als Beleg (z. B. besuchte Seiten), gekappt.
    pub sample_titles: Vec<String>,
}

/// Eine verdichtete Aufzeichnung eines PCs, bereit zum Ablegen im Tresor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PciImport {
    pub host: String,
    pub from_unix_ms: i64,
    pub to_unix_ms: i64,
    pub total_active_ms: i64,
    pub apps: Vec<AppUsage>,
}

/// Verdichtet rohe Journalzeilen zu einer Übersicht je Anwendung, nach aktiver Zeit sortiert.
///
/// `fallback_host` gilt, wenn das Journal keinen Sitzungskopf hat (z. B. eine alte Datei);
/// normalerweise nennt der Sitzungskopf den PC.
pub fn summarize(records: &[Record], fallback_host: &str) -> Option<PciImport> {
    let mut host = fallback_host.to_owned();
    let mut from = i64::MAX;
    let mut to = i64::MIN;
    let mut total = 0_i64;
    // Reihenfolge der ersten Begegnung beibehalten, damit die Ausgabe deterministisch ist.
    let mut order: Vec<String> = Vec::new();
    let mut usage: std::collections::HashMap<String, AppUsage> = std::collections::HashMap::new();

    for record in records {
        match record {
            Record::Session { host: h, .. } if !h.trim().is_empty() => host = h.clone(),
            Record::Session { .. } => {}
            Record::Window {
                at_unix_ms,
                app,
                title,
                ms,
                cont,
            } => {
                let ms = (*ms).max(0);
                from = from.min(*at_unix_ms);
                to = to.max(at_unix_ms + ms);
                total += ms;
                let entry = usage.entry(app.clone()).or_insert_with(|| {
                    order.push(app.clone());
                    AppUsage {
                        app: app.clone(),
                        total_ms: 0,
                        focus_count: 0,
                        last_seen_unix_ms: 0,
                        sample_titles: Vec::new(),
                    }
                });
                entry.total_ms += ms;
                if !*cont {
                    entry.focus_count = entry.focus_count.saturating_add(1);
                }
                entry.last_seen_unix_ms = entry.last_seen_unix_ms.max(at_unix_ms + ms);
                let title = clean_title(title);
                if !title.is_empty()
                    && entry.sample_titles.len() < MAX_SAMPLE_TITLES
                    && !entry.sample_titles.contains(&title)
                {
                    entry.sample_titles.push(title);
                }
            }
        }
    }

    if order.is_empty() {
        return None;
    }
    let mut apps: Vec<AppUsage> = order
        .into_iter()
        .filter_map(|app| usage.remove(&app))
        .collect();
    apps.sort_by(|a, b| b.total_ms.cmp(&a.total_ms).then(a.app.cmp(&b.app)));
    Some(PciImport {
        host,
        from_unix_ms: from,
        to_unix_ms: to,
        total_active_ms: total,
        apps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(at: i64, app: &str, title: &str, ms: i64) -> Record {
        Record::Window {
            at_unix_ms: at,
            app: app.into(),
            title: title.into(),
            ms,
            cont: false,
        }
    }

    #[test]
    fn titles_are_cleaned_and_clipped() {
        assert_eq!(clean_title("  Hallo\tWelt\n "), "Hallo Welt");
        assert_eq!(
            clean_title(&"x".repeat(500)).chars().count(),
            MAX_TITLE_CHARS
        );
        assert_eq!(clean_title(""), "");
    }

    #[test]
    fn records_round_trip_as_jsonl() {
        let records = vec![
            Record::Session {
                host: "PC-A".into(),
                started_unix_ms: 1,
                version: JOURNAL_VERSION,
            },
            win(10, "chrome.exe", "Wikipedia – Rom", 5000),
        ];
        let mut buf = Vec::new();
        for r in &records {
            write_record(&mut buf, r).unwrap();
        }
        let text = String::from_utf8(buf).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert_eq!(read_records(&text), records);
    }

    #[test]
    fn a_half_written_last_line_is_skipped_not_fatal() {
        let text = "{\"kind\":\"window\",\"at_unix_ms\":1,\"app\":\"a.exe\",\"title\":\"x\",\"ms\":10}\n{\"kind\":\"wi";
        let records = read_records(text);
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn summary_groups_by_app_sorted_by_time_with_sample_titles() {
        let records = vec![
            Record::Session {
                host: "BÜRO-PC".into(),
                started_unix_ms: 100,
                version: 1,
            },
            win(100, "chrome.exe", "Wikipedia", 3000),
            win(3100, "code.exe", "main.rs – IAP", 10000),
            win(13100, "chrome.exe", "Wetter Wien", 2000),
            win(15100, "chrome.exe", "Wikipedia", 1000), // doppelter Titel zählt nur einmal
        ];
        let import = summarize(&records, "egal").unwrap();
        assert_eq!(import.host, "BÜRO-PC");
        assert_eq!(import.from_unix_ms, 100);
        assert_eq!(import.to_unix_ms, 16100);
        assert_eq!(import.total_active_ms, 16000);
        // code.exe hat die meiste Zeit und steht vorn.
        assert_eq!(import.apps[0].app, "code.exe");
        assert_eq!(import.apps[0].total_ms, 10000);
        let chrome = &import.apps[1];
        assert_eq!(chrome.app, "chrome.exe");
        assert_eq!(chrome.total_ms, 6000);
        assert_eq!(chrome.focus_count, 3);
        assert_eq!(chrome.sample_titles, vec!["Wikipedia", "Wetter Wien"]);
    }

    #[test]
    fn continuations_add_time_but_not_extra_focus_counts() {
        let cont = |at, ms| Record::Window {
            at_unix_ms: at,
            app: "chrome.exe".into(),
            title: "Seite".into(),
            ms,
            cont: true,
        };
        let records = vec![
            win(0, "chrome.exe", "Seite", 30_000),
            cont(30_000, 30_000),
            cont(60_000, 5_000),
        ];
        let import = summarize(&records, "PC").unwrap();
        assert_eq!(import.total_active_ms, 65_000);
        assert_eq!(import.apps[0].focus_count, 1);
        assert_eq!(import.to_unix_ms, 65_000);
    }

    #[test]
    fn old_journal_lines_without_the_cont_field_still_read() {
        let text =
            "{\"kind\":\"window\",\"at_unix_ms\":1,\"app\":\"a.exe\",\"title\":\"x\",\"ms\":10}\n";
        assert_eq!(read_records(text), vec![win(1, "a.exe", "x", 10)]);
    }

    #[test]
    fn an_empty_journal_summarizes_to_nothing() {
        assert!(summarize(&[], "PC").is_none());
        assert!(summarize(
            &[Record::Session {
                host: "PC".into(),
                started_unix_ms: 1,
                version: 1
            }],
            "PC"
        )
        .is_none());
    }

    #[test]
    fn sample_titles_are_capped() {
        let mut records = vec![Record::Session {
            host: "PC".into(),
            started_unix_ms: 0,
            version: 1,
        }];
        for i in 0..20 {
            records.push(win(i, "chrome.exe", &format!("Seite {i}"), 100));
        }
        let import = summarize(&records, "PC").unwrap();
        assert_eq!(import.apps[0].sample_titles.len(), MAX_SAMPLE_TITLES);
        assert_eq!(import.apps[0].focus_count, 20);
    }
}
