//! Welche Ordner auf dem PC als Arbeitsordner des Code-Bereichs freigegeben werden dürfen.
//!
//! Der Nutzer wählt einen Ordner selbst und bestätigt die Freigabe; diese Regeln verhindern
//! trotzdem die Fehlgriffe, die schwer zu überblicken sind: ein ganzes Laufwerk, das gesamte
//! Benutzerprofil, Systemordner, Ordner mit Zugangsdaten oder der Stick selbst. Die Prüfung
//! arbeitet auf dem kanonischen Pfad, damit Verknüpfungen und Junctions nicht an ihr vorbei auf
//! einen gesperrten Ort zeigen können.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Warum ein Ordner nicht als Arbeitsordner taugt.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HostRootError {
    #[error("Der Ordner existiert nicht.")]
    NotFound,
    #[error("Das ist kein Ordner.")]
    NotADirectory,
    #[error("Ein ganzes Laufwerk lässt sich nicht freigeben. Wähle den Projektordner.")]
    DriveRoot,
    #[error("Dieser Ordner ist gesperrt: {reason}")]
    Protected { reason: String },
}

#[derive(Debug, Clone)]
struct Protected {
    path: PathBuf,
    /// Auch alles darunter ist gesperrt (System-, Programm-, Zugangsdatenordner). Sonst sind nur
    /// der Ordner selbst und seine übergeordneten Ordner gesperrt (z. B. das Benutzerprofil: der
    /// Projektordner darunter ist erlaubt, das Profil als Ganzes nicht).
    inside_too: bool,
    reason: &'static str,
}

/// Die Sperrliste für Arbeitsordner auf dem Host.
#[derive(Debug, Clone, Default)]
pub struct HostRootRules {
    protected: Vec<Protected>,
}

/// Kanonischer Pfad als Vergleichsschlüssel: unter Windows ohne Groß-/Kleinschreibung und mit
/// einheitlichen Trennern, ohne das `\\?\`-Präfix der Kanonisierung.
fn key(path: &Path) -> Vec<String> {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_owned();
    let text = if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    };
    text.split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn is_prefix(prefix: &[String], whole: &[String]) -> bool {
    whole.len() >= prefix.len() && whole[..prefix.len()] == *prefix
}

impl HostRootRules {
    /// Leere Regeln; nur Laufwerkswurzeln und Nicht-Ordner werden abgelehnt.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sperrt `path` samt allem darunter. Nicht vorhandene Pfade werden ignoriert, weil es
    /// dort ohnehin nichts freizugeben gibt.
    pub fn protect_tree(mut self, path: impl AsRef<Path>, reason: &'static str) -> Self {
        self.push(path.as_ref(), true, reason);
        self
    }

    /// Sperrt `path` und seine übergeordneten Ordner, nicht aber Ordner darunter.
    pub fn protect_only_self_and_parents(
        mut self,
        path: impl AsRef<Path>,
        reason: &'static str,
    ) -> Self {
        self.push(path.as_ref(), false, reason);
        self
    }

    fn push(&mut self, path: &Path, inside_too: bool, reason: &'static str) {
        if let Ok(canonical) = std::fs::canonicalize(path) {
            self.protected.push(Protected {
                path: canonical,
                inside_too,
                reason,
            });
        }
    }

