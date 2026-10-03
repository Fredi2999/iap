//! Sprache der Oberfläche und der Modellantworten.
//!
//! Die Wahl muss schon vor dem Entsperren lesbar sein (Entsperr-Bildschirm),
//! deshalb liegt sie unverschlüsselt als eigene kleine Datei neben dem Tresor
//! auf dem Stick. Bewusst nicht in `vault.meta`: Diese Datei trägt den Salt der
//! Schlüsselableitung, und ein Fehler beim Schreiben einer Anzeigeeinstellung
//! darf den Tresor nie unbrauchbar machen. Auf dem Host-Rechner wird nichts abgelegt.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Aktive Sprache des Prozesses als Index in [`SUPPORTED`]. Prozessweit, weil
/// Prompt-Aufbau an mehreren Stellen ohne Zugriff auf den App-Zustand passiert.
static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// Aktuell gewählte Sprache.
pub fn current() -> &'static str {
    SUPPORTED
        .get(CURRENT.load(Ordering::Relaxed))
        .copied()
        .unwrap_or(DEFAULT_LANGUAGE)
}

/// Setzt die aktive Sprache nach dem Laden oder Ändern der Wahl.
pub fn set_current(code: &str) {
    if let Some(index) = SUPPORTED.iter().position(|supported| *supported == code) {
        CURRENT.store(index, Ordering::Relaxed);
    }
}

/// Unterstützte Sprachen als ISO-639-1-Codes.
pub const SUPPORTED: [&str; 5] = ["de", "en", "es", "fr", "ja"];

/// Voreinstellung, wenn noch keine Wahl gespeichert ist.
pub const DEFAULT_LANGUAGE: &str = "de";

const FILE_NAME: &str = "ui-language";

/// Liefert den Dateipfad im Datenordner des Pakets (`AI/data`).
pub fn file_path(package_root: &Path) -> PathBuf {
    package_root.join("AI").join("data").join(FILE_NAME)
}

/// Prüft einen Sprachcode gegen die unterstützte Liste.
pub fn validate(code: &str) -> Result<&'static str, String> {
    SUPPORTED
        .iter()
        .find(|supported| **supported == code)
        .copied()
        .ok_or_else(|| format!("Sprache `{code}` wird nicht unterstützt"))
}

/// Liest die gespeicherte Sprache; unbekannte oder fehlende Werte ergeben die Voreinstellung,
/// damit eine beschädigte Datei den Start nie verhindert.
pub fn read(package_root: &Path) -> &'static str {
    std::fs::read_to_string(file_path(package_root))
        .ok()
        .and_then(|raw| validate(raw.trim()).ok())
        .unwrap_or(DEFAULT_LANGUAGE)
}

/// Schreibt die Sprache atomar: erst in eine Nachbardatei, dann umbenennen.
pub fn write(package_root: &Path, code: &str) -> Result<&'static str, String> {
    let code = validate(code)?;
    let target = file_path(package_root);
    let parent = target
        .parent()
        .ok_or_else(|| "Datenordner nicht gefunden".to_owned())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let partial = target.with_extension("partial");
    std::fs::write(&partial, code).map_err(|error| error.to_string())?;
    std::fs::rename(&partial, &target).map_err(|error| error.to_string())?;
    Ok(code)
}

/// Anweisung an das Modell, in der gewählten Sprache zu antworten. Für Deutsch
/// ist keine Anweisung nötig, weil die bestehenden Systemprompts deutsch sind.
pub fn response_instruction(code: &str) -> Option<&'static str> {
    match code {
        "en" => Some("Always answer in English unless the user explicitly asks for another language."),
        "es" => Some("Responde siempre en español, salvo que el usuario pida expresamente otro idioma."),
        "fr" => Some("Réponds toujours en français, sauf si l'utilisateur demande explicitement une autre langue."),
        "ja" => Some("ユーザーが別の言語を明示的に求めない限り、常に日本語で回答してください。"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("iap-lang-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn validates_supported_codes_only() {
        assert_eq!(validate("ja").unwrap(), "ja");
        assert!(validate("it").is_err());
        assert!(validate("DE").is_err());
        assert!(validate("").is_err());
    }

    #[test]
    fn missing_or_broken_file_falls_back_to_german() {
        let root = temp_root("missing");
        assert_eq!(read(&root), "de");
        std::fs::create_dir_all(root.join("AI").join("data")).unwrap();
        std::fs::write(file_path(&root), "klingonisch").unwrap();
        assert_eq!(read(&root), "de");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_then_read_round_trips() {
        let root = temp_root("roundtrip");
        assert_eq!(write(&root, "fr").unwrap(), "fr");
        assert_eq!(read(&root), "fr");
        assert!(write(&root, "xx").is_err());
        assert_eq!(read(&root), "fr");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn current_language_follows_set_current() {
        set_current("es");
        assert_eq!(current(), "es");
        set_current("unbekannt");
        assert_eq!(current(), "es");
        set_current("de");
        assert_eq!(current(), "de");
    }

    #[test]
    fn instruction_only_for_non_german() {
        assert!(response_instruction("de").is_none());
        assert!(response_instruction("en").is_some());
        assert!(response_instruction("ja").is_some());
    }
}
