use std::{
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use rusqlite::backup::Backup;
use sha2::{Digest, Sha256};

use crate::{
    key::VaultKey,
    open_encrypted,
    repository::{VaultIdentity, VaultRepository},
    VaultError,
};

const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
const COPY_BUFFER_BYTES: usize = 1024 * 1024;

/// Macht den ausfallsicheren Synchronisationsfortschritt für UI und Recovery sichtbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotCopyState {
    Clean,
    Dirty,
    Syncing,
    PendingMedia,
    Recoverable,
}

/// Verlangt bei auseinanderlaufenden intakten Kopien eine bewusste Datenwahl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryMode {
    RequireConfirmation,
    UseNewestHost,
    UsePortable,
}

/// Definiert exakt die Grenzen, an denen Crash- und Medienfehler simuliert werden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncFault {
    BeforeFlush,
    BeforeRename,
    AfterRename,
}

impl SyncFault {
    /// Liefert einen stabilen Diagnosewert für Test- und Fehlerberichte.
    pub fn name(self) -> &'static str {
        match self {
            Self::BeforeFlush => "before_flush",
            Self::BeforeRename => "before_rename",
            Self::AfterRename => "after_rename",
        }
    }
}

/// Injiziert nur Fehlergrenzen und kann dadurch keine alternative Sync-Implementierung einschleusen.
pub trait SyncFaultInjector {
    fn check(&mut self, point: SyncFault) -> Result<(), VaultError>;
}

/// Ist der explizite Produktionspfad ohne Testfehler.
pub struct NoFault;

impl SyncFaultInjector for NoFault {
    fn check(&mut self, _point: SyncFault) -> Result<(), VaultError> {
        Ok(())
    }
}

/// Besitzt ausschließlich die geöffnete Hostkopie; das portable Original bleibt stets geschlossen.
pub struct HotVault {
    repository: VaultRepository,
    key: VaultKey,
    portable_path: PathBuf,
    hot_path: PathBuf,
    recovery_path: PathBuf,
    state: HotCopyState,
    last_sync: Instant,
    media_available: bool,
}

impl HotVault {
    /// Bricht bei einer neueren Hostkopie ab, bis deren Verwendung ausdrücklich bestätigt wurde.
    pub fn start(
        portable_path: &Path,
        host_directory: &Path,
        key: VaultKey,
    ) -> Result<Self, VaultError> {
        Self::start_with_recovery(
            portable_path,
            host_directory,
            key,
            RecoveryMode::RequireConfirmation,
        )
    }

    /// Wendet die ausdrückliche Recovery-Entscheidung an und bewahrt die nicht gewählte neuere Kopie.
    pub fn start_with_recovery(
        portable_path: &Path,
        host_directory: &Path,
        key: VaultKey,
        recovery_mode: RecoveryMode,
    ) -> Result<Self, VaultError> {
        fs::create_dir_all(host_directory).map_err(|source| VaultError::Io {
            path: host_directory.to_path_buf(),
            source,
        })?;
        let hot_path = host_directory.join("hot.db");
        let recovery_path = host_directory.join("recovery.db");
        let conflict_directory = host_directory.join("conflicts");
        let (selected, recovered) = select_valid_candidate(
            portable_path,
            &hot_path,
            &recovery_path,
            &conflict_directory,
            &key,
            recovery_mode,
        )?;
        if let Some(selected) = selected {
            if selected != hot_path {
                copy_file_synced(&selected, &hot_path)?;
            }
        }
        let repository = VaultRepository::open(&hot_path, &key)?;
        repository.check_integrity()?;
        Ok(Self {
            repository,
            key,
            portable_path: portable_path.to_path_buf(),
            hot_path,
            recovery_path,
            state: if recovered {
                HotCopyState::Recoverable
            } else {
                HotCopyState::Clean
            },
            last_sync: Instant::now(),
            media_available: true,
        })
    }

    /// Ob `other` der Schlüssel dieser geöffneten Sitzung ist (konstante Zeit). Damit lässt sich ein
    /// eingegebenes Passwort prüfen, ohne die Datenbank erneut zu öffnen.
    pub fn key_matches(&self, other: &VaultKey) -> bool {
        self.key.matches(other)
    }

