//! Verwaltung der Tresore: das Passwort des geöffneten Tresors ändern und einen Tresor im
//! Startbildschirm löschen.
//!
//! Beides sind Eingriffe mit dauerhafter Wirkung. Deshalb verlangt jeder Weg das aktuelle
//! Passwort des betroffenen Tresors, und das Löschen zusätzlich die Eingabe des Tresornamens.
//! Passwörter bleiben nur kurz im Speicher (`Zeroizing`) und tauchen nie in Meldungen oder im
//! Audit-Log auf.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use pa_policy::{path::resolve_absolute_in_scope, PathScope};
use pa_vault::{
    cleanup::best_effort_secure_delete,
    hot_copy::{HotVault, NoFault},
    key::derive_key,
    meta::VaultMeta,
    VaultError,
};
use tauri::{AppHandle, Manager};
use zeroize::Zeroizing;

use crate::{
    ensure_bootstrap, hot_vault_directory, lifecycle, require_session, AppError, AppResult,
    AppState,
};

/// Kürzestes neues Passwort. Beim Anlegen eines Tresors gilt keine Grenze (bestehende Tresore
/// behalten ihr Passwort); wer es ändert, wählt bewusst ein besseres.
const MIN_PASSWORD_CHARS: usize = 8;

fn invalid(text: impl Into<String>) -> AppError {
    AppError::Invalid(text.into())
}

/// Ändert das Passwort des offenen Tresors. Alles oder nichts: Auf dem Stick gilt danach entweder
/// das neue oder weiter das alte Passwort.
pub(crate) fn change_password_core(
    vault: &mut HotVault,
    meta: &VaultMeta,
    old_passphrase: &str,
    new_passphrase: &str,
) -> AppResult<()> {
    if new_passphrase.chars().count() < MIN_PASSWORD_CHARS {
        return Err(invalid(format!(
            "Das neue Passwort braucht mindestens {MIN_PASSWORD_CHARS} Zeichen."
        )));
    }
    if new_passphrase == old_passphrase {
        return Err(invalid(
            "Das neue Passwort muss sich vom aktuellen unterscheiden.",
        ));
    }
    let old_key = derive_key(old_passphrase, meta)?;
    if !vault.key_matches(&old_key) {
        return Err(invalid("Das aktuelle Passwort stimmt nicht."));
    }
    let new_key = derive_key(new_passphrase, meta)?;
    vault
        .change_key(new_key, &mut NoFault)
        .map_err(|error| match error {
            VaultError::MediaUnavailable | VaultError::Io { .. } => invalid(
                "Der Stick ist nicht erreichbar oder nicht beschreibbar. Das Passwort wurde nicht geändert.",
            ),
            other => AppError::from(other),
        })
}

/// Name einer Begleitdatei des Tresors, z. B. `vault.db` plus `-wal`.
fn sibling(vault: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = vault.file_name().unwrap_or_default().to_owned();
    name.push(suffix);
    vault.with_file_name(name)
}

