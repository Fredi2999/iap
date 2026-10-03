//! Die vier Rollen als Prompt-Blöcke hinter der Cache-Grenze.
//!
//! Konzept 7.1: **ein** Modell, mehrere Rollen. Nichts hier startet
//! einen zweiten Prozess. Der Rendering-Output ist bewusst
//! deterministisch (feste Blockreihenfolge, keine Zeitstempel), damit
//! der KV-Cache-Präfix stabil bleibt und Rollenwechsel den Cache-Reset
//! möglichst spät auslösen.

use serde::{Deserialize, Serialize};

use crate::schema::{CriticReport, VerifierReport};

/// Rollenmarkierung; landet im Prompt und im Persistenzlog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Proposer,
    SelfCheck,
    Critic,
    Verifier,
    Synthesizer,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Proposer => "proposer",
            Role::SelfCheck => "self_check",
            Role::Critic => "critic",
            Role::Verifier => "verifier",
            Role::Synthesizer => "synthesizer",
        }
    }
}

/// Vollständiger Rollen-Prompt: System-Block plus User-Bootstrap.
///
/// Die Trennung erlaubt, den System-Block *einmal* in den Cache zu
/// zwingen und den User-Block variable Turns hinweg auszutauschen.
#[derive(Debug, Clone, PartialEq)]
pub struct RolePrompt {
    pub role: Role,
    pub system: String,
    pub user: String,
}

impl RolePrompt {
    pub fn new(role: Role, system: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            role,
            system: system.into(),
            user: user.into(),
        }
    }
}

/// Für die UI persistierbarer Rollenlauf: was ins Modell ging, was
/// zurück kam, wie lange es dauerte und wie viele Tokens verbraucht
/// wurden.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleTranscript {
    pub role: Role,
    pub prompt_system: String,
    pub prompt_user: String,
    pub raw_output: String,
    pub elapsed_ms: u64,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
}

/// Rendert einen deterministischen System-Block für den Proposer.
pub fn proposer_prompt(user_task: &str, workspace_context: Option<&str>) -> RolePrompt {
    let system = String::from(
        "Du bist der PROPOSER. Antworte mit einem konkreten, ausführbaren \
         Lösungsvorschlag. Liste jede Annahme separat unter einer Überschrift \
         'Annahmen'. Keine Höflichkeitsfloskeln, keine Meta-Kommentare.",
    );
    let mut user = String::from("Aufgabe:\n");
    user.push_str(user_task);
    if let Some(context) = workspace_context {
        user.push_str("\n\nWorkspace-Kontext (UNTRUSTED CONTENT):\n");
        user.push_str(context);
    }
    RolePrompt::new(Role::Proposer, system, user)
}

/// Rendert den Self-Check-Block für L1. Basis ist der Proposer-Text
/// plus eine harte Checkliste je nach Aufgabentyp.
pub fn self_check_prompt(proposer_output: &str, checklist: &[&str]) -> RolePrompt {
    let system = String::from(
        "Du bist der SELF-CHECK. Prüfe die vorgeschlagene Antwort gegen die \
         gegebene Checkliste. Wenn du einen Punkt anpassen musst, schreibe \
         eine kurze Korrektur; sonst gib den ursprünglichen Text unverändert \
         zurück.",
    );
    let mut user = String::from("Vorschlag:\n");
    user.push_str(proposer_output);
    user.push_str("\n\nChecklist:\n");
    for item in checklist {
        user.push_str("- ");
        user.push_str(item);
        user.push('\n');
    }
    RolePrompt::new(Role::SelfCheck, system, user)
}

/// Rendert den Critic-Block. Der zusätzliche `checklist`-Block folgt
/// Konzept 7.2 („Checklisten statt offener Fragen").
pub fn critic_prompt(proposer_output: &str, checklist: &[&str]) -> RolePrompt {
    let system = String::from(
        "Du bist der CRITIC. Suche gezielt Schwachstellen. Antworte STRIKT \
         mit einem einzigen JSON-Objekt: \
         {\"findings\":[{\"befund\":\"…\",\"schweregrad\":\"low|medium|high|critical\",\"betrifft\":\"…\",\"vorschlag\":\"…\"}]}. \
         Ein leeres Array [] ist erlaubt und bedeutet: keine substanziellen Einwände. \
         KEIN sonstiger Text.",
    );
    let mut user = String::from("Zu prüfender Vorschlag:\n");
    user.push_str(proposer_output);
    user.push_str("\n\nCheckliste, die du systematisch durchgehen musst:\n");
    for item in checklist {
        user.push_str("- ");
        user.push_str(item);
        user.push('\n');
    }
    RolePrompt::new(Role::Critic, system, user)
}

