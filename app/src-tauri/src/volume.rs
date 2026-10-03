//! Dateisystem und freier Platz eines Ordners (für die Vorprüfung von Agent Flow).
//!
//! exFAT und FAT32 (typisch für USB-Sticks) kennen weder Symlinks noch
//! Dateien über 4 GB; der freie Platz entscheidet, ob Worktrees überhaupt Platz
//! finden. Die Werte kommen vom Betriebssystem und werden nicht geschätzt.

use std::path::Path;

/// Angaben zum Datenträger eines Ordners.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeInfo {
    /// Name des Dateisystems, z. B. `NTFS` oder `exFAT`.
    pub filesystem: String,
    /// Für den Nutzer verfügbare freie Bytes.
    pub free_bytes: u64,
}

/// Fragt Dateisystem und freien Platz ab.
///
/// # Errors
/// Textmeldung des Betriebssystems, wenn der Ordner nicht abgefragt werden kann.
#[cfg(windows)]
pub fn query(path: &Path) -> Result<VolumeInfo, String> {
    use std::os::windows::ffi::OsStrExt;

    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            GetDiskFreeSpaceExW, GetVolumeInformationW, GetVolumePathNameW,
        },
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: alle Puffer sind lokal, die Längen stimmen, die Zeiger gelten für die Dauer des Aufrufs.
    unsafe {
        let mut free = 0_u64;
        GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None)
            .map_err(|e| e.message())?;
        let mut root = [0_u16; 260];
        GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut root).map_err(|e| e.message())?;
        let mut name = [0_u16; 64];
        GetVolumeInformationW(
            PCWSTR(root.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut name),
        )
        .map_err(|e| e.message())?;
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        Ok(VolumeInfo {
            filesystem: String::from_utf16_lossy(&name[..len]),
            free_bytes: free,
        })
    }
}

/// Auf nicht geprüften Plattformen gibt es keine Abfrage.
///
/// # Errors
/// Immer: die Plattform ist noch nicht geprüft.
#[cfg(not(windows))]
pub fn query(_path: &Path) -> Result<VolumeInfo, String> {
    Err("Die Datenträgerabfrage ist auf dieser Plattform noch nicht geprüft.".to_owned())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn temp_directory_reports_a_filesystem_and_free_space() {
        let info = query(&std::env::temp_dir()).expect("Abfrage");
        assert!(!info.filesystem.is_empty());
        assert!(info.free_bytes > 0);
    }
}
