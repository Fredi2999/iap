//! Optionale Zusatzpakete unter `AI/packs/<id>/` (Sprache, Vorlesen, Bild, Git).
//!
//! Jedes Paket bringt eine `PACK.toml` mit Dateiliste, Größen und SHA-256 mit
//! (erzeugt von `packaging/windows/packs.ps1`). IAP nutzt ein Paket nur,
//! wenn alle Dateien da sind und die Größen stimmen; die Hashes werden vor der
//! ersten Nutzung einer Sitzung geprüft. Fehlt etwas, ist die Funktion
//! sichtbar „nicht eingerichtet“ – es gibt keinen stillen Ersatz und keinen
//! Download.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use pa_policy::{safe_join, PathScope};
use pa_types::voice::PackStatus;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Kennungen der bekannten Pakete.
pub const PACK_WHISPER: &str = "whisper";
pub const PACK_PIPER: &str = "piper";
pub const PACK_VISION: &str = "vision";
pub const PACK_GIT: &str = "git";

/// Fehler beim Lesen oder Prüfen eines Pakets.
#[derive(Debug, Error)]
pub enum PackError {
    /// Das Paket ist nicht installiert.
    #[error("Paket `{0}` ist nicht eingerichtet")]
    Missing(String),
    /// Manifest unlesbar oder unvollständig.
    #[error("Paket `{id}`: Manifest ungültig: {reason}")]
    Manifest { id: String, reason: String },
    /// Datei fehlt oder hat die falsche Größe.
    #[error("Paket `{id}`: Datei `{file}` fehlt oder ist beschädigt")]
    File { id: String, file: String },
    /// Hash stimmt nicht.
    #[error("Paket `{id}`: Prüfsumme von `{file}` stimmt nicht")]
    Hash { id: String, file: String },
}

