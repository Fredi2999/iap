use std::{
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::Path,
};

use crate::VaultError;

const ZERO_BUFFER_BYTES: usize = 1024 * 1024;

/// Überschreibt reguläre Hot-Copy-Dateien vor dem Entfernen bestmöglich.
///
/// Flash- und SSD-Controller können physische Altblöcke durch Wear-Leveling behalten;
/// die eigentliche Vertraulichkeitsgrenze bleibt daher SQLCipher plus Schlüssel-Drop.
pub fn best_effort_secure_delete(path: &Path) -> Result<(), VaultError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(VaultError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    if !metadata.is_file() {
        return Err(VaultError::Integrity(
            "Bereinigungsziel ist keine reguläre Datei".to_owned(),
        ));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|source| VaultError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.seek(SeekFrom::Start(0))
        .map_err(|source| VaultError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let zeros = vec![0_u8; ZERO_BUFFER_BYTES];
    let mut remaining = metadata.len();
    while remaining > 0 {
        let count = remaining.min(ZERO_BUFFER_BYTES as u64) as usize;
        file.write_all(&zeros[..count])
            .map_err(|source| VaultError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        remaining -= count as u64;
    }
    file.flush().map_err(|source| VaultError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.sync_all().map_err(|source| VaultError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    drop(file);
    fs::remove_file(path).map_err(|source| VaultError::Io {
        path: path.to_path_buf(),
        source,
    })
}
