use std::{fs, io::Write, path::Path};

use pa_types::hardware::HardwareProfile;
use sha2::{Digest, Sha256};

use crate::{atomic_file, LauncherError};

/// Erzeugt eine stabile, nicht geheime Hostkennung ohne den schwankenden freien RAM.
pub fn host_id(hostname: &str, profile: &HardwareProfile) -> String {
    let stable = serde_json::json!({
        "hostname": hostname,
        "cpu_model": profile.cpu_model,
        "physical_cores": profile.physical_cores,
        "logical_processors": profile.logical_processors,
        "total_ram_bytes": profile.total_ram_bytes,
        "operating_system": profile.operating_system,
        "instruction_sets": profile.instruction_sets,
        "gpus": profile.gpus,
        "graphics_apis": profile.graphics_apis,
    });
    hex::encode(Sha256::digest(stable.to_string().as_bytes()))
}

/// Schreibt Hostmetadaten über eine Flush-und-Rename-Strecke, damit Abbrüche kein halbes JSON lassen.
pub fn save_host_profile(
    cache_root: &Path,
    id: &str,
    profile: &HardwareProfile,
) -> Result<(), LauncherError> {
    validate_id(id)?;
    let directory = cache_root.join("hosts");
    fs::create_dir_all(&directory).map_err(|source| LauncherError::Io {
        action: "Hostprofil-Verzeichnis anlegen",
        path: directory.clone(),
        source,
    })?;
    let destination = directory.join(format!("{id}.json"));
    let partial = directory.join(format!("{id}.json.partial"));
    let mut file = fs::File::create(&partial).map_err(|source| LauncherError::Io {
        action: "partielles Hostprofil anlegen",
        path: partial.clone(),
        source,
    })?;
    serde_json::to_writer_pretty(&mut file, profile)
        .map_err(|source| LauncherError::Cache(format!("Hostprofil serialisieren: {source}")))?;
    file.write_all(b"\n").map_err(|source| LauncherError::Io {
        action: "Hostprofil schreiben",
        path: partial.clone(),
        source,
    })?;
    file.sync_all().map_err(|source| LauncherError::Io {
        action: "Hostprofil synchronisieren",
        path: partial.clone(),
        source,
    })?;
    atomic_file::replace(&partial, &destination)?;
    Ok(())
}

/// Nutzt gespeicherte stabile Fakten, ersetzt aber freien RAM stets durch die aktuelle Messung.
pub fn load_current_host_profile(
    cache_root: &Path,
    id: &str,
    current: &HardwareProfile,
) -> Result<Option<HardwareProfile>, LauncherError> {
    validate_id(id)?;
    let path = cache_root.join("hosts").join(format!("{id}.json"));
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(LauncherError::Io {
                action: "Hostprofil lesen",
                path,
                source,
            })
        }
    };
    let mut saved: HardwareProfile = serde_json::from_slice(&bytes)
        .map_err(|source| LauncherError::Cache(format!("Hostprofil JSON: {source}")))?;
    saved.available_ram_bytes = current.available_ram_bytes;
    saved.process_elevated = current.process_elevated.clone();
    Ok(Some(saved))
}

fn validate_id(id: &str) -> Result<(), LauncherError> {
    if !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Ok(())
    } else {
        Err(LauncherError::Cache("ungültige Host-ID".to_owned()))
    }
}
