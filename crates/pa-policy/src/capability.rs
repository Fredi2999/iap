//! Ableitung und Prüfung konkreter Werkzeug-Anforderungen.
//!
//! Ein `CapabilityRequest` entsteht, sobald ein Werkzeug aufgerufen wird. Er
//! wird gegen den aktuellen Modus, die Freigaben und die Herkunft der Aktion
//! geprüft, bevor er in `Capability` (mit kanonischen Pfaden) übergeht.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    grants::GrantStore,
    path::{safe_join, PathScope},
    Mode, PolicyError,
};

/// Herkunft der Aktion; unterscheidet Nutzerabsicht von Fremdinhalt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivationSource {
    /// Direkt vom Nutzer angeforderte Aktion (Klick, Eingabe).
    UserIntent,
    /// Vom Modell aus gelesenem Fremdinhalt abgeleitet (Prompt-Injection-Risiko).
    UntrustedContent,
}

/// Beschreibt eine gewünschte Fähigkeit vor der Prüfung.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRequest {
    /// Aktion (lesen, schreiben, ausführen …).
    pub action: CapabilityAction,
    /// Relativer Zielpfad innerhalb der Wurzel des zugehörigen `PathScope`.
    pub relative_path: Option<PathBuf>,
    /// Herkunft der Aktion.
    pub source: DerivationSource,
    /// Freitext-Grund; landet im Audit-Log.
    pub reason: String,
}

/// Konkrete Aktionsart, mit der die Policy arbeitet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAction {
    FileRead,
    FileWrite,
    FileList,
    Network,
    Exec,
    /// Reine Rechen-/Datumsfunktion, kein Dateisystem-Kontakt.
    Pure,
    /// Anfrage an die Exa-Schnittstelle; nur über `egress::authorize_exa`.
    ExaRequest,
    /// Anfrage an einen anderen Web-Konnektor (Wikipedia, Open-Meteo, Brave); nur über
    /// `egress::authorize_connector`.
    WebRequest,
    /// Mails lesen oder suchen; nur über `egress::authorize_mail`.
    MailRead,
    /// Eine Mail senden oder einen Entwurf anlegen; nur über `egress::authorize_mail`.
    MailSend,
    /// Kalendertermine abrufen; nur über `egress::authorize_calendar`.
    CalendarRead,
    /// Einen Termin im verbundenen Kalender anlegen; nur über `egress::authorize_calendar`.
    CalendarWrite,
    /// Eine Mikrofonaufnahme; nur über `device::authorize_capture`.
    MicCapture,
    /// Eine einzelne Bildschirmaufnahme; nur über `device::authorize_capture`.
    ScreenCapture,
    /// Git-Operation im Worktree-Betrieb; nur über `process::authorize_process`.
    GitOp,
    /// Start eines externen Prozesses; nur über `process::authorize_process`.
    ProcessRun,
}

impl CapabilityAction {
    /// Aktionen, die nur über ihren eigenen, engeren Prüfpfad freigegeben
    /// werden (Exa, Aufnahme, Git, Prozesse).
    ///
    /// Warum: `evaluate` kennt nur Modus und Pfad. Diese Aktionen brauchen
    /// zusätzlich Laufbindung, Datenherkunft oder Einmal-Tickets; ein Modus
    /// allein darf sie nie öffnen.
    pub fn has_dedicated_gate(self) -> bool {
        matches!(
            self,
            Self::ExaRequest
                | Self::WebRequest
                | Self::MailRead
                | Self::MailSend
                | Self::CalendarRead
                | Self::CalendarWrite
                | Self::MicCapture
                | Self::ScreenCapture
                | Self::GitOp
                | Self::ProcessRun
        )
    }
}

/// Freigegebene Kombination aus Aktion und kanonisiertem Zielpfad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    pub action: CapabilityAction,
    pub canonical_path: Option<PathBuf>,
}

/// Ergebnis einer Prüfung; `Allow` fährt die Aktion aus, `Prompt` erfordert
/// eine Bestätigung im UI-Thread, `Deny` bricht ab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow(Capability),
    Prompt(String),
    Deny(String),
}

/// Führt die Modus- und Herkunftsprüfung durch und kanonisiert den Pfad.
///
/// Grants aus `GrantStore` werden hier gelesen; sie zählen nur bei
/// `DerivationSource::UserIntent`. Für `UntrustedContent` erzwingt die
/// Policy stets eine bewusste Bestätigung (siehe Konzept 10.3).
pub fn evaluate(
    request: &CapabilityRequest,
    mode: Mode,
    workspace: &PathScope,
    grants: &GrantStore,
) -> Result<Decision, PolicyError> {
    let canonical = match &request.relative_path {
        Some(path) => Some(safe_join(workspace, path)?),
        None => None,
    };
    let capability = Capability {
        action: request.action,
        canonical_path: canonical.clone(),
    };

    if request.action.has_dedicated_gate() {
        // Diese Aktionen haben je einen eigenen, engeren Prüfpfad (Konzept
        // 10.5). Der allgemeine Pfad-Evaluator gibt sie nie frei.
        return Ok(Decision::Deny(format!(
            "Aktion {:?} wird nur über ihren eigenen Prüfpfad freigegeben",
            request.action
        )));
    }

    if request.action == CapabilityAction::Network {
        // Konzept 10.1: Netzwerk bleibt in allen Modi gesperrt.
        return Ok(Decision::Deny("Netzwerk in allen Modi verboten".to_owned()));
    }

    if request.source == DerivationSource::UntrustedContent {
        // Auto-Freigaben gelten hier nicht.
        return Ok(Decision::Prompt(
            "Aktion stammt aus Fremdinhalt; bitte bewusst bestätigen".to_owned(),
        ));
    }

    // Ein Grant kann die Modus-Standardregel aufweichen (z. B. „diese Datei
    // dieses Mal doch schreiben"). Er kann sie nie verschärfen; verschärfende
    // Grants existieren im MVP nicht.
    let grant_covers = canonical
        .as_ref()
        .map(|path| grants.covers(path, request.action))
        .unwrap_or(false);

    match mode_allows(mode, request.action) {
        ModeVerdict::Allow => Ok(Decision::Allow(capability)),
        ModeVerdict::Prompt if grant_covers => Ok(Decision::Allow(capability)),
        ModeVerdict::Prompt => Ok(Decision::Prompt(format!(
            "Aktion {:?} verlangt Bestätigung in Modus {mode:?}",
            request.action
        ))),
        ModeVerdict::Deny => Ok(Decision::Deny(format!(
            "Modus {mode:?} verbietet Aktion {:?}",
            request.action
        ))),
    }
}

