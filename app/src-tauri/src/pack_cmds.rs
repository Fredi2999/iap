//! Befehle und Hilfen für die optionalen Zusatzpakete.
//!
//! Ein Paket gilt als nutzbar, wenn es installiert ist, vom Nutzer nicht
//! abgeschaltet wurde und seine Prüfsummen in dieser Sitzung stimmen. Das
//! Abschalten wird im Tresor je Paket gemerkt (`pack.<id>.enabled`).

use pa_types::voice::PackStatus;
use tauri::State;

use crate::{
    lock,
    packs::{self, Pack, PACK_GIT, PACK_PIPER, PACK_VISION, PACK_WHISPER},
    require_session, AppError, AppResult, AppState,
};

/// Bekannte Pakete mit Anzeigenamen für den Fall, dass sie fehlen.
const KNOWN: [(&str, &str); 4] = [
    (PACK_WHISPER, "Spracherkennung (whisper.cpp)"),
    (PACK_PIPER, "Sprachausgabe (Piper)"),
    (PACK_VISION, "Bildverständnis (Projektor)"),
    (PACK_GIT, "Git für Agent Flow (MinGit)"),
];

fn setting_key(id: &str) -> String {
    format!("pack.{id}.enabled")
}

/// Ordner `AI/packs` der aktuellen Paketwurzel.
pub fn packs_dir(state: &AppState) -> AppResult<std::path::PathBuf> {
    let root = lock(&state.package_root)?.clone();
    Ok(packs::packs_root(&root))
}

/// Ob das Paket eingeschaltet ist. Ohne offenen Tresor gilt der Standard „an“.
pub fn pack_enabled(state: &AppState, id: &str) -> bool {
    let Ok(session) = require_session(state) else {
        return true;
    };
    let Ok(vault) = session.vault_runtime.lock() else {
        return true;
    };
    vault
        .repository()
        .setting(&setting_key(id))
        .ok()
        .flatten()
        .as_deref()
        != Some("false")
}

/// Öffnet ein nutzbares Paket: installiert, eingeschaltet, Prüfsummen einmal je
/// Sitzung geprüft. Der Fehlertext ist für die Oberfläche gedacht.
pub fn open_usable(state: &AppState, id: &str) -> AppResult<Pack> {
    if !pack_enabled(state, id) {
        return Err(AppError::Invalid(format!(
            "Das Paket `{id}` ist ausgeschaltet."
        )));
    }
    let pack = Pack::open(&packs_dir(state)?, id).map_err(|e| AppError::Invalid(e.to_string()))?;
    let already_verified = lock(&state.flow.verified_packs)?.contains(id);
    if already_verified {
        pack.check_sizes()
            .map_err(|e| AppError::Invalid(e.to_string()))?;
    } else {
        pack.verify_hashes()
            .map_err(|e| AppError::Invalid(e.to_string()))?;
        lock(&state.flow.verified_packs)?.insert(id.to_owned());
    }
    Ok(pack)
}

/// Status aller Pakete (schnell: nur Größen).
pub fn all_status(state: &AppState) -> AppResult<Vec<PackStatus>> {
    let dir = packs_dir(state)?;
    Ok(KNOWN
        .iter()
        .map(|(id, name)| packs::status(&dir, id, name, pack_enabled(state, id)))
        .collect())
}

/// Liefert den Status der Zusatzpakete.
#[tauri::command]
pub fn list_packs(state: State<'_, AppState>) -> AppResult<Vec<PackStatus>> {
    all_status(&state)
}

/// Schaltet ein Paket ein oder aus (wird im Tresor gemerkt).
#[tauri::command]
pub fn set_pack_enabled(
    state: State<'_, AppState>,
    pack_id: String,
    enabled: bool,
) -> AppResult<()> {
    if !KNOWN.iter().any(|(id, _)| *id == pack_id) {
        return Err(AppError::Invalid("Unbekanntes Paket".to_owned()));
    }
    let session = require_session(&state)?;
    let mut vault = session.vault_runtime.lock()?;
    vault.repository_mut().set_setting(
        &setting_key(&pack_id),
        if enabled { "true" } else { "false" },
    )?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

/// Prüft die Prüfsummen eines Pakets vollständig (dauert bei großen Paketen).
#[tauri::command(async)]
pub fn verify_pack(state: State<'_, AppState>, pack_id: String) -> AppResult<()> {
    let pack =
        Pack::open(&packs_dir(&state)?, &pack_id).map_err(|e| AppError::Invalid(e.to_string()))?;
    pack.verify_hashes()
        .map_err(|e| AppError::Invalid(e.to_string()))?;
    lock(&state.flow.verified_packs)?.insert(pack_id);
    Ok(())
}