/// Löscht einen Tresor mit allen Begleitdateien und der Arbeitskopie auf diesem PC.
///
/// Geprüft wird in dieser Reihenfolge: Der Pfad liegt direkt im Datenordner des Sticks (die
/// Pfadprüfung kommt aus `pa-policy`), der Tresor ist nicht gerade geöffnet, der eingegebene
/// Name stimmt, und das Passwort öffnet den Tresor wirklich. Erst dann wird gelöscht.
///
/// Was **nicht** gelöscht wird: Dateien im Arbeitsordner, Modelle und Pakete. Arbeitskopien auf
/// anderen PCs, auf denen der Stick benutzt wurde, kann IAP nicht erreichen.
pub(crate) fn delete_vault_core(
    data_dir: &Path,
    vault_path: &str,
    passphrase: &str,
    confirm_name: &str,
    active_vault: Option<&Path>,
    hot_directory: impl Fn(&Path) -> std::io::Result<PathBuf>,
) -> AppResult<()> {
    let scope = PathScope::new(data_dir).map_err(|_| invalid("Der Datenordner fehlt."))?;
    let resolved =
        resolve_absolute_in_scope(&scope, Path::new(vault_path.trim())).map_err(|_| {
            invalid("Dieser Tresor liegt nicht im Datenordner des Sticks und wird nicht gelöscht.")
        })?;
    let name = resolved
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let is_plain_vault = resolved.parent() == Some(scope.root())
        && resolved.extension().is_some_and(|e| e == "db")
        && !name.contains(".partial")
        && !name.contains(".next");
    if !is_plain_vault || !resolved.is_file() {
        return Err(invalid("Das ist kein Tresor in diesem Datenordner."));
    }
    if active_vault.is_some_and(|active| std::fs::canonicalize(active).is_ok_and(|a| a == resolved))
    {
        return Err(invalid(
            "Dieser Tresor ist gerade geöffnet. Beende IAP und lösche ihn im Startbildschirm.",
        ));
    }
    if confirm_name.trim() != name {
        return Err(invalid(
            "Der eingegebene Name stimmt nicht mit dem Tresor überein. Es wurde nichts gelöscht.",
        ));
    }
    let meta_path = resolved.with_extension("meta");
    let meta = VaultMeta::load(&meta_path).map_err(|_| {
        invalid("Zu diesem Tresor fehlt die vault.meta; ohne sie lässt sich das Passwort nicht prüfen. Es wurde nichts gelöscht.")
    })?;
    let key = derive_key(passphrase, &meta)?;
    pa_vault::verify_key(&resolved, &key)
        .map_err(|_| invalid("Das Passwort stimmt nicht. Der Tresor wurde nicht gelöscht."))?;

    // Bis hierher wurde nichts verändert. Ab jetzt wird gelöscht; Fehler einzelner Begleitdateien
    // brechen nicht ab, sondern werden am Ende gemeldet, damit möglichst nichts zurückbleibt.
    let mut failed: Vec<String> = Vec::new();
    for suffix in ["-wal", "-shm", ".partial", ".next"] {
        if let Err(error) = best_effort_secure_delete(&sibling(&resolved, suffix)) {
            failed.push(error.to_string());
        }
    }
    for path in [
        sibling(&meta_path, ".partial"),
        resolved.clone(),
        meta_path.clone(),
    ] {
        if let Err(error) = best_effort_secure_delete(&path) {
            failed.push(error.to_string());
        }
    }
    if let Ok(hot) = hot_directory(&resolved) {
        if hot.is_dir() {
            if let Err(error) = delete_tree_securely(&hot) {
                failed.push(error.to_string());
            }
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(AppError::Internal(format!(
            "Der Tresor ist gelöscht, aber einige Dateien blieben zurück: {}",
            failed.join("; ")
        )))
    }
}

/// Überschreibt und löscht alle Dateien unter `dir` bestmöglich und entfernt dann die Ordner.
fn delete_tree_securely(dir: &Path) -> Result<(), VaultError> {
    let mut stack = vec![dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(current) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => stack.push(path),
                Ok(_) => files.push(path),
                Err(_) => {}
            }
        }
    }
    for file in files {
        best_effort_secure_delete(&file)?;
    }
    std::fs::remove_dir_all(dir).map_err(|source| VaultError::Io {
        path: dir.to_path_buf(),
        source,
    })
}

