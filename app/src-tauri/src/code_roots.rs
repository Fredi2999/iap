//! Arbeitsordner des Code-Bereichs: der Ordner auf dem Stick (Standard) oder ein vom Nutzer
//! ausdrücklich freigegebener Ordner auf dem PC.
//!
//! Warum eine eigene Freigabe: Ein Ordner auf dem Host liegt außerhalb des Vaults und des
//! Sticks. Deshalb wählt ihn der Nutzer selbst, bestätigt die Freigabe einmal, und IAP merkt
//! sie verschlüsselt im Tresor (widerrufbar). Welche Ordner überhaupt zulässig sind, entscheidet
//! `pa_policy::host_root`; jeder Zugriff darin läuft danach wie bisher durch `PathScope`.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use pa_policy::host_root::HostRootRules;
use pa_policy::{Capability, CapabilityAction, Decision};
use serde::Serialize;
use tauri::State;

use crate::{
    lifecycle, lock, require_session, session_workspace, ui_state, AppError, AppResult, AppState,
};

/// Schlüssel der Freigabeliste im Tresor (JSON-Liste kanonischer Pfade).
const APPROVED_KEY: &str = "ui.code_roots";

/// Der gerade gewählte Wurzelordner des Code-Bereichs. `None` heißt: der Arbeitsordner auf dem
/// Stick. Gilt nur für die laufende Sitzung; nach dem nächsten Entsperren beginnt es wieder dort.
#[derive(Debug, Default)]
pub struct CodeRootRuntime {
    active: Mutex<Option<PathBuf>>,
}

impl CodeRootRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Zurück auf den Arbeitsordner des Sticks (neue Sitzung, Widerruf).
    pub fn reset(&self) {
        if let Ok(mut active) = self.active.lock() {
            *active = None;
        }
    }
}

/// Wo der Code-Bereich gerade arbeitet.
#[derive(Debug, Clone)]
pub struct ActiveRoot {
    pub path: PathBuf,
    /// Ein Ordner auf dem PC (nicht der Stick-Arbeitsordner).
    pub host: bool,
}

/// Der aktive Wurzelordner. Nur mit entsperrtem Tresor.
pub fn active_root(state: &AppState) -> AppResult<ActiveRoot> {
    let stick = session_workspace(state)?;
    Ok(match lock(&state.code_root.active)?.clone() {
        Some(path) => ActiveRoot { path, host: true },
        None => ActiveRoot {
            path: stick,
            host: false,
        },
    })
}

/// Zustand für die Oberfläche.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodeRootsStatus {
    /// Aktiver Ordner auf dem PC; `None`, wenn der Arbeitsordner des Sticks aktiv ist.
    pub host_path: Option<String>,
    /// Der Arbeitsordner auf dem Stick.
    pub stick_path: String,
    /// Alle freigegebenen Ordner auf dem PC.
    pub approved: Vec<String>,
}

/// Ergebnis der Vorprüfung eines gewählten Ordners.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodeRootCheck {
    /// Kanonischer Pfad, wie er freigegeben und angezeigt wird.
    pub path: String,
    /// Der Ordner wurde schon einmal freigegeben; die Oberfläche braucht keinen Dialog.
    pub approved: bool,
}

fn rules(state: &AppState) -> AppResult<HostRootRules> {
    let package_root = lock(&state.package_root)?.clone();
    Ok(HostRootRules::from_environment(&[package_root]))
}

