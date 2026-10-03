use std::{
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use sha2::{Digest, Sha256};

use crate::{atomic_file, LauncherError};

const COPY_BUFFER_BYTES: usize = 1024 * 1024;
static PARTIAL_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Erlaubt UI-Fortschritt und kontrollierte Fehlerproben, ohne die Kopierlogik zu duplizieren.
pub trait CopyObserver {
    fn copied(&mut self, bytes: u64, total: u64) -> Result<(), LauncherError>;
}

/// Unterscheidet sichtbar zwischen tatsächlicher SSD-Kopie und verifiziertem Treffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheResult {
    Copied(PathBuf),
    Reused(PathBuf),
}

impl CacheResult {
    /// Liefert nur den bereits vollständig verifizierten, veröffentlichten Pfad.
    pub fn path(&self) -> &Path {
        match self {
            Self::Copied(path) | Self::Reused(path) => path,
        }
    }
}

/// Kapselt den Host-Cache, damit Tests niemals echte Benutzerverzeichnisse berühren.
#[derive(Debug, Clone)]
pub struct ModelCache {
    root: PathBuf,
}

impl ModelCache {
    /// Verwendet einen injizierten Root für Tests und explizite portable Konfigurationen.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Wählt den einzigen erlaubten persistenten Hostpfad für Modellkopien.
    pub fn for_current_host() -> Result<Self, LauncherError> {
        let local = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| LauncherError::Cache("LOCALAPPDATA fehlt".to_owned()))?;
        Ok(Self::new(local.join("IAP").join("model-cache")))
    }

    /// Leitet die Identität ausschließlich vom Inhalt ab, nicht vom Laufwerksbuchstaben.
    pub fn entry_path(&self, expected_sha256: &str) -> PathBuf {
        self.root.join(expected_sha256.to_ascii_lowercase())
    }

    /// Teilt den klar benannten Host-Cache mit der Hostprofil-Persistenz.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Veröffentlicht ein Modell erst nach vollständigem Kopieren, Flush und Hashprüfung.
    pub fn ensure_cached(
        &self,
        source: &Path,
        expected_sha256: &str,
        expected_bytes: u64,
        observer: &mut dyn CopyObserver,
    ) -> Result<CacheResult, LauncherError> {
        validate_hash(expected_sha256)?;
        fs::create_dir_all(&self.root).map_err(|source_error| LauncherError::Io {
            action: "Modell-Cache anlegen",
            path: self.root.clone(),
            source: source_error,
        })?;
        let destination = self.entry_path(expected_sha256);
        if valid_file(&destination, expected_sha256, expected_bytes)? {
            return Ok(CacheResult::Reused(destination));
        }

        let suffix = PARTIAL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let partial = self.root.join(format!(
            ".{}.{}.{}.partial",
            expected_sha256,
            std::process::id(),
            suffix
        ));
        let copy_result =
            copy_and_verify(source, &partial, expected_sha256, expected_bytes, observer);
        if let Err(error) = copy_result {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
        atomic_file::replace(&partial, &destination)?;
        Ok(CacheResult::Copied(destination))
    }
}

fn copy_and_verify(
    source: &Path,
    partial: &Path,
    expected_sha256: &str,
    expected_bytes: u64,
    observer: &mut dyn CopyObserver,
) -> Result<(), LauncherError> {
    let input = File::open(source).map_err(|source_error| LauncherError::Io {
        action: "Quellmodell öffnen",
        path: source.to_path_buf(),
        source: source_error,
    })?;
    let actual_source_bytes = input
        .metadata()
        .map_err(|source_error| LauncherError::Io {
            action: "Quellmodellgröße lesen",
            path: source.to_path_buf(),
            source: source_error,
        })?
        .len();
    if actual_source_bytes != expected_bytes {
        return Err(LauncherError::SizeMismatch {
            path: source.to_path_buf(),
            expected: expected_bytes,
            actual: actual_source_bytes,
        });
    }
    let output = File::create(partial).map_err(|source_error| LauncherError::Io {
        action: "partielle Modellkopie anlegen",
        path: partial.to_path_buf(),
        source: source_error,
    })?;
    let mut reader = BufReader::with_capacity(COPY_BUFFER_BYTES, input);
    let mut writer = BufWriter::with_capacity(COPY_BUFFER_BYTES, output);
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|source_error| LauncherError::Io {
                action: "Quellmodell lesen",
                path: source.to_path_buf(),
                source: source_error,
            })?;
        if count == 0 {
            break;
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|source_error| LauncherError::Io {
                action: "partielle Modellkopie schreiben",
                path: partial.to_path_buf(),
                source: source_error,
            })?;
        hasher.update(&buffer[..count]);
        copied = copied.saturating_add(count as u64);
        observer.copied(copied, expected_bytes)?;
    }
    writer.flush().map_err(|source_error| LauncherError::Io {
        action: "Modellkopie flushen",
        path: partial.to_path_buf(),
        source: source_error,
    })?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|source_error| LauncherError::Io {
            action: "Modellkopie synchronisieren",
            path: partial.to_path_buf(),
            source: source_error,
        })?;
    let actual_hash = hex::encode(hasher.finalize());
    if actual_hash != expected_sha256.to_ascii_lowercase() {
        return Err(LauncherError::HashMismatch {
            path: source.to_path_buf(),
            expected: expected_sha256.to_ascii_lowercase(),
            actual: actual_hash,
        });
    }
    Ok(())
}

fn valid_file(
    path: &Path,
    expected_sha256: &str,
    expected_bytes: u64,
) -> Result<bool, LauncherError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(LauncherError::Io {
                action: "Cache-Eintrag prüfen",
                path: path.to_path_buf(),
                source,
            })
        }
    };
    if !metadata.is_file() || metadata.len() != expected_bytes {
        return Ok(false);
    }
    let mut file = BufReader::with_capacity(
        COPY_BUFFER_BYTES,
        File::open(path).map_err(|source| LauncherError::Io {
            action: "Cache-Eintrag öffnen",
            path: path.to_path_buf(),
            source,
        })?,
    );
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut hasher = Sha256::new();
    loop {
        let count = file.read(&mut buffer).map_err(|source| LauncherError::Io {
            action: "Cache-Eintrag hashen",
            path: path.to_path_buf(),
            source,
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()).eq_ignore_ascii_case(expected_sha256))
}

fn validate_hash(hash: &str) -> Result<(), LauncherError> {
    if hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(LauncherError::Cache(
            "Cache-Schlüssel muss ein SHA-256 sein".to_owned(),
        ))
    }
}
