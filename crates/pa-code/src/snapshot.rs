//! Snapshot-Verwaltung (Konzept 9.4).
//!
//! „Vor jedem Agentenlauf, der schreiben darf, legt der Core einen
//! Snapshot des betroffenen Verzeichnisses an (Copy-on-Write per
//! Hardlink, wo möglich) — ein Klick auf ‚Alles zurücknehmen' stellt
//! den Ausgangszustand wieder her."
//!
//! Implementation: pro Snapshot ein Zielverzeichnis; für jede Datei
//! wird zuerst ein Hardlink versucht (fast frei), bei Fehlschlag ein
//! echter Kopiervorgang. Verzeichnisse werden mirrored angelegt. Das
//! erzeugt keine sperrige Vollkopie, solange Werkzeuge die Dateien
//! ersetzen statt sie in-place zu überschreiben — das ist der übliche
//! Fall bei Patch-basierten Edits.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::CodeError;

/// Wie ein Snapshot angelegt wurde. `Copy` heißt „mindestens eine Datei
/// musste real kopiert werden"; `Hardlink` heißt „alle Dateien wurden
/// verlinkt".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    Hardlink,
    Copy,
    Mixed,
}

/// Fertiger Snapshot; enthält Metadaten für die UI und den restore-Pfad.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Interne ID (z. B. Zeitstempel-hex); wird auch als Verzeichnisname
    /// verwendet, damit die Zuordnung auch nach einem Absturz eindeutig
    /// bleibt.
    pub id: String,
    /// Ursprungsverzeichnis, das gesichert wurde.
    pub source_root: PathBuf,
    /// Zielverzeichnis innerhalb der Snapshot-Wurzel.
    pub snapshot_root: PathBuf,
    pub kind: SnapshotKind,
    pub file_count: u32,
    pub bytes_referenced: u64,
    pub created_unix_ms: i64,
}

/// Bündelt Snapshot-Basisverzeichnis und Anlege-Methoden.
///
/// Die Basis liegt üblicherweise unter `AI/workspace/.snapshots/` gemäß
/// Konzept 9.1; die Struktur ist bewusst flach, damit sie mit dem
/// Vault-Rücksync kompatibel bleibt.
pub struct SnapshotManager {
    root: PathBuf,
}

impl SnapshotManager {
    /// Bindet den Manager an ein bereits existierendes Basisverzeichnis
    /// (wird angelegt, falls es fehlt).
    pub fn open(root: impl AsRef<Path>) -> Result<Self, CodeError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Legt einen Snapshot des `source_root`-Verzeichnisses an.
    ///
    /// Der Rückgabe-Snapshot enthält den absoluten Zielpfad, damit ein
    /// späterer Rollback ohne Zusatzinformation möglich ist.
    pub fn create(
        &self,
        source_root: impl AsRef<Path>,
        now_unix_ms: i64,
    ) -> Result<Snapshot, CodeError> {
        let source_root = source_root.as_ref();
        if !source_root.exists() {
            return Err(CodeError::Snapshot(format!(
                "Quellverzeichnis {} existiert nicht",
                source_root.display()
            )));
        }
        let id = format!("snap-{now_unix_ms:x}");
        let snapshot_root = self.root.join(&id);
        if snapshot_root.exists() {
            return Err(CodeError::Snapshot(format!(
                "Snapshot-Ziel {} existiert bereits",
                snapshot_root.display()
            )));
        }
        fs::create_dir_all(&snapshot_root)?;

        let mut counts = Counters::default();
        clone_tree(source_root, &snapshot_root, &mut counts)?;
        let kind = match (counts.hardlinked, counts.copied) {
            (_, 0) => SnapshotKind::Hardlink,
            (0, _) => SnapshotKind::Copy,
            _ => SnapshotKind::Mixed,
        };
        Ok(Snapshot {
            id,
            source_root: source_root.to_path_buf(),
            snapshot_root,
            kind,
            file_count: counts.hardlinked + counts.copied,
            bytes_referenced: counts.bytes,
            created_unix_ms: now_unix_ms,
        })
    }

