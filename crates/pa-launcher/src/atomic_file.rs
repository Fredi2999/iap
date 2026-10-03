use std::path::Path;

use crate::LauncherError;

/// Ersetzt das Ziel auf Windows mit Write-Through, ohne eine sichtbare Lücke zu erzeugen.
#[cfg(windows)]
pub(crate) fn replace(source: &Path, destination: &Path) -> Result<(), LauncherError> {
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
    .map_err(|error| LauncherError::Io {
        action: "Datei atomar ersetzen",
        path: destination.to_path_buf(),
        source: std::io::Error::other(error),
    })
}

/// POSIX-rename ersetzt ein vorhandenes reguläres Ziel atomar.
#[cfg(not(windows))]
pub(crate) fn replace(source: &Path, destination: &Path) -> Result<(), LauncherError> {
    std::fs::rename(source, destination).map_err(|source_error| LauncherError::Io {
        action: "Datei atomar ersetzen",
        path: destination.to_path_buf(),
        source: source_error,
    })
}