/// Ändert das Passwort des geöffneten Tresors. Die Passwörter verlassen den Speicher nie.
#[tauri::command]
pub async fn change_vault_password(
    app: AppHandle,
    old_passphrase: String,
    new_passphrase: String,
) -> AppResult<()> {
    let old = Zeroizing::new(old_passphrase);
    let new = Zeroizing::new(new_passphrase);
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = worker.state::<AppState>();
        let session = require_session(&state)?;
        let meta = VaultMeta::load(&session.vault_path.with_extension("meta"))?;
        {
            let mut vault = session.vault_runtime.lock()?;
            change_password_core(&mut vault, &meta, &old, &new)?;
        }
        let ok = pa_policy::Decision::Allow(pa_policy::Capability {
            action: pa_policy::CapabilityAction::FileWrite,
            canonical_path: None,
        });
        lifecycle::audit_decision(
            &state,
            pa_policy::CapabilityAction::FileWrite,
            Some("Tresor".to_owned()),
            &ok,
            "Tresor-Passwort geändert",
        );
        Ok(())
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

/// Löscht einen Tresor im Startbildschirm, nach Passwort und Namensbestätigung.
#[tauri::command]
pub async fn delete_vault(
    app: AppHandle,
    vault_path: String,
    passphrase: String,
    confirm_name: String,
) -> AppResult<()> {
    let passphrase = Zeroizing::new(passphrase);
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = worker.state::<AppState>();
        let bootstrap = ensure_bootstrap(&state)?;
        let data_dir = bootstrap.package_root.join("AI").join("data");
        // Ein geöffneter Tresor darf nicht gelöscht werden.
        let active_vault = require_session(&state).ok().map(|s| s.vault_path.clone());
        let host_identifier = bootstrap.host_identifier.clone();
        delete_vault_core(
            &data_dir,
            &vault_path,
            &passphrase,
            &confirm_name,
            active_vault.as_deref(),
            |vault| hot_vault_directory(vault, &host_identifier),
        )
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_vault::{
        hot_copy::NoFault,
        meta::{Argon2Parameters, VaultMeta},
        repository::VaultRepository,
    };

    fn fast_meta() -> VaultMeta {
        VaultMeta::new(
            [3; 16],
            Argon2Parameters {
                memory_kib: 8192,
                iterations: 1,
                parallelism: 1,
            },
        )
    }

    /// Legt einen echten Tresor mit einer Unterhaltung an und lässt ihn geschlossen zurück.
    fn create_vault(data: &Path, name: &str, passphrase: &str) -> PathBuf {
        std::fs::create_dir_all(data).expect("Datenordner");
        let path = data.join(name);
        let meta = fast_meta();
        meta.save_atomic(&path.with_extension("meta"))
            .expect("meta");
        let hot = data.join(format!("hot-{name}"));
        let key = derive_key(passphrase, &meta).expect("Schlüssel");
        let mut vault = HotVault::start(&path, &hot, key).expect("start");
        vault
            .repository_mut()
            .create_conversation("c1", "geheim", 1)
            .expect("schreiben");
        vault.shutdown(&mut NoFault).expect("beenden");
        path
    }

    fn open(path: &Path, passphrase: &str) -> bool {
        let key = derive_key(passphrase, &fast_meta()).expect("Schlüssel");
        VaultRepository::open(path, &key).is_ok()
    }

    // ---------------------------------------------------------- Passwort ändern

    fn started(data: &Path, passphrase: &str) -> (HotVault, PathBuf, PathBuf) {
        let path = create_vault(data, "vault.db", passphrase);
        let hot = data.join("hot-live");
        let key = derive_key(passphrase, &fast_meta()).expect("Schlüssel");
        (HotVault::start(&path, &hot, key).expect("start"), path, hot)
    }

    #[test]
    fn the_password_is_changed_only_with_the_right_old_one_and_a_good_new_one() {
        let temp = tempfile::tempdir().expect("Temp");
        let (mut vault, path, _hot) = started(temp.path(), "altes-passwort");
        let meta = fast_meta();

        let wrong = change_password_core(&mut vault, &meta, "falsch-falsch", "neues-passwort")
            .unwrap_err()
            .to_string();
        assert!(wrong.contains("aktuelle Passwort stimmt nicht"), "{wrong}");
        let short = change_password_core(&mut vault, &meta, "altes-passwort", "kurz")
            .unwrap_err()
            .to_string();
        assert!(short.contains("mindestens 8 Zeichen"), "{short}");
        let same = change_password_core(&mut vault, &meta, "altes-passwort", "altes-passwort")
            .unwrap_err()
            .to_string();
        assert!(same.contains("unterscheiden"), "{same}");
        // Nach allen Ablehnungen gilt noch das alte Passwort.
        vault.sync(&mut NoFault).expect("sync");
        assert!(open(&path, "altes-passwort"));

        change_password_core(&mut vault, &meta, "altes-passwort", "neues-passwort")
            .expect("Wechsel");
        assert!(!open(&path, "altes-passwort"));
        assert!(open(&path, "neues-passwort"));
        // Die Daten sind unverändert.
        let key = derive_key("neues-passwort", &meta).expect("Schlüssel");
        let repo = VaultRepository::open(&path, &key).expect("öffnen");
        assert_eq!(repo.conversations().expect("lesen")[0].title, "geheim");
    }

    #[test]
    fn an_unreachable_stick_leaves_the_old_password_in_place_with_a_clear_message() {
        let temp = tempfile::tempdir().expect("Temp");
        let (mut vault, path, _hot) = started(temp.path(), "altes-passwort");
        vault.set_media_available(false);
        let message =
            change_password_core(&mut vault, &fast_meta(), "altes-passwort", "neues-passwort")
                .unwrap_err()
                .to_string();
        assert!(message.contains("nicht geändert"), "{message}");
        vault.set_media_available(true);
        vault.sync(&mut NoFault).expect("sync");
        assert!(open(&path, "altes-passwort"));
        assert!(!open(&path, "neues-passwort"));
    }

    // ---------------------------------------------------------- Tresor löschen

    fn no_hot(_: &Path) -> std::io::Result<PathBuf> {
        Err(std::io::Error::other("keine Arbeitskopie"))
    }

    #[test]
    fn a_vault_is_deleted_with_all_companions_only_after_password_and_name() {
        let temp = tempfile::tempdir().expect("Temp");
        let data = temp.path().join("data");
        let doomed = create_vault(&data, "arbeit.db", "pw-arbeit-1");
        let other = create_vault(&data, "privat.db", "pw-privat-1");
        std::fs::write(sibling(&doomed, "-wal"), b"x").expect("wal");
        std::fs::write(sibling(&doomed, ".partial"), b"x").expect("partial");
        let hot_dir = temp.path().join("hot-arbeit");
        std::fs::create_dir_all(hot_dir.join("conflicts")).expect("hot");
        std::fs::write(hot_dir.join("hot.db"), b"geheim").expect("hot.db");
        std::fs::write(hot_dir.join("conflicts/a.db"), b"geheim").expect("conflict");
        let hot_for = hot_dir.clone();
        let hot = move |_: &Path| Ok(hot_for.clone());
        let path = doomed.to_string_lossy().into_owned();

        // Falsches Passwort und falscher Name verändern nichts.
        let wrong_pw = delete_vault_core(&data, &path, "falsch-falsch", "arbeit.db", None, &hot)
            .unwrap_err()
            .to_string();
        assert!(wrong_pw.contains("Passwort stimmt nicht"), "{wrong_pw}");
        let wrong_name = delete_vault_core(&data, &path, "pw-arbeit-1", "anderer.db", None, &hot)
            .unwrap_err()
            .to_string();
        assert!(wrong_name.contains("Name stimmt nicht"), "{wrong_name}");
        assert!(doomed.exists() && doomed.with_extension("meta").exists() && hot_dir.exists());

        delete_vault_core(&data, &path, "pw-arbeit-1", "arbeit.db", None, &hot).expect("löschen");
        assert!(!doomed.exists());
        assert!(!doomed.with_extension("meta").exists());
        assert!(!sibling(&doomed, "-wal").exists() && !sibling(&doomed, ".partial").exists());
        assert!(!hot_dir.exists(), "die Arbeitskopie auf diesem PC ist weg");
        // Der andere Tresor ist unberührt.
        assert!(open(&other, "pw-privat-1"));
    }

    #[test]
    fn only_vaults_directly_in_the_data_folder_can_be_deleted_never_other_files() {
        let temp = tempfile::tempdir().expect("Temp");
        let data = temp.path().join("data");
        let vault = create_vault(&data, "vault.db", "passwort-1");
        let outside = create_vault(&temp.path().join("anderswo"), "fremd.db", "passwort-1");
        std::fs::write(data.join("notiz.txt"), b"wichtig").expect("Datei");
        std::fs::create_dir_all(data.join("tief")).expect("Ordner");
        let inner = create_vault(&data.join("tief"), "innen.db", "passwort-1");

        for (target, name) in [
            (outside.to_string_lossy().into_owned(), "fremd.db"),
            (
                format!("{}/../anderswo/fremd.db", data.display()),
                "fremd.db",
            ),
            (
                data.join("notiz.txt").to_string_lossy().into_owned(),
                "notiz.txt",
            ),
            (data.to_string_lossy().into_owned(), "data"),
            (inner.to_string_lossy().into_owned(), "innen.db"),
            (
                sibling(&vault, ".partial").to_string_lossy().into_owned(),
                "vault.db.partial",
            ),
        ] {
            let result = delete_vault_core(&data, &target, "passwort-1", name, None, no_hot);
            assert!(result.is_err(), "{target} darf nicht gelöscht werden");
        }
        assert!(outside.exists() && inner.exists() && vault.exists());
        assert_eq!(
            std::fs::read(data.join("notiz.txt")).expect("lesen"),
            b"wichtig"
        );
    }

    #[test]
    fn the_vault_that_is_currently_open_cannot_be_deleted() {
        let temp = tempfile::tempdir().expect("Temp");
        let data = temp.path().join("data");
        let vault = create_vault(&data, "vault.db", "passwort-1");
        let error = delete_vault_core(
            &data,
            &vault.to_string_lossy(),
            "passwort-1",
            "vault.db",
            Some(&vault),
            no_hot,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("gerade geöffnet"), "{error}");
        assert!(vault.exists());
    }

    #[test]
    fn a_vault_without_its_meta_file_is_not_deleted_blindly() {
        let temp = tempfile::tempdir().expect("Temp");
        let data = temp.path().join("data");
        let vault = create_vault(&data, "vault.db", "passwort-1");
        std::fs::remove_file(vault.with_extension("meta")).expect("meta weg");
        let error = delete_vault_core(
            &data,
            &vault.to_string_lossy(),
            "passwort-1",
            "vault.db",
            None,
            no_hot,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("vault.meta"), "{error}");
        assert!(vault.exists());
    }
}