fn display(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    text.strip_prefix(r"\\?\")
        .map_or(text.clone(), str::to_owned)
}

/// Liest die Freigabeliste. Ein beschädigter Wert gilt als leere Liste (keine Freigabe).
fn parse_approved(raw: Option<String>) -> Vec<String> {
    raw.and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .unwrap_or_default()
}

fn load_approved(state: &AppState) -> AppResult<Vec<String>> {
    let key = ui_state::storage_key(APPROVED_KEY).map_err(AppError::Invalid)?;
    let session = require_session(state)?;
    let vault = session.vault_runtime.lock()?;
    Ok(parse_approved(vault.repository().setting(&key)?))
}

fn store_approved(state: &AppState, list: &[String]) -> AppResult<()> {
    let key = ui_state::storage_key(APPROVED_KEY).map_err(AppError::Invalid)?;
    let text =
        serde_json::to_string(list).map_err(|error| AppError::Internal(error.to_string()))?;
    let session = require_session(state)?;
    let mut vault = session.vault_runtime.lock()?;
    vault.repository_mut().set_setting(&key, &text)?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

/// Ob die Freigabe schon erteilt wurde; vergleicht ohne Rücksicht auf Groß-/Kleinschreibung unter
/// Windows, weil derselbe Ordner dort verschieden geschrieben werden kann.
fn is_approved(list: &[String], path: &str) -> bool {
    list.iter().any(|entry| {
        if cfg!(windows) {
            entry.eq_ignore_ascii_case(path)
        } else {
            entry == path
        }
    })
}

fn audit(state: &AppState, target: &str, reason: &str) {
    lifecycle::audit_decision(
        state,
        CapabilityAction::FileRead,
        Some(target.to_owned()),
        &Decision::Allow(Capability {
            action: CapabilityAction::FileRead,
            canonical_path: None,
        }),
        reason,
    );
}

fn status(state: &AppState) -> AppResult<CodeRootsStatus> {
    let root = active_root(state)?;
    Ok(CodeRootsStatus {
        host_path: root.host.then(|| display(&root.path)),
        stick_path: display(&session_workspace(state)?),
        approved: load_approved(state)?,
    })
}

/// Wo arbeitet der Code-Bereich gerade, und welche PC-Ordner sind freigegeben.
#[tauri::command]
pub fn code_roots_status(state: State<'_, AppState>) -> AppResult<CodeRootsStatus> {
    status(&state)
}

/// Prüft einen gewählten Ordner gegen die Sperrliste und sagt, ob er schon freigegeben ist.
/// Ändert nichts; die Oberfläche fragt danach gegebenenfalls nach der Freigabe.
#[tauri::command]
pub fn code_root_check(state: State<'_, AppState>, path: String) -> AppResult<CodeRootCheck> {
    let canonical = rules(&state)?
        .validate(Path::new(path.trim()))
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    let shown = display(&canonical);
    let approved = is_approved(&load_approved(&state)?, &shown);
    Ok(CodeRootCheck {
        path: shown,
        approved,
    })
}

/// Macht einen Ordner auf dem PC zum Arbeitsordner. Ist er noch nicht freigegeben, braucht es
/// `confirmed = true` (der Nutzer hat den Freigabedialog bestätigt); die Freigabe wird gemerkt.
#[tauri::command]
pub fn code_root_open_host(
    state: State<'_, AppState>,
    path: String,
    confirmed: bool,
) -> AppResult<CodeRootsStatus> {
    let canonical = rules(&state)?
        .validate(Path::new(path.trim()))
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    let shown = display(&canonical);
    let mut approved = load_approved(&state)?;
    if !is_approved(&approved, &shown) {
        if !confirmed {
            return Err(AppError::Invalid(
                "Freigabe nötig: Bitte bestätige den Ordner.".to_owned(),
            ));
        }
        approved.push(shown.clone());
        store_approved(&state, &approved)?;
        audit(&state, &shown, "Code: PC-Ordner freigegeben");
    }
    *lock(&state.code_root.active)? = Some(canonical);
    // Vorschläge und Verlauf gehören zum bisherigen Ordner.
    state.code_agent.reset();
    audit(&state, &shown, "Code: PC-Ordner geöffnet");
    status(&state)
}

/// Zurück zum Arbeitsordner auf dem Stick.
#[tauri::command]
pub fn code_root_use_stick(state: State<'_, AppState>) -> AppResult<CodeRootsStatus> {
    state.code_root.reset();
    state.code_agent.reset();
    status(&state)
}

/// Nimmt die Freigabe eines Ordners zurück. War er aktiv, arbeitet der Bereich wieder im
/// Stick-Arbeitsordner. Dateien im Ordner werden nicht angefasst.
#[tauri::command]
pub fn code_root_revoke(state: State<'_, AppState>, path: String) -> AppResult<CodeRootsStatus> {
    let mut approved = load_approved(&state)?;
    let before = approved.len();
    approved.retain(|entry| !is_approved(std::slice::from_ref(entry), path.trim()));
    if approved.len() != before {
        store_approved(&state, &approved)?;
        audit(
            &state,
            path.trim(),
            "Code: Freigabe eines PC-Ordners widerrufen",
        );
    }
    let active = active_root(&state)?;
    if active.host && is_approved(&[display(&active.path)], path.trim()) {
        state.code_root.reset();
        state.code_agent.reset();
    }
    status(&state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_damaged_or_missing_approval_list_means_no_approval() {
        assert!(parse_approved(None).is_empty());
        assert!(parse_approved(Some("kein json".to_owned())).is_empty());
        assert!(parse_approved(Some("{\"a\":1}".to_owned())).is_empty());
        assert_eq!(
            parse_approved(Some("[\"C:/a\",\"C:/b\"]".to_owned())),
            vec!["C:/a".to_owned(), "C:/b".to_owned()]
        );
    }

    #[cfg(windows)]
    #[test]
    fn approval_ignores_case_on_windows_only_for_the_same_path() {
        let list = vec![r"C:\Work\Projekt".to_owned()];
        assert!(is_approved(&list, r"c:\work\projekt"));
        assert!(!is_approved(&list, r"C:\Work\Projekt2"));
        // Ein Unterordner einer freigegebenen Wurzel ist keine eigene Freigabe.
        assert!(!is_approved(&list, r"C:\Work\Projekt\src"));
    }

    #[test]
    fn the_verbatim_prefix_is_not_shown() {
        assert_eq!(display(Path::new(r"\\?\C:\Work")), r"C:\Work");
        assert_eq!(display(Path::new("/home/a")), "/home/a");
    }
}
