//! Gespeicherter Oberflächenzustand (z. B. Effekt-Stufe, Kalenderaufgaben).
//!
//! Die Oberfläche darf nur Werte in einem eigenen Namensraum ablegen. So kann
//! sie keine Backend-Einstellungen wie Modellprofile oder das Farbschema
//! überschreiben, und alle Werte liegen verschlüsselt im Vault statt auf dem Host.

/// Höchstgröße eines einzelnen Werts. Kalenderaufgaben und Vorlagen passen
/// bequem hinein; größere Datenmengen gehören in eigene Tabellen.
pub const MAX_UI_STATE_BYTES: usize = 256 * 1024;

/// Präfix, unter dem die Werte in der Settings-Tabelle des Vaults liegen.
const STORAGE_PREFIX: &str = "uistate.";

/// Prüft einen Schlüssel der Oberfläche und liefert den Speicher-Schlüssel.
///
/// Erlaubt sind nur `ui.` gefolgt von 1 bis 48 Zeichen aus Kleinbuchstaben,
/// Ziffern, `_` und `.`, damit Schlüssel vorhersehbar und nicht zur
/// Pfad- oder Namensraum-Manipulation nutzbar sind.
pub fn storage_key(key: &str) -> Result<String, String> {
    let rest = key
        .strip_prefix("ui.")
        .ok_or_else(|| "Schlüssel muss mit `ui.` beginnen".to_owned())?;
    if rest.is_empty() || rest.len() > 48 {
        return Err("Schlüssel muss 1 bis 48 Zeichen nach `ui.` haben".to_owned());
    }
    if !rest
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
    {
        return Err("Schlüssel darf nur a-z, 0-9, `_` und `.` enthalten".to_owned());
    }
    Ok(format!("{STORAGE_PREFIX}{key}"))
}

/// Prüft die Größe eines Werts, bevor er in den Vault geschrieben wird.
pub fn check_value(value: &str) -> Result<(), String> {
    if value.len() > MAX_UI_STATE_BYTES {
        return Err(format!(
            "Wert ist zu groß ({} Bytes, erlaubt sind {MAX_UI_STATE_BYTES})",
            value.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_key_is_namespaced() {
        assert_eq!(storage_key("ui.effects").unwrap(), "uistate.ui.effects");
        assert_eq!(
            storage_key("ui.scheduler.tasks").unwrap(),
            "uistate.ui.scheduler.tasks"
        );
    }

    #[test]
    fn key_without_prefix_is_rejected() {
        assert!(storage_key("model.active_id").is_err());
        assert!(storage_key("effects").is_err());
    }

    #[test]
    fn key_with_forbidden_characters_is_rejected() {
        assert!(storage_key("ui.Effects").is_err());
        assert!(storage_key("ui.a/b").is_err());
        assert!(storage_key("ui.a b").is_err());
        assert!(storage_key("ui.").is_err());
    }

    #[test]
    fn overlong_key_is_rejected() {
        let key = format!("ui.{}", "a".repeat(49));
        assert!(storage_key(&key).is_err());
        let ok = format!("ui.{}", "a".repeat(48));
        assert!(storage_key(&ok).is_ok());
    }

    #[test]
    fn value_size_is_limited() {
        assert!(check_value(&"x".repeat(MAX_UI_STATE_BYTES)).is_ok());
        assert!(check_value(&"x".repeat(MAX_UI_STATE_BYTES + 1)).is_err());
    }
}
