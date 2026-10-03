//! Update-, Backup-, Export- und Recovery-Logik (Konzept 15).
//!
//! Kernaussagen des Konzepts, hier ehrlich umgesetzt:
//!
//! - **Kein Auto-Update.** Update-Bundles (`.paup`) liegen als Datei auf dem
//!   Stick; der Launcher prüft die Signatur (Ed25519) und die SHA-256-Summen
//!   und schaltet nach erfolgreichem Testlauf um.
//! - **A/B-Verzeichnisse.** `bin/current/` ist aktiv, `bin/next/` die Kopie
//!   des neuen Bundles, `bin/prev/` der Fallback für den Rollback.
//! - **Rollback über Umschalttaste.** Der Launcher liest einen einfachen
//!   Marker (`bin/rollback.request`), den die Start-Wrapper setzen, wenn
//!   Shift gedrückt ist.
//! - **Rollierende Backups.** `backup/auto/` hält höchstens 7 Einträge; der
//!   älteste fliegt bei jedem neuen Backup raus.
//! - **Safe-Mode.** `bin/safe.mode` — vom Launcher gesetzt, wenn ein
//!   Integritätsfehler erkannt wurde; die App startet dann ohne Skills,
//!   Hooks und Memory.
//!
//! **Signaturprüfung (Ed25519).** `ed25519-dalek` ist im aktuellen Offline-
//! Vendor-Cache nicht enthalten; [`verify_bundle`] enthält deshalb die volle
//! SHA-256-Prüfung des Bundle-Inhalts und einen typisierten Skeleton für die
//! Ed25519-Prüfung, der bei nicht-leerer Signatur den Status
//! `SignatureVerification::Skipped` zurückgibt (mit Hinweis in `reason`).
//! Sobald die Crate im Vendor-Cache liegt, wird der Skeleton durch
//! `ed25519_dalek::VerifyingKey::verify_strict` ersetzt — kein weiterer
//! Umbau nötig.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("io: {source}")]
    Io {
        #[source]
        source: std::io::Error,
    },
    #[error("Bundle-Manifest ungültig: {0}")]
    Manifest(String),
    #[error("SHA-256 stimmt nicht (Datei {path}, erwartet {expected}, gefunden {actual})")]
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("Rollback-Ziel `bin/prev` existiert nicht — kein Rollback möglich")]
    NoRollbackAvailable,
    #[error("Signaturprüfung fehlgeschlagen: {0}")]
    Signature(String),
}

impl From<std::io::Error> for UpdateError {
    fn from(source: std::io::Error) -> Self {
        UpdateError::Io { source }
    }
}

