//! Befehle des Code-Agenten: prüfen, vom Nutzer bestätigen lassen, ausführen.
//!
//! Das Modell darf einen Befehl nur **vorschlagen**. Jeder einzelne Befehl läuft erst, nachdem
//! `pa-policy` ihn geprüft hat und der Nutzer ihn im Dialog gesehen und freigegeben hat. Der
//! Dialog nennt offen, was die Prüfung nicht leisten kann (keine Sandbox: das Programm selbst
//! kann das Netz nutzen und Dateien außerhalb des Projekts ändern). Aufruf und Ausgang stehen im
//! Audit-Log.

use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, mpsc, Arc},
    time::{Duration, Instant},
};

use pa_code::agent::{CommandGate, CommandRequest};
use pa_policy::{
    command::{authorize_command, CommandPolicy, CommandSpec, CommandVerdict},
    Capability, CapabilityAction, Decision, PathScope,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{command_runner, lifecycle, random_id, AppError, AppResult, AppState};

/// Wie lange der Dialog auf eine Antwort wartet, bevor der Befehl als abgelehnt gilt.
const ANSWER_WAIT: Duration = Duration::from_secs(600);
/// Größte Ausgabe, die ein Lauf behält.
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
/// Längste Zeitgrenze eines Befehls.
const MAX_TIMEOUT_MS: u64 = 600_000;

/// Die Anfrage an den Nutzer, wie sie der Dialog zeigt.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CommandPrompt {
    pub id: String,
    /// Der Befehl in einer Zeile.
    pub command: String,
    /// Arbeitsordner relativ zum Projekt.
    pub cwd: String,
    pub timeout_seconds: u64,
    /// Das Modell hat den Befehl aus gelesenem Dateiinhalt übernommen.
    pub derived_from_content: bool,
}

/// Die Bestätigungsstelle für `run_command` im Code-Agenten.
pub struct AppCommandGate {
    pub app: AppHandle,
    /// Kanonischer Projektordner.
    pub root: PathBuf,
    /// Wird gesetzt, wenn die Anfrage abgebrochen wird.
    pub cancel: Arc<AtomicBool>,
}

fn audit(state: &AppState, target: &str, decision: &Decision, reason: &str) {
    lifecycle::audit_decision(
        state,
        CapabilityAction::ProcessRun,
        Some(target.to_owned()),
        decision,
        reason,
    );
}

fn allow() -> Decision {
    Decision::Allow(Capability {
        action: CapabilityAction::ProcessRun,
        canonical_path: None,
    })
}

/// Der Text, den das Modell über den Lauf zurückbekommt.
fn describe(result: &command_runner::CommandOutput, timeout_seconds: u64) -> String {
    let headline = if result.cancelled {
        "Der Lauf wurde abgebrochen.".to_owned()
    } else if result.timed_out {
        format!("Die Zeitgrenze von {timeout_seconds} s war erreicht; der Befehl wurde beendet.")
    } else {
        match result.exit_code {
            Some(0) => format!(
                "Erfolgreich beendet (Exit-Code 0) nach {} ms.",
                result.millis
            ),
            Some(code) => format!(
                "Mit Fehler beendet (Exit-Code {code}) nach {} ms.",
                result.millis
            ),
            None => "Ohne Exit-Code beendet.".to_owned(),
        }
    };
    let mut text = headline;
    text.push('\n');
    if result.truncated {
        text.push_str("[Der Anfang der Ausgabe wurde gekürzt.]\n");
    }
    if result.output.trim().is_empty() {
        text.push_str("(keine Ausgabe)");
    } else {
        text.push_str(result.output.trim_end());
    }
    text
}

/// Wartet auf die Antwort des Nutzers; `None` bei Abbruch oder Zeitablauf.
fn wait_for_answer(
    receiver: &mpsc::Receiver<bool>,
    cancel: &AtomicBool,
    limit: Duration,
) -> Option<bool> {
    let started = Instant::now();
    loop {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(answer) => return Some(answer),
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if cancel.load(std::sync::atomic::Ordering::SeqCst) || started.elapsed() >= limit {
            return None;
        }
    }
}

