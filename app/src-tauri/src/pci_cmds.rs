//! PCI (Personal Computer Information): der sichtbare Aktivitäts-Begleiter.
//!
//! Der Begleiter (`iap-pci.exe`) läuft auf einem PC, während der Stick draußen ist, und
//! schreibt ein lokales Journal: welche Anwendung im Vordergrund war, deren Fenstertitel und wie
//! lange. Keine Nachrichten, keine Tastatureingaben, kein Bildschirm, kein Netz. Beim Einstecken
//! des Sticks liest IAP das Journal und legt es als PCI im Tresor ab, abrufbar nach PC.
//!
//! Die Host-seitigen Schritte (installieren, starten, stoppen, entfernen) gibt es nur unter
//! Windows und werden im Audit-Log festgehalten. Dass hier bewusst Daten außerhalb des Tresors
//! auf dem Host liegen, ist die in `AGENTS.md` und Konzept 10.6 dokumentierte Ausnahme zu
//! Invariante 6.

use std::path::PathBuf;

use pa_types::ipc::{PciActivity, PciComputer, PciStatus};
use pa_vault::pci::ImportInput;
use tauri::State;

use crate::{lifecycle, now_unix_ms, random_id, require_session, AppError, AppResult, AppState};

/// Name des Begleitprogramms.
const COLLECTOR_EXE: &str = "iap-pci.exe";

fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unbekannter-PC".to_owned())
}

/// Der klar benannte Ordner des Begleiters auf dem Host (nur Windows sinnvoll).
fn host_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("IAPPCI"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|base| PathBuf::from(base).join(".iap-pci"))
    }
}

fn collector_path() -> Option<PathBuf> {
    host_dir().map(|dir| dir.join(COLLECTOR_EXE))
}

/// Ob der Begleiter gerade läuft (ein sichtbarer Prozess dieses Namens).
fn collector_running() -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system.processes().values().any(|p| {
        p.name()
            .to_string_lossy()
            .eq_ignore_ascii_case(COLLECTOR_EXE)
    })
}

fn audit(state: &AppState, target: String, reason: &str) {
    lifecycle::audit_decision(
        state,
        pa_policy::CapabilityAction::FileWrite,
        Some(target),
        &pa_policy::Decision::Allow(pa_policy::Capability {
            action: pa_policy::CapabilityAction::FileWrite,
            canonical_path: None,
        }),
        reason,
    );
}

/// Zustand für die Oberfläche.
#[tauri::command]
pub fn pci_status() -> AppResult<PciStatus> {
    let installed = collector_path().is_some_and(|p| p.exists());
    // Nur echte Aktivität zählt: die Kopfzeile allein ist kein Grund zum Importieren.
    let journal_active_ms = host_dir().map_or(0, |dir| pa_pci::journal::pending_active_ms(&dir));
    Ok(PciStatus {
        supported: cfg!(windows),
        installed,
        running: installed && collector_running(),
        journal_pending: journal_active_ms > 0,
        journal_active_ms,
        host: host_name(),
    })
}

/// Nimmt das Journal aus `dir`, verdichtet es und übergibt es an `store` (das es im Tresor
/// ablegt). Die Journaldateien werden erst gelöscht, wenn `store` Erfolg meldet; scheitert es,
/// bleiben sie für den nächsten Versuch liegen. Gibt den Namen des PCs zurück.
fn import_from_dir(
    dir: &std::path::Path,
    fallback_host: &str,
    store: impl FnOnce(&ImportInput) -> AppResult<()>,
) -> AppResult<String> {
    let taken = pa_pci::journal::take(dir)
        .map_err(|e| AppError::Invalid(format!("Journal nicht lesbar: {e}")))?;
    let Some(summary) = pa_pci::summarize(&taken.records(), fallback_host) else {
        // Nur die Kopfzeile oder gar nichts: kein Grund, leere Dateien zu behalten.
        let _ = taken.commit();
        return Err(AppError::Invalid(
            "Noch keine Aktivität aufgezeichnet. Der Begleiter sichert alle 30 Sekunden; \
             lass ihn etwas länger laufen und versuche es dann erneut."
                .to_owned(),
        ));
    };
    let input = ImportInput {
        host: summary.host.clone(),
        from_unix_ms: summary.from_unix_ms,
        to_unix_ms: summary.to_unix_ms,
        total_active_ms: summary.total_active_ms,
        apps: summary
            .apps
            .into_iter()
            .map(|a| pa_types::ipc::PciAppUsage {
                app: a.app,
                total_ms: a.total_ms,
                focus_count: a.focus_count,
                last_seen_unix_ms: a.last_seen_unix_ms,
                sample_titles: a.sample_titles,
            })
            .collect(),
    };
    store(&input)?;
    taken
        .commit()
        .map_err(|e| AppError::Invalid(format!("Journal nicht aufräumbar: {e}")))?;
    Ok(input.host)
}

