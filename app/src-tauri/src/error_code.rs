//! Stabile Fehlercodes zwischen Backend und Oberfläche.
//!
//! Warum: Die Oberfläche hat Fehlertexte bisher mit Textmustern erraten (`errors.ts`). Das bricht,
//! sobald ein Satz umformuliert oder übersetzt wird. Ein Code ist Teil des Vertrags; die Liste hier
//! und die Zuordnung in `app/src/lib/errors.ts` ändern sich nur gemeinsam.

/// Der Tresor ist nicht entsperrt.
pub const VAULT_LOCKED: &str = "vault_locked";
/// Passwort falsch oder Tresor beschädigt (SQLCipher kann beides nicht unterscheiden).
pub const WRONG_PASSPHRASE: &str = "wrong_passphrase";
/// Das portable Medium (der Stick) ist nicht erreichbar.
pub const VAULT_MEDIA_UNAVAILABLE: &str = "vault_media_unavailable";
/// Die Integritätsprüfung des Tresors ist fehlgeschlagen.
pub const VAULT_INTEGRITY: &str = "vault_integrity";

/// Ein globales Tastenkürzel ist schon von einem anderen Programm belegt.
pub const HOTKEY_TAKEN: &str = "hotkey_taken";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppError;

    #[test]
    fn coded_errors_serialize_as_objects_and_plain_ones_as_strings() {
        let coded = serde_json::to_value(AppError::locked()).unwrap_or_default();
        assert_eq!(coded["code"], VAULT_LOCKED);
        assert_eq!(coded["message"], "Der Tresor ist gesperrt.");
        let plain = serde_json::to_value(AppError::Invalid("x".to_owned())).unwrap_or_default();
        assert_eq!(plain, serde_json::json!("Ungültige Eingabe: x"));
    }

    #[test]
    fn vault_authentication_maps_to_a_code() {
        let error = AppError::from(pa_vault::VaultError::Authentication);
        assert!(matches!(
            error,
            AppError::Coded { code, .. } if code == WRONG_PASSPHRASE
        ));
        let other = AppError::from(pa_vault::VaultError::InvalidMetadata("x".to_owned()));
        assert!(matches!(other, AppError::Vault(_)));
    }

    #[test]
    fn codes_are_unique() {
        let all = [
            VAULT_LOCKED,
            WRONG_PASSPHRASE,
            VAULT_MEDIA_UNAVAILABLE,
            VAULT_INTEGRITY,
        ];
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
    }
}
