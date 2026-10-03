//! Zentrale Pfadnormalisierung nach Konzept 10.2.
//!
//! Diese Datei ist die einzige Stelle, an der Pfade außerhalb des Vaults
//! aufgelöst werden. Sämtliche Werkzeuge müssen [`safe_join`] verwenden;
//! direkte Aufrufe von `std::fs::canonicalize` oder `Path::join` außerhalb
//! dieser Datei sind ein Bug.
//!
//! Verteidigt wird gegen:
//! - `..`-Traversal (auch mit gemischten Trennzeichen).
//! - Symlinks und Windows-Junctions, die aus dem erlaubten Bereich hinausführen.
//! - NUL-Bytes und ASCII-Steuerzeichen in Komponenten.
//! - Windows-Reservenamen (`CON`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`,
//!   `PRN`, `AUX`) samt Endung.
//! - Trailing dots/spaces, die Windows still trimmt.
//! - UNC-Pfade, die aus dem lokalen Wurzelbereich zeigen.

use std::{
    io,
    path::{Component, Path, PathBuf},
};

use crate::PolicyError;

/// Beschreibt eine erlaubte Wurzel (Workspace, Projektpfad).
///
/// Der Root wird bei der Konstruktion kanonisiert; sämtliche
/// Fremdvergleiche laufen im selben kanonischen Format.
#[derive(Debug, Clone)]
pub struct PathScope {
    root: PathBuf,
}

impl PathScope {
    /// Registriert ein bereits vorhandenes Verzeichnis als Wurzel.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, PolicyError> {
        let canonical = std::fs::canonicalize(root.as_ref())?;
        Ok(Self { root: canonical })
    }

    /// Für Tests: nimmt einen bereits kanonischen Pfad ohne Dateisystemzugriff.
    pub fn from_canonical(root: PathBuf) -> Self {
        Self { root }
    }

    /// Die kanonische Wurzel dieses Bereichs.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Führt die reine Komponentenprüfung durch, ohne Dateisystemzugriff.
///
/// Diese Funktion ist bewusst separat, damit auch schreibende Werkzeuge (die
/// noch keine kanonisierbare Zieldatei haben) die Regeln erzwingen können.
pub fn normalize(candidate: &Path) -> Result<(), PolicyError> {
    for component in candidate.components() {
        match component {
            Component::ParentDir => {
                return Err(PolicyError::PathTraversal {
                    path: candidate.to_path_buf(),
                    reason: "Elternverzeichnisverweis (..)".to_owned(),
                });
            }
            Component::Normal(name) => {
                validate_component(name.to_string_lossy().as_ref(), candidate)?;
            }
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
        }
    }
    Ok(())
}

/// Setzt einen relativen Pfad sicher in einen [`PathScope`] und liefert den
/// kanonisierten absoluten Pfad zurück.
///
/// Auch wenn der Zielpfad noch nicht existiert (Schreibvorgang), wird die
/// letzte Komponente einzeln geprüft und an das kanonisierte Elternverzeichnis
/// gehängt. Symlinks und Junctions im Elternpfad werden dabei automatisch
/// aufgelöst.
pub fn safe_join(scope: &PathScope, relative: &Path) -> Result<PathBuf, PolicyError> {
    normalize(relative)?;
    if relative.is_absolute() {
        return Err(PolicyError::PathTraversal {
            path: relative.to_path_buf(),
            reason: "absoluter Pfad im relativen Kontext".to_owned(),
        });
    }
    if relative.components().count() == 0 {
        return Err(PolicyError::PathTraversal {
            path: relative.to_path_buf(),
            reason: "leerer Pfad".to_owned(),
        });
    }
    let combined = scope.root.join(relative);
    let canonical = canonicalize_lenient(&combined)?;
    if !starts_with_canonical(&canonical, &scope.root) {
        return Err(PolicyError::PathOutOfScope { path: canonical });
    }
    Ok(canonical)
}

