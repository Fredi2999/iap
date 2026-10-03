use std::{fs, io::Write, path::Path};

use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::error::VaultError;

/// Hält die Argon2-Kosten versioniert, damit bestehende Vaults reproduzierbar bleiben.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argon2Parameters {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl Default for Argon2Parameters {
    fn default() -> Self {
        Self {
            memory_kib: 64 * 1024,
            iterations: 3,
            parallelism: 1,
        }
    }
}

/// Ist der unverschlüsselte, nicht geheime Begleiter, der zur Schlüsselableitung nötig ist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultMeta {
    pub format_version: u32,
    pub kdf: String,
    pub salt_hex: String,
    pub parameters: Argon2Parameters,
}

impl VaultMeta {
    /// Fixiert Algorithmus und Format, statt implizite Bibliotheksdefaults zu speichern.
    pub fn new(salt: [u8; 16], parameters: Argon2Parameters) -> Self {
        Self {
            format_version: 1,
            kdf: "argon2id-v19".to_owned(),
            salt_hex: hex::encode(salt),
            parameters,
        }
    }

    /// Erzeugt den Salt aus der Betriebssystem-Zufallsquelle.
    pub fn random(parameters: Argon2Parameters) -> Self {
        let mut salt = [0_u8; 16];
        OsRng.fill_bytes(&mut salt);
        Self::new(salt, parameters)
    }

    /// Liest und validiert nur unterstützte Metadatenversionen.
    pub fn load(path: &Path) -> Result<Self, VaultError> {
        let bytes = fs::read(path).map_err(|source| VaultError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let meta: Self = serde_json::from_slice(&bytes)
            .map_err(|error| VaultError::InvalidMetadata(error.to_string()))?;
        meta.validate()?;
        Ok(meta)
    }

    /// Veröffentlicht Metadaten erst nach Flush und atomarer Ersetzung.
    pub fn save_atomic(&self, path: &Path) -> Result<(), VaultError> {
        self.validate()?;
        let parent = path.parent().ok_or_else(|| {
            VaultError::InvalidMetadata("Metadatenpfad hat kein Elternverzeichnis".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(|source| VaultError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let partial = path.with_extension("meta.partial");
        let mut file = fs::File::create(&partial).map_err(|source| VaultError::Io {
            path: partial.clone(),
            source,
        })?;
        serde_json::to_writer_pretty(&mut file, self)
            .map_err(|error| VaultError::InvalidMetadata(error.to_string()))?;
        file.write_all(b"\n").map_err(|source| VaultError::Io {
            path: partial.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| VaultError::Io {
            path: partial.clone(),
            source,
        })?;
        replace(&partial, path)
    }

    /// Dekodiert erst nach Formatprüfung den Salt für Argon2id.
    pub(crate) fn salt(&self) -> Result<[u8; 16], VaultError> {
        self.validate()?;
        let decoded = hex::decode(&self.salt_hex)
            .map_err(|error| VaultError::InvalidMetadata(error.to_string()))?;
        decoded.try_into().map_err(|_| {
            VaultError::InvalidMetadata("Salt muss genau 16 Bytes lang sein".to_owned())
        })
    }

    fn validate(&self) -> Result<(), VaultError> {
        if self.format_version != 1 || self.kdf != "argon2id-v19" {
            return Err(VaultError::InvalidMetadata(
                "unbekannte Format- oder KDF-Version".to_owned(),
            ));
        }
        if self.salt_hex.len() != 32 || hex::decode(&self.salt_hex).is_err() {
            return Err(VaultError::InvalidMetadata(
                "Salt muss 16 Bytes als Hex enthalten".to_owned(),
            ));
        }
        if self.parameters.memory_kib < 8
            || self.parameters.iterations == 0
            || self.parameters.parallelism == 0
        {
            return Err(VaultError::InvalidMetadata(
                "Argon2-Parameter liegen außerhalb des gültigen Bereichs".to_owned(),
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
fn replace(source: &Path, destination: &Path) -> Result<(), VaultError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
    };
    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| VaultError::Io {
        path: destination.to_path_buf(),
        source: std::io::Error::other(error),
    })
}

#[cfg(not(windows))]
fn replace(source: &Path, destination: &Path) -> Result<(), VaultError> {
    fs::rename(source, destination).map_err(|source_error| VaultError::Io {
        path: destination.to_path_buf(),
        source: source_error,
    })
}