    /// Regeln für diesen PC aus den Umgebungsvariablen (Benutzerprofil, Windows, Programme,
    /// AppData) plus die Orte von IAP selbst, die nie Arbeitsordner sein dürfen.
    pub fn from_environment(iap_paths: &[PathBuf]) -> Self {
        let mut rules = Self::new();
        let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
        for (name, reason) in [
            ("WINDIR", "Systemordner"),
            ("ProgramFiles", "Programmordner"),
            ("ProgramFiles(x86)", "Programmordner"),
            ("ProgramData", "Programmdaten"),
            ("APPDATA", "Anwendungsdaten (enthalten Zugangsdaten)"),
            ("LOCALAPPDATA", "Anwendungsdaten (enthalten Zugangsdaten)"),
        ] {
            if let Some(path) = env(name) {
                rules = rules.protect_tree(path, reason);
            }
        }
        if let Some(profile) = env("USERPROFILE").or_else(|| env("HOME")) {
            for secret in [".ssh", ".gnupg", ".aws", ".azure", ".kube", ".config"] {
                rules = rules.protect_tree(profile.join(secret), "Ordner mit Zugangsdaten");
            }
            rules = rules.protect_only_self_and_parents(
                profile,
                "das ganze Benutzerprofil (wähle einen Projektordner darin)",
            );
        }
        for path in iap_paths {
            rules = rules.protect_tree(path, "der IAP-Stick selbst");
        }
        rules
    }

    /// Prüft den Kandidaten und liefert den kanonischen Pfad, mit dem weitergearbeitet wird.
    pub fn validate(&self, candidate: &Path) -> Result<PathBuf, HostRootError> {
        let canonical = std::fs::canonicalize(candidate).map_err(|_| HostRootError::NotFound)?;
        if !canonical.is_dir() {
            return Err(HostRootError::NotADirectory);
        }
        let candidate_key = key(&canonical);
        // Ein Laufwerk (`C:`) oder `/`: höchstens eine Komponente.
        if candidate_key.len() <= 1 {
            return Err(HostRootError::DriveRoot);
        }
        for protected in &self.protected {
            let protected_key = key(&protected.path);
            let is_same_or_parent = is_prefix(&candidate_key, &protected_key);
            let is_inside = protected.inside_too && is_prefix(&protected_key, &candidate_key);
            if is_same_or_parent || is_inside {
                return Err(HostRootError::Protected {
                    reason: protected.reason.to_owned(),
                });
            }
        }
        Ok(canonical)
    }
}