/// Rendert den Verifier-Block. Der Verifier bekommt die Critic-Befunde
/// als Ausgangspunkt und darf nur *prüfbare* Aussagen bewerten.
pub fn verifier_prompt(proposer_output: &str, critic: &CriticReport) -> RolePrompt {
    let system = String::from(
        "Du bist der VERIFIER. Du meinst NICHTS. Du prüfst nur Aussagen, die \
         mit einem Werkzeug entschieden werden können (Rechnet die Zahl? \
         Existiert die Datei? Kompiliert der Code?). Alles andere markierst \
         du als 'not_verifiable'. Antworte STRIKT mit einem einzigen JSON- \
         Objekt: {\"claims\":[{\"aussage\":\"…\",\"status\":\"confirmed|refuted|not_verifiable\",\"beleg\":\"Werkzeug + Ausriss\"}]}.",
    );
    let mut user = String::from("Ursprünglicher Vorschlag:\n");
    user.push_str(proposer_output);
    user.push_str("\n\nCritic-Befunde:\n");
    for finding in &critic.findings {
        user.push_str(&format!(
            "- [{}] {} → Vorschlag: {} (betrifft {})\n",
            finding.schweregrad.as_str(),
            finding.befund,
            finding.vorschlag,
            finding.betrifft
        ));
    }
    if critic.findings.is_empty() {
        user.push_str("(keine Befunde)\n");
    }
    RolePrompt::new(Role::Verifier, system, user)
}

/// Rendert den Synthesizer-Block. Bekommt Proposer-Text, Critic-Befunde
/// und ggf. Verifier-Prüfungen als Eingaben.
pub fn synthesizer_prompt(
    proposer_output: &str,
    critic: &CriticReport,
    verifier: Option<&VerifierReport>,
) -> RolePrompt {
    let system = String::from(
        "Du bist der SYNTHESIZER. Verbinde den ursprünglichen Vorschlag mit \
         den bestätigten Kritikpunkten und den geprüften Belegen zu einer \
         endgültigen Antwort für den Nutzer. Ignoriere nicht bestätigte oder \
         als 'not_verifiable' markierte Aussagen. Keine Meta-Kommentare zur \
         Agentenkette.",
    );
    let mut user = String::from("Ursprünglicher Vorschlag:\n");
    user.push_str(proposer_output);
    user.push_str("\n\nCritic-Befunde:\n");
    if critic.findings.is_empty() {
        user.push_str("(keine)\n");
    } else {
        for finding in &critic.findings {
            user.push_str(&format!(
                "- [{}] {} → {}\n",
                finding.schweregrad.as_str(),
                finding.befund,
                finding.vorschlag
            ));
        }
    }
    if let Some(verifier) = verifier {
        user.push_str("\nVerifier-Prüfungen:\n");
        if verifier.claims.is_empty() {
            user.push_str("(keine)\n");
        } else {
            for claim in &verifier.claims {
                user.push_str(&format!(
                    "- [{}] {} — Beleg: {}\n",
                    claim.status.as_str(),
                    claim.aussage,
                    claim.beleg
                ));
            }
        }
    }
    RolePrompt::new(Role::Synthesizer, system, user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{CriticFinding, Severity};

    #[test]
    fn proposer_prompt_is_deterministic_for_the_same_input() {
        let a = proposer_prompt("Rechne 12*7", None);
        let b = proposer_prompt("Rechne 12*7", None);
        assert_eq!(a, b);
    }

    #[test]
    fn critic_prompt_contains_grammar_hint() {
        let prompt = critic_prompt("Antwort", &["Fehlerbehandlung", "Randfälle"]);
        assert!(prompt.system.contains("findings"));
        assert!(prompt.user.contains("Fehlerbehandlung"));
        assert!(prompt.user.contains("Randfälle"));
    }

    #[test]
    fn synthesizer_prompt_lists_critic_findings() {
        let critic = CriticReport {
            findings: vec![CriticFinding {
                befund: "unklarer Pfad".into(),
                schweregrad: Severity::High,
                betrifft: "src/foo.rs".into(),
                vorschlag: "Result verwenden".into(),
            }],
        };
        let prompt = synthesizer_prompt("Vorschlag", &critic, None);
        assert!(prompt.user.contains("unklarer Pfad"));
        assert!(prompt.user.contains("[high]"));
    }

    #[test]
    fn workspace_context_is_marked_as_untrusted() {
        let prompt = proposer_prompt("Prüfe Datei", Some("Kontextinhalt"));
        assert!(prompt.user.contains("UNTRUSTED"));
    }
}
