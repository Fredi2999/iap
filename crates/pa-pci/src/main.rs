//! IAP PCI: sichtbarer Aktivitäts-Begleiter für einen PC, während der Stick draußen ist.
//!
//! Das Programm zeigt beim Start klar, was es tut, und läuft in einem sichtbaren Fenster. Es hält
//! nur fest, welche Anwendung im Vordergrund ist, deren Fenstertitel und die Dauer, und schreibt
//! das in ein lokales Journal. Es liest keine Nachrichten, keine Tastatureingaben und keinen
//! Bildschirm und verbindet sich mit keinem Netz. Beenden: dieses Fenster schließen.

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pa_pci::collector::{idle_ms, sample_foreground, Tracker, IDLE_THRESHOLD_MS};
use pa_pci::journal::JOURNAL_FILE;
use pa_pci::{write_record, Record, JOURNAL_VERSION};

/// Abstand zwischen zwei Stichproben.
const TICK_MS: u64 = 5_000;

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unbekannter-PC".to_owned())
}

/// Ordner des Journals. Klar benannt, nicht versteckt.
fn journal_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("IAPPCI"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|base| PathBuf::from(base).join(".iap-pci"))
    }
}

fn append(path: &std::path::Path, record: &Record) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if let Err(error) = write_record(&mut file, record) {
        return Err(std::io::Error::other(error.to_string()));
    }
    file.flush()
}

fn main() {
    // Titel mitschreiben? Standard an; mit --no-titles nur Anwendungsnamen.
    let record_titles = !std::env::args().any(|a| a == "--no-titles");
    let host = host_name();

    println!("────────────────────────────────────────────────────────");
    println!(" IAP PCI – Aktivitäts-Begleiter (sichtbar, lokal)");
    println!("────────────────────────────────────────────────────────");
    println!(" PC: {host}");
    println!(
        " Es wird aufgezeichnet: aktive Anwendung{} und Dauer.",
        if record_titles { " + Fenstertitel" } else { "" }
    );
    println!(" NICHT aufgezeichnet: Nachrichten, Tastatureingaben, Bildschirm.");
    println!(" Kein Netzwerk. Daten bleiben lokal, bis du den Stick einsteckst.");
    println!(" Der Zwischenstand wird alle 30 Sekunden gesichert.");
    println!(" Zum Beenden: dieses Fenster schließen.");
    println!("────────────────────────────────────────────────────────");

    if cfg!(not(windows)) {
        println!("Hinweis: Die Fenstererfassung gibt es derzeit nur unter Windows.");
        return;
    }

    let Some(dir) = journal_dir() else {
        eprintln!("Fehler: Journal-Ordner nicht bestimmbar (LOCALAPPDATA fehlt).");
        return;
    };
    if let Err(error) = create_dir_all(&dir) {
        eprintln!("Fehler: Journal-Ordner nicht anlegbar: {error}");
        return;
    }
    let journal = dir.join(JOURNAL_FILE);

    if let Err(error) = append(
        &journal,
        &Record::Session {
            host: host.clone(),
            started_unix_ms: now_unix_ms(),
            version: JOURNAL_VERSION,
        },
    ) {
        eprintln!("Fehler: Journal nicht schreibbar: {error}");
        return;
    }

    let mut tracker = Tracker::new(record_titles);
    let mut was_idle = false;
    loop {
        let now = now_unix_ms();
        let foreground = sample_foreground();
        let idle = idle_ms();
        // Sichtbar machen, warum nichts mitgeschrieben wird: bei Leerlauf pausiert die Aufzeichnung.
        let is_idle = idle >= IDLE_THRESHOLD_MS;
        if is_idle != was_idle {
            println!(
                "{}",
                if is_idle {
                    "Leerlauf erkannt – Aufzeichnung pausiert, bis du den PC wieder benutzt."
                } else {
                    "PC wird wieder benutzt – Aufzeichnung läuft."
                }
            );
            was_idle = is_idle;
        }
        if let Some(record) = tracker.observe(now, foreground.as_ref(), idle) {
            match append(&journal, &record) {
                Ok(()) => {
                    if let Record::Window { app, ms, .. } = &record {
                        println!("Gesichert: {app} ({} s)", ms / 1000);
                    }
                }
                Err(error) => eprintln!("Journal-Schreibfehler (wird übersprungen): {error}"),
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(TICK_MS));
    }
}