/// Importiert das Journal des Hosts in den Tresor. Der Begleiter darf dabei weiterlaufen.
#[tauri::command]
pub fn pci_import(state: State<'_, AppState>) -> AppResult<PciActivity> {
    let Some(dir) = host_dir() else {
        return Err(AppError::Invalid(
            "Kein Journal-Ordner auf diesem System.".to_owned(),
        ));
    };
    let host = import_from_dir(&dir, &host_name(), |input| {
        let session = require_session(&state)?;
        let mut vault = session.vault_runtime.lock()?;
        pa_vault::pci::insert_import(
            vault.repository_mut().connection_mut(),
            &random_id("pci"),
            now_unix_ms(),
            input,
        )?;
        let mut no_fault = pa_vault::hot_copy::NoFault;
        let _ = vault.sync(&mut no_fault);
        Ok(())
    })?;
    audit(&state, host.clone(), "PCI-Journal importiert");
    pci_activity(state, host)
}

/// Alle PCs mit gespeicherter Aktivität.
#[tauri::command]
pub fn pci_list_computers(state: State<'_, AppState>) -> AppResult<Vec<PciComputer>> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(pa_vault::pci::list_computers(
        vault.repository().connection(),
    )?)
}

/// Aktivität eines PCs, je Anwendung verdichtet.
#[tauri::command]
pub fn pci_activity(state: State<'_, AppState>, host: String) -> AppResult<PciActivity> {
    let session = require_session(&state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(pa_vault::pci::activity_for(
        vault.repository().connection(),
        &host,
    )?)
}

/// Löscht die gespeicherte Aktivität eines PCs aus dem Tresor.
#[tauri::command]
pub fn pci_delete_host(state: State<'_, AppState>, host: String) -> AppResult<()> {
    {
        let session = require_session(&state)?;
        let mut vault = session.vault_runtime.lock()?;
        pa_vault::pci::delete_host(vault.repository_mut().connection_mut(), &host)?;
        let mut no_fault = pa_vault::hot_copy::NoFault;
        let _ = vault.sync(&mut no_fault);
    }
    audit(&state, host, "PCI eines PCs gelöscht");
    Ok(())
}

/// Installiert den Begleiter auf dem Host (kopiert die Datei vom Stick) und startet ihn sichtbar.
#[tauri::command]
pub fn pci_install_and_start(
    state: State<'_, AppState>,
    record_titles: bool,
) -> AppResult<PciStatus> {
    if !cfg!(windows) {
        return Err(AppError::Invalid(
            "Der PCI-Begleiter läuft derzeit nur unter Windows.".to_owned(),
        ));
    }
    let (Some(dir), Some(target)) = (host_dir(), collector_path()) else {
        return Err(AppError::Invalid(
            "Kein Zielordner auf diesem System.".to_owned(),
        ));
    };
    let source = {
        let root = crate::lock(&state.package_root)?.clone();
        root.join("AI")
            .join("bin")
            .join("win-x64")
            .join(COLLECTOR_EXE)
    };
    if !source.exists() {
        return Err(AppError::Invalid(
            "Das Begleitprogramm ist nicht im Paket. Bitte mit dem aktuellen Bundle neu bauen."
                .to_owned(),
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Invalid(e.to_string()))?;
    std::fs::copy(&source, &target).map_err(|e| AppError::Invalid(e.to_string()))?;
    audit(
        &state,
        target.display().to_string(),
        "PCI-Begleiter installiert",
    );
    start_collector(&target, record_titles)?;
    pci_status()
}

/// Startet den bereits installierten Begleiter (ohne neu zu kopieren).
#[tauri::command]
pub fn pci_start(state: State<'_, AppState>, record_titles: bool) -> AppResult<PciStatus> {
    let Some(target) = collector_path().filter(|p| p.exists()) else {
        return Err(AppError::Invalid(
            "Der Begleiter ist nicht installiert.".to_owned(),
        ));
    };
    if collector_running() {
        return pci_status();
    }
    refresh_collector_copy(&state, &target);
    start_collector(&target, record_titles)?;
    pci_status()
}

/// Ersetzt die installierte Kopie durch die Fassung auf dem Stick. Sonst liefe nach einem
/// IAP-Update weiter der alte Begleiter (etwa einer ohne Zwischenstände). Gelingt das nicht,
/// läuft die vorhandene Kopie einfach weiter.
fn refresh_collector_copy(state: &AppState, target: &std::path::Path) {
    let Ok(root) = crate::lock(&state.package_root).map(|r| r.clone()) else {
        return;
    };
    let source = root
        .join("AI")
        .join("bin")
        .join("win-x64")
        .join(COLLECTOR_EXE);
    if source.exists() && std::fs::copy(&source, target).is_ok() {
        audit(
            state,
            target.display().to_string(),
            "PCI-Begleiter aktualisiert",
        );
    }
}

#[cfg(windows)]
fn start_collector(path: &std::path::Path, record_titles: bool) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    // CREATE_NEW_CONSOLE: der Begleiter bekommt ein eigenes, sichtbares Fenster.
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let mut command = std::process::Command::new(path);
    if !record_titles {
        command.arg("--no-titles");
    }
    command
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map_err(|e| AppError::Invalid(format!("Begleiter nicht startbar: {e}")))?;
    Ok(())
}

#[cfg(not(windows))]
fn start_collector(_path: &std::path::Path, _record_titles: bool) -> AppResult<()> {
    Err(AppError::Invalid(
        "Der PCI-Begleiter läuft derzeit nur unter Windows.".to_owned(),
    ))
}

/// Stoppt den laufenden Begleiter.
#[tauri::command]
pub fn pci_stop() -> AppResult<PciStatus> {
    stop_collector();
    pci_status()
}

fn stop_collector() {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    for process in system.processes().values() {
        if process
            .name()
            .to_string_lossy()
            .eq_ignore_ascii_case(COLLECTOR_EXE)
        {
            process.kill();
        }
    }
}

/// Entfernt den Begleiter vom PC: stoppt ihn und löscht Programm und Journal vom Host.
/// Bereits in den Tresor importierte PCI bleibt erhalten (das ist dein Gedächtnis).
#[tauri::command]
pub fn pci_remove_collector(state: State<'_, AppState>) -> AppResult<PciStatus> {
    stop_collector();
    // Dem Prozess einen Moment zum Beenden geben, bevor die Datei gelöscht wird.
    std::thread::sleep(std::time::Duration::from_millis(400));
    if let Some(dir) = host_dir() {
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| AppError::Invalid(e.to_string()))?;
        }
        audit(
            &state,
            dir.display().to_string(),
            "PCI-Begleiter vom PC entfernt",
        );
    }
    pci_status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn dir(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new().prefix(tag).tempdir().unwrap()
    }

    fn write_journal(dir: &std::path::Path, lines: &[&str]) {
        let mut file = std::fs::File::create(dir.join(pa_pci::journal::JOURNAL_FILE)).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }

    const HEADER: &str = r#"{"kind":"session","host":"BUERO","started_unix_ms":1,"version":1}"#;
    const WINDOW: &str =
        r#"{"kind":"window","at_unix_ms":10,"app":"chrome.exe","title":"Wetter","ms":9000}"#;

    #[test]
    fn a_journal_with_activity_is_stored_and_removed() {
        let d = dir("pci-ok");
        write_journal(d.path(), &[HEADER, WINDOW]);
        let mut stored = None;
        let host = import_from_dir(d.path(), "egal", |input| {
            stored = Some(input.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(host, "BUERO");
        let input = stored.unwrap();
        assert_eq!(input.total_active_ms, 9000);
        assert_eq!(input.apps[0].app, "chrome.exe");
        assert_eq!(pa_pci::journal::pending_active_ms(d.path()), 0);
    }

    #[test]
    fn a_header_only_journal_gives_a_clear_message_instead_of_a_silent_failure() {
        let d = dir("pci-header");
        write_journal(d.path(), &[HEADER]);
        let error = import_from_dir(d.path(), "egal", |_| panic!("nichts zu speichern"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("Noch keine Aktivität"), "{error}");
    }

    #[test]
    fn a_failing_vault_keeps_the_journal_for_the_next_import() {
        let d = dir("pci-fail");
        write_journal(d.path(), &[HEADER, WINDOW]);
        let failed = import_from_dir(d.path(), "egal", |_| Err(AppError::locked()));
        assert!(failed.is_err());
        assert_eq!(pa_pci::journal::pending_active_ms(d.path()), 9000);
        // Zweiter Versuch gelingt und holt die liegengebliebene Aktivität.
        let host = import_from_dir(d.path(), "egal", |_| Ok(())).unwrap();
        assert_eq!(host, "BUERO");
        assert_eq!(pa_pci::journal::pending_active_ms(d.path()), 0);
    }

    #[test]
    fn a_missing_journal_folder_is_reported_not_crashed() {
        let d = dir("pci-none");
        let gone = d.path().join("nicht-da");
        assert!(import_from_dir(&gone, "egal", |_| Ok(())).is_err());
    }
}