impl CommandGate for AppCommandGate {
    fn run(&self, request: CommandRequest) -> String {
        let state = self.app.state::<AppState>();
        let Ok(scope) = PathScope::new(&self.root) else {
            return "FEHLER: Der Projektordner ist nicht erreichbar.".to_owned();
        };
        let policy = CommandPolicy::from_environment(scope, MAX_TIMEOUT_MS);
        let spec = CommandSpec {
            program: request.program.clone(),
            args: request.args.clone(),
            cwd: request.cwd.clone(),
            timeout_ms: request.timeout_seconds.saturating_mul(1000),
        };
        let command = match authorize_command(&policy, &spec) {
            CommandVerdict::Allowed(command) => command,
            CommandVerdict::Denied(reason) => {
                let shown = format!("{} {}", request.program, request.args.join(" "));
                audit(
                    &state,
                    &shown,
                    &Decision::Deny(reason.clone()),
                    "Code-Agent: Befehl abgelehnt",
                );
                return format!("FEHLER: {reason}");
            }
        };

        let id = random_id("command");
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut pending) = state.code_agent.pending_commands.lock() {
            pending.insert(id.clone(), sender);
        }
        let cwd_shown = command
            .cwd()
            .strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .ok()
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| ".".to_owned());
        let prompt = CommandPrompt {
            id: id.clone(),
            command: command.display(),
            cwd: cwd_shown,
            timeout_seconds: request.timeout_seconds,
            derived_from_content: request.derived_from_content,
        };
        let _ = self.app.emit("code-agent-command", prompt);
        let answer = wait_for_answer(&receiver, &self.cancel, ANSWER_WAIT);
        if let Ok(mut pending) = state.code_agent.pending_commands.lock() {
            pending.remove(&id);
        }
        let display = command.display();
        match answer {
            Some(true) => {}
            Some(false) => {
                audit(
                    &state,
                    &display,
                    &Decision::Deny("vom Nutzer abgelehnt".to_owned()),
                    "Code-Agent: Befehl vom Nutzer abgelehnt",
                );
                return "Die Nutzerin oder der Nutzer hat diesen Befehl nicht freigegeben. Frage nicht erneut nach demselben Befehl; schlage etwas anderes vor oder erkläre, was stattdessen zu tun wäre.".to_owned();
            }
            None => {
                audit(
                    &state,
                    &display,
                    &Decision::Deny("keine Antwort oder abgebrochen".to_owned()),
                    "Code-Agent: Befehl ohne Freigabe",
                );
                return "Der Befehl wurde nicht ausgeführt (keine Freigabe oder abgebrochen)."
                    .to_owned();
            }
        }

        audit(
            &state,
            &display,
            &allow(),
            "Code-Agent: Befehl vom Nutzer freigegeben und gestartet",
        );
        match command_runner::run(&command, &self.cancel, MAX_OUTPUT_BYTES) {
            Ok(result) => {
                audit(
                    &state,
                    &display,
                    &allow(),
                    &format!(
                        "Code-Agent: Befehl beendet (Exit-Code {:?}, Zeitgrenze erreicht: {}, abgebrochen: {})",
                        result.exit_code, result.timed_out, result.cancelled
                    ),
                );
                describe(&result, request.timeout_seconds)
            }
            Err(reason) => format!("FEHLER: {reason}"),
        }
    }
}

/// Antwort auf den Bestätigungsdialog: `allow = true` führt den Befehl aus.
#[tauri::command]
pub fn code_agent_command_respond(
    state: State<'_, AppState>,
    id: String,
    allow: bool,
) -> AppResult<()> {
    let sender = state
        .code_agent
        .pending_commands
        .lock()
        .map_err(|_| AppError::Internal("Sperre vergiftet".to_owned()))?
        .remove(&id);
    match sender {
        Some(sender) => {
            let _ = sender.send(allow);
            Ok(())
        }
        None => Err(AppError::Invalid(
            "Diese Anfrage ist nicht mehr offen.".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use command_runner::CommandOutput;

    fn output(exit: Option<i32>, text: &str) -> CommandOutput {
        CommandOutput {
            exit_code: exit,
            output: text.to_owned(),
            truncated: false,
            timed_out: false,
            cancelled: false,
            millis: 1500,
        }
    }

    #[test]
    fn the_model_is_told_how_the_run_ended() {
        let ok = describe(&output(Some(0), "alles gut\n"), 120);
        assert!(ok.starts_with("Erfolgreich beendet (Exit-Code 0)"), "{ok}");
        assert!(ok.ends_with("alles gut"));
        let failed = describe(&output(Some(101), "error[E0425]"), 120);
        assert!(
            failed.contains("Exit-Code 101") && failed.contains("E0425"),
            "{failed}"
        );
        let mut slow = output(None, "");
        slow.timed_out = true;
        let text = describe(&slow, 30);
        assert!(
            text.contains("Zeitgrenze von 30 s") && text.contains("(keine Ausgabe)"),
            "{text}"
        );
        let mut cut = output(Some(0), "ende");
        cut.truncated = true;
        assert!(describe(&cut, 1).contains("gekürzt"));
    }

    #[test]
    fn waiting_ends_on_an_answer_on_cancel_and_on_timeout() {
        let cancel = AtomicBool::new(false);
        let (sender, receiver) = mpsc::channel();
        sender.send(true).expect("senden");
        assert_eq!(
            wait_for_answer(&receiver, &cancel, Duration::from_secs(5)),
            Some(true)
        );

        let (_keep, receiver) = mpsc::channel::<bool>();
        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            wait_for_answer(&receiver, &cancel, Duration::from_secs(5)),
            None
        );

        let (_keep, receiver) = mpsc::channel::<bool>();
        let fresh = AtomicBool::new(false);
        let started = Instant::now();
        assert_eq!(
            wait_for_answer(&receiver, &fresh, Duration::from_millis(300)),
            None
        );
        assert!(started.elapsed() < Duration::from_secs(3));

        // Ein verworfener Sender (Dialog geschlossen) gilt nicht als Freigabe.
        let (sender, receiver) = mpsc::channel::<bool>();
        drop(sender);
        assert_eq!(
            wait_for_answer(&receiver, &fresh, Duration::from_secs(5)),
            None
        );
    }
}