/// Eine Datei der Paketliste.
#[derive(Debug, Clone, Deserialize)]
pub struct PackFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Inhalt der `PACK.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct PackManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub license: String,
    /// Nur für Projektor-Pakete: zu welchem Modell er gehört.
    #[serde(default)]
    pub for_model: Option<String>,
    #[serde(default, rename = "file")]
    pub files: Vec<PackFile>,
}

/// Ein gefundenes Paket.
#[derive(Debug, Clone)]
pub struct Pack {
    dir: PathBuf,
    scope: PathScope,
    pub manifest: PackManifest,
}

impl Pack {
    /// Liest das Manifest von `packs_root/<id>/PACK.toml`.
    ///
    /// # Errors
    /// `Missing`, wenn der Ordner oder das Manifest fehlt.
    pub fn open(packs_root: &Path, id: &str) -> Result<Self, PackError> {
        let dir = packs_root.join(id);
        let text = fs::read_to_string(dir.join("PACK.toml"))
            .map_err(|_| PackError::Missing(id.to_owned()))?;
        let manifest: PackManifest = toml::from_str(&text).map_err(|e| PackError::Manifest {
            id: id.to_owned(),
            reason: e.to_string(),
        })?;
        if manifest.id != id {
            return Err(PackError::Manifest {
                id: id.to_owned(),
                reason: "Kennung passt nicht zum Ordner".to_owned(),
            });
        }
        let scope = PathScope::new(&dir).map_err(|e| PackError::Manifest {
            id: id.to_owned(),
            reason: e.to_string(),
        })?;
        Ok(Self {
            dir,
            scope,
            manifest,
        })
    }

    /// Ordner des Pakets.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Löst eine Datei des Pakets sicher auf (keine Ausbrüche über `..` oder Links).
    pub fn file(&self, relative: &str) -> Result<PathBuf, PackError> {
        safe_join(&self.scope, Path::new(relative)).map_err(|_| PackError::File {
            id: self.manifest.id.clone(),
            file: relative.to_owned(),
        })
    }

    /// Prüft Vorhandensein und Größe aller Dateien (schnell, ohne Hash).
    pub fn check_sizes(&self) -> Result<(), PackError> {
        if self.manifest.files.is_empty() {
            return Err(PackError::Manifest {
                id: self.manifest.id.clone(),
                reason: "Dateiliste ist leer".to_owned(),
            });
        }
        for file in &self.manifest.files {
            let path = self.file(&file.path)?;
            let meta = fs::metadata(&path).map_err(|_| PackError::File {
                id: self.manifest.id.clone(),
                file: file.path.clone(),
            })?;
            if !meta.is_file() || meta.len() != file.size_bytes {
                return Err(PackError::File {
                    id: self.manifest.id.clone(),
                    file: file.path.clone(),
                });
            }
        }
        Ok(())
    }

    /// Prüft zusätzlich alle SHA-256-Summen. Bei großen Paketen dauert das
    /// einige Sekunden; der Aufrufer macht es einmal je Sitzung im Hintergrund.
    pub fn verify_hashes(&self) -> Result<(), PackError> {
        self.check_sizes()?;
        for file in &self.manifest.files {
            let path = self.file(&file.path)?;
            let actual = sha256_of(&path).map_err(|_| PackError::File {
                id: self.manifest.id.clone(),
                file: file.path.clone(),
            })?;
            if !actual.eq_ignore_ascii_case(&file.sha256) {
                return Err(PackError::Hash {
                    id: self.manifest.id.clone(),
                    file: file.path.clone(),
                });
            }
        }
        Ok(())
    }

    /// Gesamtgröße laut Manifest.
    pub fn size_bytes(&self) -> u64 {
        self.manifest.files.iter().map(|f| f.size_bytes).sum()
    }
}

fn sha256_of(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Statuszeile für die Oberfläche; prüft nur Größen (schnell).
pub fn status(packs_root: &Path, id: &str, display_name: &str, enabled: bool) -> PackStatus {
    match Pack::open(packs_root, id).and_then(|pack| pack.check_sizes().map(|()| pack)) {
        Ok(pack) => PackStatus {
            id: id.to_owned(),
            name: pack.manifest.name.clone(),
            installed: true,
            enabled,
            size_bytes: pack.size_bytes(),
            version: Some(pack.manifest.version.clone()),
            license: Some(pack.manifest.license.clone()),
            missing_reason: None,
        },
        Err(error) => PackStatus {
            id: id.to_owned(),
            name: display_name.to_owned(),
            installed: false,
            enabled: false,
            size_bytes: 0,
            version: None,
            license: None,
            missing_reason: Some(error.to_string()),
        },
    }
}

/// Ordner der Pakete unter der Paketwurzel.
pub fn packs_root(package_root: &Path) -> PathBuf {
    package_root.join("AI").join("packs")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pack(root: &Path, id: &str, content: &[u8], declared_size: u64, hash: &str) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("data.bin"), content).unwrap();
        fs::write(
            dir.join("PACK.toml"),
            format!(
                "id = \"{id}\"\nname = \"Test\"\nversion = \"1\"\nlicense = \"MIT\"\n\n[[file]]\npath = \"data.bin\"\nsize_bytes = {declared_size}\nsha256 = \"{hash}\"\n"
            ),
        )
        .unwrap();
    }

    fn hash(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    #[test]
    fn intact_pack_passes_size_and_hash_checks() {
        let temp = tempfile::TempDir::new().unwrap();
        make_pack(temp.path(), "whisper", b"hallo", 5, &hash(b"hallo"));
        let pack = Pack::open(temp.path(), "whisper").unwrap();
        pack.check_sizes().unwrap();
        pack.verify_hashes().unwrap();
        assert_eq!(pack.size_bytes(), 5);
    }

    #[test]
    fn wrong_size_or_hash_is_detected() {
        let temp = tempfile::TempDir::new().unwrap();
        make_pack(temp.path(), "a", b"hallo", 6, &hash(b"hallo"));
        assert!(matches!(
            Pack::open(temp.path(), "a").unwrap().check_sizes(),
            Err(PackError::File { .. })
        ));
        make_pack(temp.path(), "b", b"hallo", 5, &hash(b"anders"));
        let pack = Pack::open(temp.path(), "b").unwrap();
        pack.check_sizes().unwrap();
        assert!(matches!(pack.verify_hashes(), Err(PackError::Hash { .. })));
    }

    #[test]
    fn missing_pack_is_reported_as_not_set_up() {
        let temp = tempfile::TempDir::new().unwrap();
        let status = status(temp.path(), "piper", "Sprachausgabe", true);
        assert!(!status.installed);
        assert!(!status.enabled);
        assert!(status
            .missing_reason
            .unwrap()
            .contains("nicht eingerichtet"));
    }

    #[test]
    fn manifest_id_must_match_folder() {
        let temp = tempfile::TempDir::new().unwrap();
        make_pack(temp.path(), "whisper", b"x", 1, &hash(b"x"));
        fs::rename(temp.path().join("whisper"), temp.path().join("piper")).unwrap();
        assert!(matches!(
            Pack::open(temp.path(), "piper"),
            Err(PackError::Manifest { .. })
        ));
    }

    #[test]
    fn file_paths_cannot_escape_the_pack() {
        let temp = tempfile::TempDir::new().unwrap();
        make_pack(temp.path(), "git", b"x", 1, &hash(b"x"));
        let pack = Pack::open(temp.path(), "git").unwrap();
        assert!(pack.file("../whisper/data.bin").is_err());
        assert!(pack.file("data.bin").is_ok());
    }
}
