//! Passwortwechsel eines geöffneten Tresors: alles oder nichts.

use pa_vault::{
    hot_copy::{HotCopyState, HotVault, NoFault, SyncFault, SyncFaultInjector},
    key::{derive_key, VaultKey},
    meta::{Argon2Parameters, VaultMeta},
    repository::VaultRepository,
    VaultError,
};

fn key(password: &str) -> VaultKey {
    derive_key(
        password,
        &VaultMeta::new(
            [7; 16],
            Argon2Parameters {
                memory_kib: 8192,
                iterations: 1,
                parallelism: 1,
            },
        ),
    )
    .expect("key")
}

/// Lässt das `n`-te Erreichen des Punkts scheitern. Der Schlüsselwechsel sichert erst den alten
/// Stand (1. Mal) und veröffentlicht dann den neuen (2. Mal); der Rollback-Test trifft das zweite.
struct FailNth {
    point: SyncFault,
    nth: u32,
    seen: u32,
}

impl SyncFaultInjector for FailNth {
    fn check(&mut self, point: SyncFault) -> Result<(), VaultError> {
        if point == self.point {
            self.seen += 1;
            if self.seen == self.nth {
                return Err(VaultError::InjectedFault(point.name()));
            }
        }
        Ok(())
    }
}

fn opens(path: &std::path::Path, password: &str) -> bool {
    VaultRepository::open(path, &key(password)).is_ok()
}

/// Ob die Hostkopie (die laufende Arbeitskopie im Temp-Ordner) zu diesem Passwort passt. Sie darf nie
/// unter einem anderen Schlüssel als dem aktuellen Passwort auf der Platte liegen.
fn hot_copy_opens(host: &std::path::Path, password: &str) -> bool {
    pa_vault::open_encrypted(&host.join("hot.db"), &key(password)).is_ok()
}

fn titles(path: &std::path::Path, password: &str) -> Vec<String> {
    VaultRepository::open(path, &key(password))
        .expect("öffnen")
        .conversations()
        .expect("Unterhaltungen")
        .into_iter()
        .map(|c| c.title)
        .collect()
}

#[test]
fn key_comparison_tells_the_session_key_from_another() {
    let temp = tempfile::tempdir().expect("temp");
    let vault = HotVault::start(
        &temp.path().join("usb/vault.db"),
        &temp.path().join("host"),
        key("alt"),
    )
    .expect("start");
    assert!(vault.key_matches(&key("alt")));
    assert!(!vault.key_matches(&key("falsch")));
}

#[test]
fn changing_the_key_republishes_the_vault_so_only_the_new_password_opens_it() {
    let temp = tempfile::tempdir().expect("temp");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut vault = HotVault::start(&portable, &host, key("alt")).expect("start");
    vault
        .repository_mut()
        .create_conversation("c1", "vorher", 1)
        .expect("schreiben");
    vault.sync(&mut NoFault).expect("sync");

    vault.change_key(key("neu"), &mut NoFault).expect("Wechsel");

    assert!(
        !opens(&portable, "alt"),
        "das alte Passwort darf nicht mehr gehen"
    );
    assert!(
        hot_copy_opens(&host, "neu"),
        "die Hostkopie gehört jetzt zum neuen Passwort"
    );
    assert!(
        !hot_copy_opens(&host, "alt"),
        "die Hostkopie darf nicht beim alten Passwort bleiben"
    );
    assert_eq!(
        titles(&portable, "neu"),
        ["vorher"],
        "Daten bleiben erhalten"
    );
    assert!(vault.key_matches(&key("neu")));
    assert_eq!(vault.state(), HotCopyState::Clean);

    // Die Sitzung läuft weiter und schreibt mit dem neuen Schlüssel.
    vault
        .repository_mut()
        .create_conversation("c2", "danach", 2)
        .expect("schreiben");
    vault.sync(&mut NoFault).expect("sync");
    let mut after = titles(&portable, "neu");
    after.sort();
    assert_eq!(after, ["danach", "vorher"]);

    // Und nach einem Neustart genügt das neue Passwort.
    vault.shutdown(&mut NoFault).expect("beenden");
    let restarted = HotVault::start(&portable, &host, key("neu")).expect("Neustart");
    assert_eq!(
        restarted.repository().conversations().expect("lesen").len(),
        2
    );
    assert!(HotVault::start(&portable, &host, key("alt")).is_err());
}

#[test]
fn an_unreachable_stick_refuses_the_change_and_keeps_the_old_password() {
    let temp = tempfile::tempdir().expect("temp");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut vault = HotVault::start(&portable, &host, key("alt")).expect("start");
    vault
        .repository_mut()
        .create_conversation("c1", "ungesichert", 1)
        .expect("schreiben");
    vault.set_media_available(false);

    assert!(vault.change_key(key("neu"), &mut NoFault).is_err());
    assert!(
        vault.key_matches(&key("alt")),
        "der Sitzungsschlüssel bleibt der alte"
    );

    vault.set_media_available(true);
    vault.sync(&mut NoFault).expect("sync");
    assert!(opens(&portable, "alt"));
    assert!(!opens(&portable, "neu"));
    assert_eq!(titles(&portable, "alt"), ["ungesichert"]);
}

#[test]
fn a_failed_publish_rolls_back_to_the_old_password_and_keeps_the_session_usable() {
    let temp = tempfile::tempdir().expect("temp");
    let portable = temp.path().join("usb/vault.db");
    let host = temp.path().join("host");
    let mut vault = HotVault::start(&portable, &host, key("alt")).expect("start");
    vault
        .repository_mut()
        .create_conversation("c1", "eins", 1)
        .expect("schreiben");
    vault.sync(&mut NoFault).expect("sync");

    let mut fault = FailNth {
        point: SyncFault::BeforeRename,
        nth: 2,
        seen: 0,
    };
    let result = vault.change_key(key("neu"), &mut fault);
    assert!(result.is_err());
    assert_eq!(
        fault.seen, 2,
        "der Fehler muss beim Veröffentlichen des neuen Stands auftreten, nicht davor"
    );

    // Auf dem Stick gilt weiter das alte Passwort, und nichts ging verloren.
    assert!(vault.key_matches(&key("alt")));
    assert!(opens(&portable, "alt"));
    assert!(!opens(&portable, "neu"));
    assert!(
        hot_copy_opens(&host, "alt"),
        "nach dem Rollback gilt auch für die Hostkopie das alte Passwort"
    );
    assert!(!hot_copy_opens(&host, "neu"));
    // Die laufende Sitzung funktioniert und kann mit dem alten Schlüssel weiter sichern.
    vault
        .repository_mut()
        .create_conversation("c2", "zwei", 2)
        .expect("schreiben nach dem Rollback");
    vault.sync(&mut NoFault).expect("sync nach dem Rollback");
    let mut all = titles(&portable, "alt");
    all.sort();
    assert_eq!(all, ["eins", "zwei"]);
}