#[derive(Debug, Clone, Copy)]
enum ModeVerdict {
    Allow,
    Prompt,
    Deny,
}

fn mode_allows(mode: Mode, action: CapabilityAction) -> ModeVerdict {
    use CapabilityAction::*;
    match (mode, action) {
        (_, Pure) => ModeVerdict::Allow,
        (
            _,
            Network | ExaRequest | WebRequest | MailRead | MailSend | CalendarRead | CalendarWrite
            | MicCapture | ScreenCapture | GitOp | ProcessRun,
        ) => ModeVerdict::Deny,
        (Mode::M0Observe, FileRead | FileList) => ModeVerdict::Allow,
        (Mode::M0Observe, FileWrite | Exec) => ModeVerdict::Deny,
        (Mode::M1Workspace, FileRead | FileList | FileWrite) => ModeVerdict::Allow,
        (Mode::M1Workspace, Exec) => ModeVerdict::Deny,
        (Mode::M2Extended, FileRead | FileList) => ModeVerdict::Allow,
        (Mode::M2Extended, FileWrite | Exec) => ModeVerdict::Prompt,
        (Mode::M3Autonomous, _) => ModeVerdict::Allow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded_scope() -> (tempfile::TempDir, PathScope) {
        let temp = tempfile::TempDir::with_prefix("pa-policy-cap").expect("temp");
        std::fs::write(temp.path().join("hi.txt"), "").unwrap();
        let scope = PathScope::new(temp.path()).unwrap();
        (temp, scope)
    }

    #[test]
    fn network_is_always_denied() {
        let (_temp, scope) = seeded_scope();
        let grants = GrantStore::default();
        let request = CapabilityRequest {
            action: CapabilityAction::Network,
            relative_path: None,
            source: DerivationSource::UserIntent,
            reason: "Analytics".into(),
        };
        let decision = evaluate(&request, Mode::M3Autonomous, &scope, &grants).unwrap();
        assert!(matches!(decision, Decision::Deny(_)));
    }

    #[test]
    fn read_in_m0_is_allowed() {
        let (_temp, scope) = seeded_scope();
        let grants = GrantStore::default();
        let request = CapabilityRequest {
            action: CapabilityAction::FileRead,
            relative_path: Some(std::path::PathBuf::from("hi.txt")),
            source: DerivationSource::UserIntent,
            reason: "Inhalt zeigen".into(),
        };
        let decision = evaluate(&request, Mode::M0Observe, &scope, &grants).unwrap();
        assert!(matches!(decision, Decision::Allow(_)));
    }

    #[test]
    fn write_in_m0_is_denied() {
        let (_temp, scope) = seeded_scope();
        let grants = GrantStore::default();
        let request = CapabilityRequest {
            action: CapabilityAction::FileWrite,
            relative_path: Some(std::path::PathBuf::from("hi.txt")),
            source: DerivationSource::UserIntent,
            reason: "Bearbeiten".into(),
        };
        let decision = evaluate(&request, Mode::M0Observe, &scope, &grants).unwrap();
        assert!(matches!(decision, Decision::Deny(_)));
    }

    #[test]
    fn untrusted_source_always_prompts_even_when_mode_would_allow() {
        let (_temp, scope) = seeded_scope();
        let grants = GrantStore::default();
        let request = CapabilityRequest {
            action: CapabilityAction::FileWrite,
            relative_path: Some(std::path::PathBuf::from("hi.txt")),
            source: DerivationSource::UntrustedContent,
            reason: "aus gelesenem Dokument".into(),
        };
        let decision = evaluate(&request, Mode::M3Autonomous, &scope, &grants).unwrap();
        assert!(matches!(decision, Decision::Prompt(_)));
    }

    #[test]
    fn m2_write_prompts_without_grant_and_allows_with_grant() {
        let (_temp, scope) = seeded_scope();
        let target = std::fs::canonicalize(scope.root().join("hi.txt")).unwrap();

        let request = CapabilityRequest {
            action: CapabilityAction::FileWrite,
            relative_path: Some(std::path::PathBuf::from("hi.txt")),
            source: DerivationSource::UserIntent,
            reason: "Bearbeiten".into(),
        };
        let empty = GrantStore::default();
        assert!(matches!(
            evaluate(&request, Mode::M2Extended, &scope, &empty).unwrap(),
            Decision::Prompt(_)
        ));

        let mut with_grant = GrantStore::default();
        with_grant.add(crate::grants::Grant {
            action: CapabilityAction::FileWrite,
            subject: crate::grants::GrantSubject::Exact { path: target },
            scope: crate::grants::GrantScope::Session,
        });
        assert!(matches!(
            evaluate(&request, Mode::M2Extended, &scope, &with_grant).unwrap(),
            Decision::Allow(_)
        ));
    }
}