/// Ob ein relativer Pfad auf Zugangsdaten oder Schlüsselmaterial zeigt. Das Modell liest und
/// ändert solche Dateien nie, auch nicht im freigegebenen Projektordner: Ein Fremdtext darin
/// (Prompt-Injection) soll keine Geheimnisse in Vorschläge oder Befehle tragen können.
/// Gemeint sind nur die Dateien des Modells; der Nutzer öffnet sie im Editor wie jede andere.
pub fn is_secret_path(relative: &str) -> bool {
    relative
        .replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty())
        .any(|part| {
            let name = part.to_lowercase();
            let is_example = name.ends_with(".example") || name.ends_with(".sample");
            if name == ".env" || (name.starts_with(".env.") && !is_example) {
                return true;
            }
            if matches!(
                name.as_str(),
                ".npmrc"
                    | ".pypirc"
                    | ".netrc"
                    | ".git-credentials"
                    | "credentials"
                    | "credentials.json"
                    | ".ssh"
                    | ".gnupg"
                    | ".aws"
            ) {
                return true;
            }
            if ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"]
                .iter()
                .any(|prefix| name.starts_with(prefix))
            {
                return true;
            }
            [".pem", ".key", ".p12", ".pfx", ".kdbx", ".keystore", ".jks"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_files_are_recognised_by_name_not_by_content() {
        for secret in [
            ".env",
            "app/.env",
            ".env.production",
            "keys/server.pem",
            "deploy\\id_rsa",
            "id_ed25519.pub",
            "wallet.kdbx",
            "home/.ssh/config",
            ".NPMRC",
            "x/credentials.json",
        ] {
            assert!(is_secret_path(secret), "{secret}");
        }
        for fine in [
            "src/main.rs",
            ".env.example",
            "env.rs",
            "README.md",
            "docs/key-concepts.md",
            "monkey.rs",
        ] {
            assert!(!is_secret_path(fine), "{fine}");
        }
    }

    fn make(root: &Path, relative: &str) -> PathBuf {
        let path = root.join(relative);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn a_normal_project_folder_is_accepted_and_returned_canonical() {
        let temp = tempfile::tempdir().unwrap();
        let project = make(temp.path(), "work/projekt");
        let rules = HostRootRules::new();
        let accepted = rules.validate(&project).unwrap();
        assert_eq!(accepted, std::fs::canonicalize(&project).unwrap());
    }

    #[test]
    fn missing_paths_and_files_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        let rules = HostRootRules::new();
        assert_eq!(
            rules.validate(&temp.path().join("nicht-da")),
            Err(HostRootError::NotFound)
        );
        let file = temp.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(rules.validate(&file), Err(HostRootError::NotADirectory));
    }

    #[test]
    fn a_whole_drive_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path())
            .unwrap()
            .ancestors()
            .last()
            .unwrap()
            .to_path_buf();
        assert_eq!(
            HostRootRules::new().validate(&root),
            Err(HostRootError::DriveRoot)
        );
    }

    #[test]
    fn the_whole_profile_and_its_parents_are_refused_but_projects_inside_are_fine() {
        let temp = tempfile::tempdir().unwrap();
        let profile = make(temp.path(), "Users/anna");
        let project = make(temp.path(), "Users/anna/Documents/projekt");
        let rules = HostRootRules::new().protect_only_self_and_parents(&profile, "Profil");
        assert!(matches!(
            rules.validate(&profile),
            Err(HostRootError::Protected { .. })
        ));
        // Der übergeordnete Ordner (alle Benutzer) ebenso.
        assert!(matches!(
            rules.validate(profile.parent().unwrap()),
            Err(HostRootError::Protected { .. })
        ));
        assert!(rules.validate(&project).is_ok());
    }

    #[test]
    fn protected_trees_are_closed_including_everything_below() {
        let temp = tempfile::tempdir().unwrap();
        let secrets = make(temp.path(), "home/.ssh");
        let below = make(temp.path(), "home/.ssh/keys");
        let rules = HostRootRules::new().protect_tree(&secrets, "Zugangsdaten");
        assert!(rules.validate(&secrets).is_err());
        assert!(rules.validate(&below).is_err());
        // Ein Geschwisterordner ist nicht betroffen.
        let sibling = make(temp.path(), "home/projekt");
        assert!(rules.validate(&sibling).is_ok());
    }

    #[test]
    fn the_stick_and_folders_containing_it_are_refused() {
        let temp = tempfile::tempdir().unwrap();
        let stick = make(temp.path(), "usb/IAP");
        let rules = HostRootRules::new().protect_tree(&stick, "der IAP-Stick selbst");
        let error = rules.validate(&stick).unwrap_err();
        assert!(error.to_string().contains("Stick"), "{error}");
        assert!(rules.validate(stick.parent().unwrap()).is_err());
    }

    #[test]
    fn the_environment_rules_protect_appdata_where_the_temp_folder_lives() {
        // Auf Windows liegt %TEMP% unter %LOCALAPPDATA%: dort darf kein Arbeitsordner entstehen.
        if std::env::var_os("LOCALAPPDATA").is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let project = make(temp.path(), "projekt");
        let rules = HostRootRules::from_environment(&[]);
        assert!(matches!(
            rules.validate(&project),
            Err(HostRootError::Protected { .. })
        ));
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_to_a_protected_folder_is_resolved_and_refused() {
        let temp = tempfile::tempdir().unwrap();
        let secrets = make(temp.path(), "home/.ssh");
        let link = temp.path().join("harmlos");
        junction::create(&secrets, &link).unwrap();
        let rules = HostRootRules::new().protect_tree(&secrets, "Zugangsdaten");
        assert!(rules.validate(&link).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_compares_case_insensitively() {
        let temp = tempfile::tempdir().unwrap();
        let secrets = make(temp.path(), "home/.ssh");
        let rules = HostRootRules::new().protect_tree(&secrets, "Zugangsdaten");
        let shouted = PathBuf::from(secrets.to_string_lossy().to_uppercase());
        assert!(rules.validate(&shouted).is_err());
    }
}
