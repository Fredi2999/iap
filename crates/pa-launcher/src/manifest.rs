use std::{
    collections::HashSet,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{paths::PackageRoot, LauncherError};

const HASH_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    files: Vec<ManifestEntry>,
    /// Optionaler Verweis auf das Embedding-Modell (Konzept, Meilenstein 10).
    ///
    /// Wenn gesetzt, weiß der Bootstrap, dass ein zweiter `llama-server` mit
    /// `--embedding` gestartet werden soll. Fehlt das Feld, bleibt der
    /// `HashingEmbedder` als Fallback aktiv (heutiger Zustand).
    #[serde(default)]
    embedding_model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    path: String,
    bytes: u64,
    sha256: String,
}

/// Fasst nur tatsächlich verifizierte Bytes zusammen und dient später der Startanzeige.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub version: String,
    pub verified_files: usize,
    pub verified_bytes: u64,
    pub embedding_model_id: Option<String>,
}

/// Prüft jede ausgelieferte Datei, bevor ausführbarer Code oder Modelle verwendet werden.
pub fn verify_manifest(
    root: &PackageRoot,
    manifest_path: &Path,
) -> Result<VerificationReport, LauncherError> {
    let resolved_manifest = root.resolve_existing(manifest_path)?;
    let bytes = std::fs::read(&resolved_manifest).map_err(|source| LauncherError::Io {
        action: "Manifest lesen",
        path: resolved_manifest.clone(),
        source,
    })?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|source| LauncherError::ManifestJson {
            path: resolved_manifest,
            source,
        })?;
    validate_manifest_shape(&manifest)?;

    let mut verified_bytes = 0_u64;
    for entry in &manifest.files {
        let path = root.resolve_existing(Path::new(&entry.path))?;
        let actual_bytes = path
            .metadata()
            .map_err(|source| LauncherError::Io {
                action: "Dateigröße lesen",
                path: path.clone(),
                source,
            })?
            .len();
        if actual_bytes != entry.bytes {
            return Err(LauncherError::SizeMismatch {
                path,
                expected: entry.bytes,
                actual: actual_bytes,
            });
        }
        let actual_hash = hash_file(&path)?;
        if !actual_hash.eq_ignore_ascii_case(&entry.sha256) {
            return Err(LauncherError::HashMismatch {
                path,
                expected: entry.sha256.to_ascii_lowercase(),
                actual: actual_hash,
            });
        }
        verified_bytes = verified_bytes.saturating_add(actual_bytes);
    }

    Ok(VerificationReport {
        version: manifest.version,
        verified_files: manifest.files.len(),
        verified_bytes,
        embedding_model_id: manifest.embedding_model_id,
    })
}

fn validate_manifest_shape(manifest: &Manifest) -> Result<(), LauncherError> {
    if manifest.version.trim().is_empty() {
        return Err(LauncherError::InvalidManifest(
            "Version darf nicht leer sein".to_owned(),
        ));
    }
    if manifest.files.is_empty() {
        return Err(LauncherError::InvalidManifest(
            "mindestens eine Datei ist erforderlich".to_owned(),
        ));
    }
    let mut paths = HashSet::new();
    for entry in &manifest.files {
        if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(LauncherError::InvalidManifest(format!(
                "SHA-256 für `{}` muss 64 Hexzeichen enthalten",
                entry.path
            )));
        }
        if !paths.insert(entry.path.clone()) {
            return Err(LauncherError::InvalidManifest(format!(
                "doppelter Pfad `{}`",
                entry.path
            )));
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, LauncherError> {
    let file = File::open(path).map_err(|source| LauncherError::Io {
        action: "Datei zum Hashen öffnen",
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = BufReader::with_capacity(HASH_BUFFER_BYTES, file);
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
    let mut hasher = Sha256::new();
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|source| LauncherError::Io {
                action: "Datei hashen",
                path: PathBuf::from(path),
                source,
            })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}
