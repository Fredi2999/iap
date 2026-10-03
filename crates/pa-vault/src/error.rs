use std::{io, path::PathBuf};

use thiserror::Error;

/// Bewahrt Verschlüsselungs-, Integritäts- und Medienfehler als getrennte Fälle.
#[derive(Debug, Error)]
pub enum VaultError {
    #[error("Vault-Dateizugriff `{path}` fehlgeschlagen: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Vault-Metadaten sind ungültig: {0}")]
    InvalidMetadata(String),
    #[error("Argon2id-Schlüsselableitung fehlgeschlagen: {0}")]
    KeyDerivation(String),
    #[error("SQLCipher-Operation fehlgeschlagen: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Vault-Passphrase ist falsch oder die Datenbank ist beschädigt")]
    Authentication,
    #[error("Vault-Integritätsprüfung fehlgeschlagen: {0}")]
    Integrity(String),
    #[error("injizierter Synchronisationsfehler bei {0}")]
    InjectedFault(&'static str),
    #[error("portables Vault-Medium ist nicht verfügbar")]
    MediaUnavailable,
    #[error(
        "Host-Recovery Generation {host_generation} weicht vom portablen Stand {portable_generation} ab"
    )]
    RecoveryChoiceRequired {
        portable_generation: u64,
        host_generation: u64,
    },
}
