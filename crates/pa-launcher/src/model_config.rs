use std::{collections::HashSet, fs, path::Path};

use pa_types::model::{KvQuantization, ModelDescriptor};

use crate::{paths::PackageRoot, LauncherError};

/// Lädt nur paketinterne Deskriptoren und validiert Messwerte vor der Ressourcenplanung.
pub fn load_model_descriptor(
    package: &PackageRoot,
    relative_path: &Path,
) -> Result<ModelDescriptor, LauncherError> {
    let path = package.resolve_existing(relative_path)?;
    let contents = fs::read_to_string(&path).map_err(|source| LauncherError::Io {
        action: "Modellbeschreibung lesen",
        path: path.clone(),
        source,
    })?;
    let descriptor: ModelDescriptor =
        toml::from_str(&contents).map_err(|error| LauncherError::ModelDescriptor {
            path: path.clone(),
            reason: error.to_string(),
        })?;
    validate(&path, &descriptor)?;
    Ok(descriptor)
}

fn validate(path: &Path, descriptor: &ModelDescriptor) -> Result<(), LauncherError> {
    let fail = |reason: &str| LauncherError::ModelDescriptor {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    };
    if descriptor.id.trim().is_empty()
        || descriptor.display_name.trim().is_empty()
        || descriptor.family.trim().is_empty()
        || descriptor.gguf_file.trim().is_empty()
        || descriptor.phase0_source.trim().is_empty()
    {
        return Err(fail("Pflichttextfeld ist leer"));
    }
    if descriptor.sha256.len() != 64 || hex::decode(&descriptor.sha256).is_err() {
        return Err(fail("SHA-256 muss aus genau 64 Hex-Zeichen bestehen"));
    }
    if descriptor.file_bytes == 0
        || descriptor.max_context_tokens == 0
        || descriptor.measured_peak_rss_bytes_8k == 0
        || descriptor.gpu_layer_bytes.is_empty()
        || descriptor.gpu_layer_bytes.contains(&0)
    {
        return Err(fail("numerische Messwerte müssen größer als null sein"));
    }
    let mut quantizations = HashSet::new();
    for profile in &descriptor.kv {
        if profile.bytes_per_token == 0 || profile.source.trim().is_empty() {
            return Err(fail("KV-Messwerte und Quelle müssen gesetzt sein"));
        }
        if !quantizations.insert(profile.quantization) {
            return Err(fail("KV-Quantisierung ist doppelt vorhanden"));
        }
    }
    for required in [KvQuantization::F16, KvQuantization::Q8_0] {
        if !quantizations.contains(&required) {
            return Err(fail("f16- und q8_0-KV-Profile sind beide erforderlich"));
        }
    }
    Ok(())
}