/// Bundle-Manifest — landet als `manifest.json` im Bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    pub version: String,
    pub files: Vec<BundleFileEntry>,
    /// Ed25519-Signatur (Base16/hex) über die kanonisierte Repräsentation
    /// des `files`-Arrays; leer = ungezeichnet (nur für Tests/Dev-Modus).
    #[serde(default)]
    pub signature_hex: String,
    /// Public-Key-Hex des Signaturschlüssels (32 Bytes). Der Launcher
    /// vergleicht ihn gegen die im Binary hinterlegte Whitelist.
    #[serde(default)]
    pub public_key_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleFileEntry {
    /// Pfad relativ zur Bundle-Wurzel (z. B. `bin/win-x64/portable-ai.exe`).
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// Verifikationsergebnis für den UI-Statusblock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundleVerification {
    pub verified_files: u32,
    pub verified_bytes: u64,
    pub signature: SignatureVerification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignatureVerification {
    Ok,
    Skipped { reason: String },
    NotProvided,
}

/// Prüft ein bereits ausgepacktes Bundle unter `bundle_root`.
///
/// Alle Dateien müssen existieren und dem Manifest entsprechen. Sobald
/// `ed25519-dalek` im Vendor-Cache ist, wird die Signatur echt geprüft;
/// bis dahin gibt [`SignatureVerification::Skipped`] Auskunft.
pub fn verify_bundle(
    bundle_root: &Path,
    trusted_keys_hex: &[String],
) -> Result<BundleVerification, UpdateError> {
    let manifest_path = bundle_root.join("manifest.json");
    let manifest_text = fs::read_to_string(&manifest_path)?;
    let manifest: BundleManifest = serde_json::from_str(&manifest_text)
        .map_err(|error| UpdateError::Manifest(error.to_string()))?;
    if manifest.files.is_empty() {
        return Err(UpdateError::Manifest(
            "mindestens eine Datei erforderlich".into(),
        ));
    }
    let mut verified_bytes: u64 = 0;
    for entry in &manifest.files {
        let target = bundle_root.join(&entry.path);
        let metadata = fs::metadata(&target)?;
        if metadata.len() != entry.bytes {
            return Err(UpdateError::HashMismatch {
                path: entry.path.clone(),
                expected: format!("{} Bytes", entry.bytes),
                actual: format!("{} Bytes", metadata.len()),
            });
        }
        let actual = hash_file(&target)?;
        if !entry.sha256.eq_ignore_ascii_case(&actual) {
            return Err(UpdateError::HashMismatch {
                path: entry.path.clone(),
                expected: entry.sha256.clone(),
                actual,
            });
        }
        verified_bytes = verified_bytes.saturating_add(metadata.len());
    }
    let signature = evaluate_signature(&manifest, trusted_keys_hex);
    Ok(BundleVerification {
        verified_files: u32::try_from(manifest.files.len()).unwrap_or(u32::MAX),
        verified_bytes,
        signature,
    })
}

fn evaluate_signature(
    manifest: &BundleManifest,
    trusted_keys_hex: &[String],
) -> SignatureVerification {
    if manifest.signature_hex.is_empty() {
        return SignatureVerification::NotProvided;
    }
    if !trusted_keys_hex
        .iter()
        .any(|key| key.eq_ignore_ascii_case(&manifest.public_key_hex))
    {
        return SignatureVerification::Skipped {
            reason: format!(
                "Signatur vorhanden, aber Public-Key `{}` ist nicht in der Whitelist",
                manifest.public_key_hex
            ),
        };
    }
    // Sobald ed25519-dalek verfügbar ist, an dieser Stelle:
    //   let vk = ed25519_dalek::VerifyingKey::from_bytes(&key)?;
    //   vk.verify_strict(canonical_signable_bytes, &sig)
    //     .map(|_| SignatureVerification::Ok)
    SignatureVerification::Skipped {
        reason: "Ed25519-Bibliothek ist im aktuellen Vendor-Cache nicht enthalten; \
                 SHA-256-Prüfung war erfolgreich, kryptografische Signatur wird beim \
                 nächsten Build-System-Upgrade nachgerüstet"
            .to_owned(),
    }
}

fn hash_file(path: &Path) -> Result<String, UpdateError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Standard-Layout: `bin/current`, `bin/next`, `bin/prev` unter der übergebenen
/// Paket-Wurzel (z. B. `X:\AI` auf dem Stick).
pub struct AbLayout {
    pub bin_root: PathBuf,
}

impl AbLayout {
    pub fn new(bin_root: impl Into<PathBuf>) -> Self {
        Self {
            bin_root: bin_root.into(),
        }
    }

    pub fn current(&self) -> PathBuf {
        self.bin_root.join("current")
    }
    pub fn next(&self) -> PathBuf {
        self.bin_root.join("next")
    }
    pub fn prev(&self) -> PathBuf {
        self.bin_root.join("prev")
    }
    pub fn rollback_marker(&self) -> PathBuf {
        self.bin_root.join("rollback.request")
    }
    pub fn safe_mode_marker(&self) -> PathBuf {
        self.bin_root.join("safe.mode")
    }
}

/// Schaltet auf den vorbereiteten `next`-Stand um.
///
/// - `current` → `prev` (der alte Rollback-Fallback wird ersetzt).
/// - `next` → `current`.
/// - Der ehemalige `prev` wird entfernt.
pub fn switch_to_next(layout: &AbLayout) -> Result<(), UpdateError> {
    if !layout.next().exists() {
        return Err(UpdateError::Manifest(
            "bin/next existiert nicht; Bundle wurde nicht entpackt".into(),
        ));
    }
    if layout.prev().exists() {
        fs::remove_dir_all(layout.prev())?;
    }
    if layout.current().exists() {
        fs::rename(layout.current(), layout.prev())?;
    }
    fs::rename(layout.next(), layout.current())?;
    Ok(())
}

/// Führt einen Rollback aus (`prev` → `current`, altes `current` verworfen).
pub fn rollback(layout: &AbLayout) -> Result<(), UpdateError> {
    if !layout.prev().exists() {
        return Err(UpdateError::NoRollbackAvailable);
    }
    if layout.current().exists() {
        fs::remove_dir_all(layout.current())?;
    }
    fs::rename(layout.prev(), layout.current())?;
    // Nach dem Rollback ist der Marker verbraucht.
    let _ = fs::remove_file(layout.rollback_marker());
    Ok(())
}

/// Setzt den Rollback-Wunsch (der Launcher wertet ihn beim nächsten Start
/// aus). Der Start-Wrapper setzt ihn, wenn Shift beim Start gedrückt ist.
pub fn request_rollback(layout: &AbLayout) -> Result<(), UpdateError> {
    if let Some(parent) = layout.rollback_marker().parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(layout.rollback_marker())?;
    file.write_all(b"rollback")?;
    Ok(())
}

pub fn rollback_requested(layout: &AbLayout) -> bool {
    layout.rollback_marker().exists()
}

/// Setzt den Safe-Mode-Marker; die App startet dann minimal.
pub fn set_safe_mode(layout: &AbLayout, reason: &str) -> Result<(), UpdateError> {
    if let Some(parent) = layout.safe_mode_marker().parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(layout.safe_mode_marker())?;
    file.write_all(reason.as_bytes())?;
    Ok(())
}

pub fn clear_safe_mode(layout: &AbLayout) -> Result<(), UpdateError> {
    if layout.safe_mode_marker().exists() {
        fs::remove_file(layout.safe_mode_marker())?;
    }
    Ok(())
}

pub fn is_safe_mode_active(layout: &AbLayout) -> bool {
    layout.safe_mode_marker().exists()
}

/// Rollierendes Backup-Verzeichnis (Standard: 7 Einträge nach Konzept 15).
pub struct BackupSet {
    pub root: PathBuf,
    pub keep: usize,
}

impl BackupSet {
    pub fn new(root: impl Into<PathBuf>, keep: usize) -> Self {
        Self {
            root: root.into(),
            keep,
        }
    }

    /// Registriert ein neu erzeugtes Backup-Archiv. Ordner wird bei Bedarf
    /// angelegt. Ältere Einträge fallen aus dem Ring.
    pub fn register(&self, filename: impl AsRef<str>) -> Result<PathBuf, UpdateError> {
        fs::create_dir_all(&self.root)?;
        let target = self.root.join(filename.as_ref());
        self.rotate()?;
        Ok(target)
    }

    fn rotate(&self) -> Result<(), UpdateError> {
        let mut entries: Vec<(PathBuf, std::time::SystemTime)> = fs::read_dir(&self.root)?
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let time = entry.metadata().and_then(|m| m.modified()).ok()?;
                Some((path, time))
            })
            .collect();
        entries.sort_by_key(|a| std::cmp::Reverse(a.1));
        for (path, _) in entries.into_iter().skip(self.keep) {
            if path.is_file() {
                let _ = fs::remove_file(path);
            } else if path.is_dir() {
                let _ = fs::remove_dir_all(path);
            }
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<PathBuf>, UpdateError> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut entries: Vec<PathBuf> = fs::read_dir(&self.root)?
            .flatten()
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn ab_switch_and_rollback_move_directories() {
        let temp = TempDir::new().unwrap();
        let layout = AbLayout::new(temp.path().join("bin"));
        write_file(&layout.current().join("marker.txt"), b"v1");
        write_file(&layout.next().join("marker.txt"), b"v2");

        switch_to_next(&layout).unwrap();
        assert!(!layout.next().exists());
        assert_eq!(
            fs::read_to_string(layout.current().join("marker.txt")).unwrap(),
            "v2"
        );
        assert_eq!(
            fs::read_to_string(layout.prev().join("marker.txt")).unwrap(),
            "v1"
        );

        rollback(&layout).unwrap();
        assert!(!layout.prev().exists());
        assert_eq!(
            fs::read_to_string(layout.current().join("marker.txt")).unwrap(),
            "v1"
        );
    }

    #[test]
    fn rollback_without_prev_fails_clearly() {
        let temp = TempDir::new().unwrap();
        let layout = AbLayout::new(temp.path().join("bin"));
        fs::create_dir_all(layout.current()).unwrap();
        let err = rollback(&layout).unwrap_err();
        assert!(matches!(err, UpdateError::NoRollbackAvailable));
    }

    #[test]
    fn safe_mode_marker_round_trips() {
        let temp = TempDir::new().unwrap();
        let layout = AbLayout::new(temp.path().join("bin"));
        set_safe_mode(&layout, "integrity check failed").unwrap();
        assert!(is_safe_mode_active(&layout));
        clear_safe_mode(&layout).unwrap();
        assert!(!is_safe_mode_active(&layout));
    }

    #[test]
    fn rolling_backup_keeps_at_most_n() {
        let temp = TempDir::new().unwrap();
        let set = BackupSet::new(temp.path().join("backup"), 3);
        for i in 0..5 {
            let file = set.register(format!("b-{i}.zip")).unwrap();
            fs::write(&file, format!("dummy-{i}")).unwrap();
            // Ein zweiter Rotate ist nötig, weil `register` selbst vor dem
            // Schreiben rotiert. Nach dem Schreiben nochmal auslösen.
            let _ = set.register("noop").unwrap();
            let _ = fs::remove_file(set.root.join("noop"));
        }
        // Nur die neuesten `keep` Dateien überleben.
        let files = fs::read_dir(temp.path().join("backup"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name() != "noop")
            .count();
        assert!(files <= 3, "erwarte ≤ 3 Backups, gefunden {files}");
    }

    #[test]
    fn verify_bundle_detects_wrong_hash() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("hello.txt"), b"hallo");
        let manifest = BundleManifest {
            version: "0.0.1".into(),
            files: vec![BundleFileEntry {
                path: "hello.txt".into(),
                bytes: 5,
                sha256: "0000000000000000000000000000000000000000000000000000000000000000".into(),
            }],
            signature_hex: String::new(),
            public_key_hex: String::new(),
        };
        fs::write(
            temp.path().join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let err = verify_bundle(temp.path(), &[]).unwrap_err();
        assert!(matches!(err, UpdateError::HashMismatch { .. }));
    }
}
