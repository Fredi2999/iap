//! Freigaben (Grants) mit Geltungsbereich nach Konzept 10.1.
//!
//! Grants sind ausdrücklich vom Nutzer erteilte Ausnahmen von der
//! Standardregel. Sie haben immer einen Geltungsbereich; nach dessen Ende
//! muss neu gefragt werden.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityAction;

/// Geltungsbereich einer Freigabe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrantScope {
    /// Nur für den nächsten unmittelbaren Aufruf.
    Once,
    /// Für die gesamte aktuelle Session.
    Session,
    /// Für ein benanntes Projekt (persistent im Vault).
    Project { name: String },
}

/// Zielbeschreibung: entweder ein einzelner kanonisierter Pfad oder ein
/// Präfix (Ordner samt Unterordner).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrantSubject {
    Exact { path: PathBuf },
    Prefix { path: PathBuf },
}

impl GrantSubject {
    fn matches(&self, target: &Path) -> bool {
        match self {
            GrantSubject::Exact { path } => path == target,
            GrantSubject::Prefix { path } => target.starts_with(path),
        }
    }
}

/// Eine einzelne Freigabe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub action: CapabilityAction,
    pub subject: GrantSubject,
    pub scope: GrantScope,
}

/// In-Memory-Speicher für aktive Freigaben.
///
/// Session-Grants leben mit dem `GrantStore`; Projekt-Grants werden vom
/// Aufrufer aus dem Vault vorbelegt.
#[derive(Debug, Default, Clone)]
pub struct GrantStore {
    grants: Vec<Grant>,
}

impl GrantStore {
    /// Nimmt eine neue Freigabe auf.
    pub fn add(&mut self, grant: Grant) {
        self.grants.push(grant);
    }

    /// Entfernt einmalige Grants nach dem Verbrauch.
    pub fn consume_once(&mut self, target: &Path, action: CapabilityAction) {
        self.grants.retain(|g| {
            !(matches!(g.scope, GrantScope::Once)
                && g.action == action
                && g.subject.matches(target))
        });
    }

    /// Löscht alle Session-Grants beim Session-Ende.
    pub fn drop_session_scope(&mut self) {
        self.grants
            .retain(|g| !matches!(g.scope, GrantScope::Session));
    }

    /// Prüft, ob eine Aktion durch einen aktiven Grant abgedeckt ist.
    pub fn covers(&self, target: &Path, action: CapabilityAction) -> bool {
        self.grants
            .iter()
            .any(|g| g.action == action && g.subject.matches(target))
    }

    /// Liefert alle aktuell aktiven Grants (nur Lesezugriff für Diagnose/UI).
    pub fn snapshot(&self) -> Vec<Grant> {
        self.grants.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn once_grants_are_consumed_after_use() {
        let mut store = GrantStore::default();
        let target = PathBuf::from("/tmp/allowed.txt");
        store.add(Grant {
            action: CapabilityAction::FileWrite,
            subject: GrantSubject::Exact {
                path: target.clone(),
            },
            scope: GrantScope::Once,
        });
        assert!(store.covers(&target, CapabilityAction::FileWrite));
        store.consume_once(&target, CapabilityAction::FileWrite);
        assert!(!store.covers(&target, CapabilityAction::FileWrite));
    }

    #[test]
    fn session_grants_disappear_on_reset() {
        let mut store = GrantStore::default();
        let target = PathBuf::from("/tmp/session.txt");
        store.add(Grant {
            action: CapabilityAction::FileRead,
            subject: GrantSubject::Exact {
                path: target.clone(),
            },
            scope: GrantScope::Session,
        });
        assert!(store.covers(&target, CapabilityAction::FileRead));
        store.drop_session_scope();
        assert!(!store.covers(&target, CapabilityAction::FileRead));
    }

    #[test]
    fn project_grants_survive_session_reset() {
        let mut store = GrantStore::default();
        let target = PathBuf::from("/tmp/project.txt");
        store.add(Grant {
            action: CapabilityAction::FileRead,
            subject: GrantSubject::Exact {
                path: target.clone(),
            },
            scope: GrantScope::Project {
                name: "acme".into(),
            },
        });
        store.drop_session_scope();
        assert!(store.covers(&target, CapabilityAction::FileRead));
    }

    #[test]
    fn prefix_grants_cover_subpaths() {
        let mut store = GrantStore::default();
        store.add(Grant {
            action: CapabilityAction::FileList,
            subject: GrantSubject::Prefix {
                path: PathBuf::from("/tmp/dir"),
            },
            scope: GrantScope::Session,
        });
        assert!(store.covers(
            &PathBuf::from("/tmp/dir/sub/file"),
            CapabilityAction::FileList
        ));
        assert!(!store.covers(
            &PathBuf::from("/tmp/other/file"),
            CapabilityAction::FileList
        ));
    }
}