    /// Wechselt den Schlüssel des geöffneten Tresors, alles oder nichts.
    ///
    /// Warum so: Der Stick darf nie halb umgestellt sein, sonst wäre der Tresor unlesbar. Deshalb
    /// wird zuerst der bisherige Stand gesichert (ohne erreichbaren Stick gibt es keinen Wechsel),
    /// dann die Hostkopie umgeschlüsselt und über die gewohnte, geprüfte Synchronisierung auf dem
    /// Stick veröffentlicht. Scheitert das Veröffentlichen und liegt auf dem Stick noch der alte
    /// Stand, wird die Hostkopie auf den alten Schlüssel zurückgesetzt. Die Metadaten (Salt,
    /// Kosten) bleiben unverändert; der neue Schlüssel entsteht aus dem neuen Passwort mit demselben Salt.
    ///
    /// # Errors
    /// Fehler des vorbereitenden Sicherns, des Umschlüsselns oder des Veröffentlichens; in jedem
    /// Fehlerfall gilt weiter der bisherige Schlüssel.
    pub fn change_key(
        &mut self,
        new_key: VaultKey,
        injector: &mut dyn SyncFaultInjector,
    ) -> Result<(), VaultError> {
        // 1. Der Stick muss den aktuellen Stand haben und erreichbar sein.
        self.state = HotCopyState::Dirty;
        self.sync(injector)?;
        // 2. Hostkopie umschlüsseln.
        let old_key = self.key.duplicate();
        rekey(self.repository.connection(), &new_key)?;
        self.key = new_key;
        self.state = HotCopyState::Dirty;
        // 3. Auf dem Stick veröffentlichen.
        match self.sync(injector) {
            Ok(()) => Ok(()),
            Err(error) => {
                // Steht der neue Stand trotz des Fehlers schon vollständig auf dem Stick, ist der
                // Wechsel gelungen (nur ein Nachlauf scheiterte); ein Zurücksetzen würde Stick und
                // Hostkopie auseinanderlaufen lassen.
                if VaultRepository::inspect_identity(&self.portable_path, &self.key).is_ok() {
                    self.state = HotCopyState::Dirty;
                    return Ok(());
                }
                rekey(self.repository.connection(), &old_key).map_err(|rollback| {
                    VaultError::Integrity(format!(
                        "Schlüsselwechsel gescheitert ({error}) und Rücksetzen nicht möglich ({rollback}); Sicherung des Sticks verwenden"
                    ))
                })?;
                self.key = old_key;
                self.state = HotCopyState::Dirty;
                Err(error)
            }
        }
    }

    /// Gibt nur lesenden Zugriff frei, ohne den Dirty-Zustand fälschlich zu verändern.
    pub fn repository(&self) -> &VaultRepository {
        &self.repository
    }

    /// Markiert vor jedem mutablen Zugriff vorsorglich dirty, damit kein erfolgreicher Write verloren geht.
    pub fn repository_mut(&mut self) -> &mut VaultRepository {
        self.state = HotCopyState::Dirty;
        &mut self.repository
    }

    /// Markiert Änderungen explizit, nachdem eine Repository-Transaktion erfolgreich war.
    pub fn mark_dirty(&mut self) {
        self.state = HotCopyState::Dirty;
    }

    /// Liefert den aktuellen Sync-/Recovery-Zustand ohne Dateisystemheuristik.
    pub fn state(&self) -> HotCopyState {
        self.state
    }

    /// Verwendet eine monotone Uhr, damit Systemzeitänderungen keinen Fünf-Minuten-Sync verhindern.
    pub fn sync_due(&self, now: Instant) -> bool {
        self.state == HotCopyState::Dirty
            && now.saturating_duration_since(self.last_sync) >= SYNC_INTERVAL
    }

    /// Stellt dem Worker denselben monotonen Fälligkeitspunkt bereit, den der Zustandsautomat prüft.
    pub fn next_sync_deadline(&self) -> Instant {
        self.last_sync + SYNC_INTERVAL
    }