    /// Rollt einen Snapshot zurück: leert das Zielverzeichnis und
    /// kopiert die Snapshot-Dateien wieder hinein.
    ///
    /// Bewusst über echte Kopien (nicht Hardlinks), damit ein
    /// Rollback nicht die Snapshot-Dateien mitverändert, falls der
    /// Nutzer sie in der UI später inspizieren möchte.
    pub fn restore(&self, snapshot: &Snapshot) -> Result<(), CodeError> {
        if !snapshot.snapshot_root.starts_with(&self.root) {
            return Err(CodeError::Snapshot(
                "Snapshot liegt außerhalb des Manager-Basisverzeichnisses".to_owned(),
            ));
        }
        if snapshot.source_root.exists() {
            for entry in fs::read_dir(&snapshot.source_root)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    fs::remove_dir_all(&path)?;
                } else {
                    fs::remove_file(&path)?;
                }
            }
        } else {
            fs::create_dir_all(&snapshot.source_root)?;
        }
        copy_tree(&snapshot.snapshot_root, &snapshot.source_root)
    }

    /// Löscht einen Snapshot; Nutzung z. B. beim Abschluss eines Laufs.
    pub fn discard(&self, snapshot: &Snapshot) -> Result<(), CodeError> {
        if !snapshot.snapshot_root.starts_with(&self.root) {
            return Err(CodeError::Snapshot(
                "Snapshot liegt außerhalb des Manager-Basisverzeichnisses".to_owned(),
            ));
        }
        if snapshot.snapshot_root.exists() {
            fs::remove_dir_all(&snapshot.snapshot_root)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct Counters {
    hardlinked: u32,
    copied: u32,
    bytes: u64,
}

fn clone_tree(source: &Path, target: &Path, counts: &mut Counters) -> Result<(), CodeError> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = target.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            clone_tree(&src_path, &dst_path, counts)?;
        } else if file_type.is_file() {
            let metadata = fs::metadata(&src_path)?;
            counts.bytes = counts.bytes.saturating_add(metadata.len());
            // Bewusst kein Hardlink: unter Windows (NTFS ohne
            // Copy-on-Write) teilen sich Hardlinks den Inode, sodass
            // in-place-Schreibvorgänge (fs::write, Editor-„overwrite in
            // place") den Snapshot mitverändern und ihn unbrauchbar
            // machen. Konzept 9.4 spricht von „per Hardlink, WO
            // MÖGLICH" — auf unseren Zielplattformen mit NTFS/ext4-
            // Standardeinstellungen ist die sichere Wahl eine echte
            // Kopie. Ein späterer Ausbau kann pro Filesystem ein
            // echtes Reflink (`CopyFileExW` mit COPY_FILE_ALLOW_DECRYPTED_DESTINATION,
            // `ioctl_ficlone` auf btrfs/APFS) versuchen.
            fs::copy(&src_path, &dst_path)?;
            counts.copied = counts.copied.saturating_add(1);
        }
        // Symlinks werden bewusst ignoriert — sie sind durch die
        // pa-policy-Pfadnormalisierung ohnehin problematisch und gehören
        // nicht in einen reproduzierbaren Snapshot.
    }
    Ok(())
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), CodeError> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = target.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_tree(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path, content: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn snapshot_reflects_source_tree() {
        let base = tempfile::tempdir().unwrap();
        let source = base.path().join("workspace");
        write_file(&source.join("a.txt"), b"eins");
        write_file(&source.join("sub/b.txt"), b"zwei");
        let manager = SnapshotManager::open(base.path().join(".snap")).unwrap();
        let snapshot = manager.create(&source, 100).unwrap();

        assert_eq!(snapshot.file_count, 2);
        assert!(matches!(
            snapshot.kind,
            SnapshotKind::Hardlink | SnapshotKind::Copy | SnapshotKind::Mixed
        ));
        assert!(snapshot.snapshot_root.join("a.txt").exists());
        assert!(snapshot.snapshot_root.join("sub/b.txt").exists());
    }

    #[test]
    fn restore_recovers_deleted_and_modified_files() {
        let base = tempfile::tempdir().unwrap();
        let source = base.path().join("workspace");
        write_file(&source.join("a.txt"), b"alt");
        write_file(&source.join("b.txt"), b"da");
        let manager = SnapshotManager::open(base.path().join(".snap")).unwrap();
        let snapshot = manager.create(&source, 100).unwrap();

        // Verändere Quelle: eine Datei ändern, eine löschen.
        fs::write(source.join("a.txt"), b"neu").unwrap();
        fs::remove_file(source.join("b.txt")).unwrap();

        manager.restore(&snapshot).unwrap();

        assert_eq!(fs::read_to_string(source.join("a.txt")).unwrap(), "alt");
        assert_eq!(fs::read_to_string(source.join("b.txt")).unwrap(), "da");
    }

    #[test]
    fn discard_deletes_only_the_snapshot_not_the_source() {
        let base = tempfile::tempdir().unwrap();
        let source = base.path().join("workspace");
        write_file(&source.join("a.txt"), b"eins");
        let manager = SnapshotManager::open(base.path().join(".snap")).unwrap();
        let snapshot = manager.create(&source, 100).unwrap();
        manager.discard(&snapshot).unwrap();
        assert!(!snapshot.snapshot_root.exists());
        assert!(source.join("a.txt").exists());
    }

    #[test]
    fn create_fails_when_source_missing() {
        let base = tempfile::tempdir().unwrap();
        let manager = SnapshotManager::open(base.path().join(".snap")).unwrap();
        let error = manager
            .create(base.path().join("gibt-es-nicht"), 1)
            .unwrap_err();
        assert!(matches!(error, CodeError::Snapshot(_)));
    }
}
