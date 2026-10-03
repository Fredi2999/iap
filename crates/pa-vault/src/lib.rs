//! Verschlüsselter, ausfallsicher synchronisierter Datenspeicher für PortableAI.

use std::path::Path;

use rusqlite::{ffi, Connection, OpenFlags};

pub mod agent_flow_store;
pub mod agent_run;
pub mod audit;
pub mod cleanup;
pub mod error;
pub mod flow_store;
pub mod hot_copy;
pub mod key;
pub mod library;
pub mod memory;
pub mod meta;
pub mod pci;
pub mod repository;
pub mod schema;
pub mod sync_worker;

pub use error::VaultError;
use key::VaultKey;

/// Öffnet ausschließlich SQLCipher und prüft den Schlüssel vor jeder weiteren Schemaoperation.
pub fn open_encrypted(path: &Path, key: &VaultKey) -> Result<Connection, VaultError> {
    open_encrypted_with_flags(
        path,
        key,
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )
}

/// Prüft, ob `key` den Tresor in `path` öffnet, ohne ihn zu verändern (nur lesend, keine
/// Migration). Gebraucht für Passwortabfragen vor dem Löschen eines Tresors.
///
/// # Errors
/// `VaultError::Authentication`, wenn der Schlüssel nicht passt, oder ein Dateifehler.
pub fn verify_key(path: &Path, key: &VaultKey) -> Result<(), VaultError> {
    open_encrypted_read_only(path, key).map(|_| ())
}

pub(crate) fn open_encrypted_read_only(
    path: &Path,
    key: &VaultKey,
) -> Result<Connection, VaultError> {
    open_encrypted_with_flags(path, key, path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn open_encrypted_with_flags(
    path: &Path,
    key: &VaultKey,
    error_path: &Path,
    flags: OpenFlags,
) -> Result<Connection, VaultError> {
    let connection = Connection::open_with_flags(path, flags).map_err(|source| VaultError::Io {
        path: error_path.to_path_buf(),
        source: std::io::Error::other(source),
    })?;
    // SAFETY: `connection` owns a live sqlite3 handle and SQLCipher copies the
    // exactly 32 key bytes during this call; the borrowed key remains alive.
    let key_result = unsafe {
        ffi::sqlite3_key(
            connection.handle(),
            key.expose_for_sqlcipher().as_ptr().cast(),
            32,
        )
    };
    if key_result != ffi::SQLITE_OK {
        return Err(rusqlite::Error::SqliteFailure(ffi::Error::new(key_result), None).into());
    }
    // Auf nicht erhöhten Windows-Prozessen scheiterte VirtualLock reproduzierbar;
    // AES-Seitenverschlüsselung und das zeroized Rust-Schlüsselmaterial bleiben aktiv.
    connection.execute_batch("PRAGMA cipher_memory_security = OFF;")?;
    connection
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|_| VaultError::Authentication)?;
    Ok(connection)
}