/// Prüft einen ABSOLUTEN Pfad gegen einen [`PathScope`], auch wenn mehrere
/// Zielkomponenten noch nicht existieren (z. B. ein neuer Worktree-Ordner).
///
/// Warum: Prozesse wie `git worktree add` bekommen absolute Zielpfade. Die
/// Normalisierung soll weiterhin nur hier leben; der tiefste vorhandene
/// Vorfahre wird kanonisiert (Symlinks, Junctions), der Rest komponentenweise
/// geprüft und angehängt.
pub fn resolve_absolute_in_scope(
    scope: &PathScope,
    absolute: &Path,
) -> Result<PathBuf, PolicyError> {
    normalize(absolute)?;
    if !absolute.is_absolute() {
        return Err(PolicyError::PathTraversal {
            path: absolute.to_path_buf(),
            reason: "relativer Pfad im absoluten Kontext".to_owned(),
        });
    }
    let mut existing = absolute.to_path_buf();
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| PolicyError::PathTraversal {
                path: absolute.to_path_buf(),
                reason: "kein vorhandener Vorfahre".to_owned(),
            })?
            .to_owned();
        missing.push(name);
        existing = existing
            .parent()
            .ok_or_else(|| PolicyError::PathTraversal {
                path: absolute.to_path_buf(),
                reason: "kein Elternverzeichnis".to_owned(),
            })?
            .to_path_buf();
    }
    let mut canonical = std::fs::canonicalize(&existing)?;
    for name in missing.into_iter().rev() {
        validate_component(name.to_string_lossy().as_ref(), absolute)?;
        canonical.push(name);
    }
    if !starts_with_canonical(&canonical, &scope.root) {
        return Err(PolicyError::PathOutOfScope { path: canonical });
    }
    Ok(canonical)
}

/// Führt `canonicalize` aus; existiert die Datei noch nicht, wird die letzte
/// Komponente einzeln geprüft und an den kanonisierten Elternpfad angehängt.
fn canonicalize_lenient(path: &Path) -> Result<PathBuf, PolicyError> {
    match std::fs::canonicalize(path) {
        Ok(canonical) => Ok(canonical),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| PolicyError::PathTraversal {
                path: path.to_path_buf(),
                reason: "kein Elternverzeichnis".to_owned(),
            })?;
            let file_name = path.file_name().ok_or_else(|| PolicyError::PathTraversal {
                path: path.to_path_buf(),
                reason: "kein Dateiname".to_owned(),
            })?;
            validate_component(file_name.to_string_lossy().as_ref(), path)?;
            let canonical_parent = std::fs::canonicalize(parent)?;
            Ok(canonical_parent.join(file_name))
        }
        Err(err) => Err(err.into()),
    }
}

/// Prüft eine einzelne Komponente auf NUL, Steuerzeichen, Reservenamen und
/// Windows-Trailing-Fallen.
fn validate_component(name: &str, full: &Path) -> Result<(), PolicyError> {
    if name.is_empty() {
        return Err(PolicyError::PathTraversal {
            path: full.to_path_buf(),
            reason: "leere Pfadkomponente".to_owned(),
        });
    }
    if name.contains('\0') {
        return Err(PolicyError::PathTraversal {
            path: full.to_path_buf(),
            reason: "NUL-Byte in Komponente".to_owned(),
        });
    }
    if name.chars().any(|c| (c as u32) < 0x20) {
        return Err(PolicyError::PathTraversal {
            path: full.to_path_buf(),
            reason: "Steuerzeichen in Komponente".to_owned(),
        });
    }
    if is_reserved_name(name) {
        return Err(PolicyError::ReservedName {
            path: full.to_path_buf(),
            component: name.to_owned(),
        });
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(PolicyError::PathTraversal {
            path: full.to_path_buf(),
            reason: "abschließender Punkt oder Leerzeichen (Windows trimmt still)".to_owned(),
        });
    }
    Ok(())
}

fn is_reserved_name(name: &str) -> bool {
    let base = match name.find('.') {
        Some(index) => &name[..index],
        None => name,
    };
    let upper = base.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

#[cfg(windows)]
fn starts_with_canonical(candidate: &Path, root: &Path) -> bool {
    // Auf Windows ist Groß-/Kleinschreibung im Dateisystem irrelevant; nach
    // `canonicalize` ist die Schreibweise zwar normalisiert, aber ein zweiter
    // Vergleich als Fallback nutzt den lowercased String, damit auch
    // Netzwerkfreigaben mit uneinheitlicher Groß-/Kleinschreibung sicher sind.
    if candidate.starts_with(root) {
        return true;
    }
    let candidate_lower = candidate.to_string_lossy().to_lowercase();
    let root_lower = root.to_string_lossy().to_lowercase();
    let root_with_sep = if root_lower.ends_with('\\') || root_lower.ends_with('/') {
        root_lower.clone()
    } else {
        format!("{root_lower}\\")
    };
    candidate_lower == root_lower || candidate_lower.starts_with(&root_with_sep)
}

#[cfg(not(windows))]
fn starts_with_canonical(candidate: &Path, root: &Path) -> bool {
    candidate.starts_with(root)
}
