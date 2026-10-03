use std::path::{Component, Path, PathBuf};

use crate::LauncherError;

/// Verankert alle vom Manifest kontrollierten Dateien in einem kanonischen Paketroot.
#[derive(Debug, Clone)]
pub struct PackageRoot {
    canonical: PathBuf,
}

impl PackageRoot {
    /// Kanonisiert das Root einmal, damit spätere Grenzprüfungen dieselbe Identität verwenden.
    pub fn new(path: &Path) -> Result<Self, LauncherError> {
        let canonical = path.canonicalize().map_err(|source| LauncherError::Io {
            action: "Paketroot kanonisieren",
            path: path.to_path_buf(),
            source,
        })?;
        if !canonical.is_dir() {
            return Err(LauncherError::UnsafePath {
                path: path.display().to_string(),
                reason: "Paketroot ist kein Verzeichnis",
            });
        }
        Ok(Self { canonical })
    }

    /// Löst nur existierende relative Pfade auf, deren reales Ziel im Paket bleibt.
    pub fn resolve_existing(&self, relative: &Path) -> Result<PathBuf, LauncherError> {
        validate_relative(relative)?;
        let joined = self.canonical.join(relative);
        let canonical = joined.canonicalize().map_err(|source| LauncherError::Io {
            action: "Paketpfad kanonisieren",
            path: joined,
            source,
        })?;
        if !canonical.starts_with(&self.canonical) {
            return Err(LauncherError::UnsafePath {
                path: relative.display().to_string(),
                reason: "kanonisches Ziel liegt außerhalb des Pakets",
            });
        }
        Ok(canonical)
    }

    /// Macht das kanonische Root für sichere, bereits geprüfte Ableitungen verfügbar.
    pub fn as_path(&self) -> &Path {
        &self.canonical
    }
}

fn validate_relative(path: &Path) -> Result<(), LauncherError> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(LauncherError::UnsafePath {
            path: path.display().to_string(),
            reason: "Pfad muss relativ und nicht leer sein",
        });
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(LauncherError::UnsafePath {
            path: path.display().to_string(),
            reason: "nur normale relative Pfadkomponenten sind erlaubt",
        });
    }
    Ok(())
}
