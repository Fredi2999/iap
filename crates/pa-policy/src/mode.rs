//! Die vier Berechtigungsmodi aus Konzept 10.1.
//!
//! Der Modus ist eine Nutzereinstellung; Wechsel nach oben verlangt eine
//! Bestätigung – hier definiert; die Erneuerung der Vault-Passphrase liegt
//! bei der UI, weil der Vault-Schlüssel nicht in dieser Crate lebt.

use serde::{Deserialize, Serialize};

/// Vier Berechtigungsmodi mit steigender Vertrauensstufe.
///
/// - `M0Observe`: nur lesen, keine Änderung, keine Ausführung.
/// - `M1Workspace`: Workspace mit Diff-Vorschau schreiben (Standard).
/// - `M2Extended`: Freigegebene Ordner lesend und einzeln bestätigt schreibend.
/// - `M3Autonomous`: Freigegebene Ordner ohne Einzelbestätigung, mit Audit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    M0Observe,
    #[default]
    M1Workspace,
    M2Extended,
    M3Autonomous,
}

impl Mode {
    /// Der Netzwerkzustand ist in **allen** Modi „aus".
    pub const NETWORK_ALLOWED: bool = false;

    /// Prüft, ob ein Modusaufstieg eine Vault-Bestätigung braucht.
    pub fn requires_reauth_on_upgrade(&self, target: Mode) -> bool {
        (*self as u8) < (target as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_is_always_disabled() {
        const { assert!(!Mode::NETWORK_ALLOWED) };
    }

    #[test]
    fn upgrade_requires_reauth() {
        assert!(Mode::M1Workspace.requires_reauth_on_upgrade(Mode::M2Extended));
        assert!(!Mode::M2Extended.requires_reauth_on_upgrade(Mode::M1Workspace));
        assert!(!Mode::M1Workspace.requires_reauth_on_upgrade(Mode::M1Workspace));
    }

    #[test]
    fn default_mode_is_workspace() {
        assert_eq!(Mode::default(), Mode::M1Workspace);
    }
}