    /// Erlaubt dem Medienbeobachter, ein abgezogenes Laufwerk vor Schreibversuchen zu melden.
    pub fn set_media_available(&mut self, available: bool) {
        self.media_available = available;
    }

    /// Erstellt erst eine geprüfte Host-Recovery und veröffentlicht dann atomar auf dem Stick.
    pub fn sync(&mut self, injector: &mut dyn SyncFaultInjector) -> Result<(), VaultError> {
        if self.state == HotCopyState::Clean {
            return Ok(());
        }
        self.state = HotCopyState::Syncing;
        let result = self.sync_inner(injector);
        match result {
            Ok(()) => {
                self.state = HotCopyState::Clean;
                self.last_sync = Instant::now();
                Ok(())
            }
            Err(error) => {
                self.state =
                    if matches!(error, VaultError::MediaUnavailable | VaultError::Io { .. }) {
                        HotCopyState::PendingMedia
                    } else {
                        HotCopyState::Recoverable
                    };
                Err(error)
            }
        }
    }

    fn sync_inner(&mut self, injector: &mut dyn SyncFaultInjector) -> Result<(), VaultError> {
        self.repository
            .connection()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        let recovery_next = self.recovery_path.with_extension("db.next");
        if recovery_next.exists() {
            fs::remove_file(&recovery_next).map_err(|source| VaultError::Io {
                path: recovery_next.clone(),
                source,
            })?;
        }
        create_encrypted_snapshot(self.repository.connection(), &self.key, &recovery_next)?;
        replace(&recovery_next, &self.recovery_path)?;

        if !self.media_available {
            return Err(VaultError::MediaUnavailable);
        }
        let parent = self.portable_path.parent().ok_or_else(|| {
            VaultError::Integrity("portabler Vault-Pfad hat kein Elternverzeichnis".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(|source| VaultError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        let portable_partial = self.portable_path.with_extension("db.partial");
        copy_file(&self.recovery_path, &portable_partial, false)?;
        injector.check(SyncFault::BeforeFlush)?;
        File::options()
            .write(true)
            .open(&portable_partial)
            .and_then(|file| file.sync_all())
            .map_err(|source| VaultError::Io {
                path: portable_partial.clone(),
                source,
            })?;
        let candidate = VaultRepository::open(&portable_partial, &self.key)?;
        candidate.check_integrity()?;
        drop(candidate);
        injector.check(SyncFault::BeforeRename)?;
        replace(&portable_partial, &self.portable_path)?;
        injector.check(SyncFault::AfterRename)?;
        let published = VaultRepository::open(&self.portable_path, &self.key)?;
        published.check_integrity()?;
        drop(published);
        fs::remove_file(&self.recovery_path).map_err(|source| VaultError::Io {
            path: self.recovery_path.clone(),
            source,
        })?;
        Ok(())
    }

    /// Macht den tatsächlichen Hostpfad nur für Bereinigung und Diagnose sichtbar.
    pub fn hot_path(&self) -> &Path {
        &self.hot_path
    }

    /// Synchronisiert beim Beenden und bereinigt Hot-Copy samt Sidecars nur nach bestätigtem Erfolg.
    pub fn shutdown(mut self, injector: &mut dyn SyncFaultInjector) -> Result<(), VaultError> {
        self.sync(injector)?;
        let paths = [
            self.hot_path.clone(),
            self.hot_path.with_extension("db-wal"),
            self.hot_path.with_extension("db-shm"),
        ];
        drop(self);
        for path in paths {
            crate::cleanup::best_effort_secure_delete(&path)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ValidCandidate {
    path: PathBuf,
    identity: VaultIdentity,
    portable_priority: u8,
}

fn select_valid_candidate(
    portable_path: &Path,
    hot_path: &Path,
    recovery_path: &Path,
    conflict_directory: &Path,
    key: &VaultKey,
    recovery_mode: RecoveryMode,
) -> Result<(Option<PathBuf>, bool), VaultError> {
    let mut valid = Vec::new();
    let mut last_error = None;
    let mut paths = vec![
        (portable_path.to_path_buf(), 2_u8),
        (hot_path.to_path_buf(), 1_u8),
        (recovery_path.to_path_buf(), 0_u8),
    ];
    if conflict_directory.exists() {
        let entries = fs::read_dir(conflict_directory).map_err(|source| VaultError::Io {
            path: conflict_directory.to_path_buf(),
            source,
        })?;
        let mut conflicts = Vec::new();
        for entry in entries {
            let path = entry
                .map_err(|source| VaultError::Io {
                    path: conflict_directory.to_path_buf(),
                    source,
                })?
                .path();
            if path.extension().is_some_and(|extension| extension == "db") {
                conflicts.push(path);
            }
        }
        conflicts.sort();
        paths.extend(conflicts.into_iter().map(|path| (path, 0_u8)));
    }
    for (path, portable_priority) in paths {
        if !path.exists() {
            continue;
        }
        match VaultRepository::inspect_identity(&path, key) {
            Ok(identity) => valid.push(ValidCandidate {
                path,
                identity,
                portable_priority,
            }),
            Err(error) => last_error = Some(error),
        }
    }
    let portable = valid
        .iter()
        .find(|candidate| candidate.path == portable_path);
    let divergent_host = portable.and_then(|portable| {
        valid
            .iter()
            .filter(|candidate| {
                candidate.path != portable_path && candidate.identity != portable.identity
            })
            .max_by_key(|candidate| (candidate.identity.generation, candidate.portable_priority))
    });
    let selected = if let (Some(portable), Some(host)) = (portable, divergent_host) {
        match recovery_mode {
            RecoveryMode::RequireConfirmation => {
                return Err(VaultError::RecoveryChoiceRequired {
                    portable_generation: portable.identity.generation,
                    host_generation: host.identity.generation,
                });
            }
            RecoveryMode::UseNewestHost => Some(host.path.clone()),
            RecoveryMode::UsePortable => Some(portable.path.clone()),
        }
    } else {
        valid
            .iter()
            .max_by_key(|entry| (entry.identity.generation, entry.portable_priority))
            .map(|candidate| candidate.path.clone())
    };
    if let Some(selected) = selected {
        let selected_identity = valid
            .iter()
            .find(|candidate| candidate.path == selected)
            .ok_or_else(|| VaultError::Integrity("gewählter Vaultstand fehlt".to_owned()))?
            .identity
            .clone();
        preserve_unselected_identities(&valid, &selected_identity, conflict_directory)?;
        let recovered = selected != portable_path;
        return Ok((Some(selected), recovered));
    }
    if let Some(error) = last_error {
        return Err(error);
    }
    Ok((None, false))
}

fn preserve_unselected_identities(
    candidates: &[ValidCandidate],
    selected: &VaultIdentity,
    conflict_directory: &Path,
) -> Result<(), VaultError> {
    let mut preserved = Vec::<VaultIdentity>::new();
    for candidate in candidates {
        if candidate.identity == *selected || preserved.contains(&candidate.identity) {
            continue;
        }
        fs::create_dir_all(conflict_directory).map_err(|source| VaultError::Io {
            path: conflict_directory.to_path_buf(),
            source,
        })?;
        let destination = conflict_directory.join(conflict_file_name(&candidate.identity));
        if candidate.path != destination {
            let partial = destination.with_extension("db.partial");
            copy_file_synced(&candidate.path, &partial)?;
            replace(&partial, &destination)?;
        }
        preserved.push(candidate.identity.clone());
    }
    Ok(())
}

fn conflict_file_name(identity: &VaultIdentity) -> String {
    let mut hasher = Sha256::new();
    hasher.update(identity.generation.to_le_bytes());
    hasher.update(identity.revision.as_bytes());
    format!("{}.db", hex::encode(hasher.finalize()))
}

fn create_encrypted_snapshot(
    source: &rusqlite::Connection,
    key: &VaultKey,
    destination_path: &Path,
) -> Result<(), VaultError> {
    let mut destination = open_encrypted(destination_path, key)?;
    {
        let backup = Backup::new(source, &mut destination)?;
        backup.run_to_completion(100, Duration::from_millis(0), None)?;
    }
    drop(destination);
    File::options()
        .write(true)
        .open(destination_path)
        .and_then(|file| file.sync_all())
        .map_err(|source| VaultError::Io {
            path: destination_path.to_path_buf(),
            source,
        })?;
    let verified = VaultRepository::open(destination_path, key)?;
    verified.check_integrity()?;
    Ok(())
}

/// Schlüsselt eine offene SQLCipher-Verbindung in einer Transaktion um und prüft danach, dass sie
/// lesbar ist. Der WAL-Stand wird vorher in die Hauptdatei geschrieben.
fn rekey(connection: &rusqlite::Connection, key: &VaultKey) -> Result<(), VaultError> {
    connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    // SAFETY: `connection` hält ein lebendes sqlite3-Handle; SQLCipher kopiert genau 32 Schlüssel-
    // bytes während des Aufrufs, der geliehene Schlüssel bleibt dabei gültig.
    let result = unsafe {
        rusqlite::ffi::sqlite3_rekey(
            connection.handle(),
            key.expose_for_sqlcipher().as_ptr().cast(),
            32,
        )
    };
    if result != rusqlite::ffi::SQLITE_OK {
        return Err(rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(result), None).into());
    }
    connection
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|_| VaultError::Authentication)?;
    Ok(())
}

fn copy_file_synced(source: &Path, destination: &Path) -> Result<(), VaultError> {
    copy_file(source, destination, true)
}

fn copy_file(source: &Path, destination: &Path, sync: bool) -> Result<(), VaultError> {
    let input = File::open(source).map_err(|source_error| VaultError::Io {
        path: source.to_path_buf(),
        source: source_error,
    })?;
    let output = File::create(destination).map_err(|source_error| VaultError::Io {
        path: destination.to_path_buf(),
        source: source_error,
    })?;
    let mut reader = BufReader::with_capacity(COPY_BUFFER_BYTES, input);
    let mut writer = BufWriter::with_capacity(COPY_BUFFER_BYTES, output);
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|source_error| VaultError::Io {
                path: source.to_path_buf(),
                source: source_error,
            })?;
        if count == 0 {
            break;
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|source_error| VaultError::Io {
                path: destination.to_path_buf(),
                source: source_error,
            })?;
    }
    writer.flush().map_err(|source_error| VaultError::Io {
        path: destination.to_path_buf(),
        source: source_error,
    })?;
    if sync {
        writer
            .get_ref()
            .sync_all()
            .map_err(|source_error| VaultError::Io {
                path: destination.to_path_buf(),
                source: source_error,
            })?;
    }
    Ok(())
}

#[cfg(windows)]
fn to_extended_wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let s = path.to_string_lossy();
    let extended = if s.starts_with(r"\\?\") {
        path.to_path_buf()
    } else if let Ok(abs) = if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir().map(|cwd| cwd.join(path))
    } {
        let abs_str = abs.to_string_lossy().replace('/', "\\");
        if abs_str.len() >= 3 && abs_str.as_bytes()[1] == b':' {
            PathBuf::from(format!(r"\\?\{}", abs_str))
        } else {
            abs
        }
    } else {
        path.to_path_buf()
    };
    extended.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn replace(source: &Path, destination: &Path) -> Result<(), VaultError> {
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
    };
    let source_wide = to_extended_wide(source);
    let destination_wide = to_extended_wide(destination);
    let res = unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if let Err(error) = res {
        if let Err(fallback_error) = fs::rename(source, destination) {
            return Err(VaultError::Io {
                path: destination.to_path_buf(),
                source: std::io::Error::other(format!(
                    "{error}; rename fallback: {fallback_error}"
                )),
            });
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace(source: &Path, destination: &Path) -> Result<(), VaultError> {
    fs::rename(source, destination).map_err(|source_error| VaultError::Io {
        path: destination.to_path_buf(),
        source: source_error,
    })
}
