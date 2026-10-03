//! Kern der Eskalationsleiter L0–L3.
//!
//! Der Runner ruft für jede Rolle einen [`EngineCall`] auf, den der
//! Aufrufer bereitstellt (typischerweise ein Adapter um
//! `pa_core::engine::ChatEngine::stream_chat`, der das Ergebnis sammelt).
//! Die Rollen bekommen jeweils dieselbe Modellinstanz — es gibt keinen
//! zweiten Prozess (Konzept 7.1).
//!
//! Persistenz: nach jeder Rolle wird [`AgentRunStore::upsert`] mit dem
//! aktuellen Stand aufgerufen. Ein Absturz zwischen zwei Rollen führt
//! also zu einem eindeutig weiter-verwendbaren Zustand — der letzte
//! [`RoleTranscript`] wurde entweder erfolgreich geschrieben oder gar
//! nicht.

use serde::{Deserialize, Serialize};

use crate::{
    grammar,
    persist::{AgentRun, AgentRunStore},
    roles::{
        critic_prompt, proposer_prompt, self_check_prompt, synthesizer_prompt, verifier_prompt,
        Role, RolePrompt, RoleTranscript,
    },
    schema::VerifierReport,
    AgentError,
};

/// Die vier Eskalationsstufen aus Konzept 7.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Eine direkte Antwort.
    L0Direct,
    /// Antwort + Selbstprüfung.
    L1SelfCheck,
    /// Proposer → Critic → Synth.
    L2Critique,
    /// Proposer → Critic → Verifier (mit Tools) → Synth.
    L3Full,
}

impl Stage {
    /// Grobe Token- und Runden-Budgets aus Konzept 7.1.
    ///
    /// Wird vom Router für die Zeitschätzung und vom Runner für harte
    /// Grenzen verwendet.
    pub fn token_and_round_budget(self) -> (u32, u32) {
        match self {
            Stage::L0Direct => (300, 1),
            Stage::L1SelfCheck => (500, 2),
            Stage::L2Critique => (1_100, 3),
            Stage::L3Full => (1_700, 4),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Stage::L0Direct => "l0_direct",
            Stage::L1SelfCheck => "l1_self_check",
            Stage::L2Critique => "l2_critique",
            Stage::L3Full => "l3_full",
        }
    }
}

/// Ergebnis eines Rollen-Aufrufs.
///
/// `grammar_hint` ist optional — die Engine soll GBNF nur dann anwenden,
/// wenn der Aufrufer es unterstützt. Kein grammar-Hint = freies Modell.
#[derive(Debug, Clone, PartialEq)]
pub struct StageResult {
    pub raw_output: String,
    pub elapsed_ms: u64,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
}

/// Callback, mit dem der Runner das Modell aufruft.
///
/// Die Grammatik-Zeichenfolge zeigt an, welche GBNF-Grammatik verwendet
/// werden soll (`None` = keine). Der Aufrufer entscheidet, wie er sie
/// an den Server durchreicht (in `pa-inference::chat` kommt dafür
/// später ein `grammar`-Feld dazu).
pub trait EngineCall {
    fn call(
        &mut self,
        prompt: &RolePrompt,
        grammar: Option<&str>,
    ) -> Result<StageResult, AgentError>;
}

impl<F> EngineCall for F
where
    F: FnMut(&RolePrompt, Option<&str>) -> Result<StageResult, AgentError>,
{
    fn call(
        &mut self,
        prompt: &RolePrompt,
        grammar: Option<&str>,
    ) -> Result<StageResult, AgentError> {
        (self)(prompt, grammar)
    }
}

/// UI-Fortschritt; wird nach jedem persistierten Zwischenstand emittiert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunProgress {
    Started {
        run_id: String,
        stage: Stage,
    },
    RoleFinished {
        run_id: String,
        role: Role,
        elapsed_ms: u64,
        prompt_tokens: Option<u32>,
        completion_tokens: Option<u32>,
    },
    EarlyStopped {
        run_id: String,
        reason: String,
    },
    Aborted {
        run_id: String,
    },
    Finished {
        run_id: String,
        final_answer: String,
    },
}

/// Nutzer-Callback für die Runner-Schleife.
pub struct RunCallbacks<'a> {
    /// Wird periodisch geprüft; `false` bricht ab und liefert den
    /// zuletzt sinnvollen Text als „bestes Zwischenergebnis" (Konzept 7.3).
    pub should_continue: &'a mut dyn FnMut() -> bool,
    /// Wird nach jedem persistierten Ereignis aufgerufen (UI-Baum).
    pub on_progress: &'a mut dyn FnMut(RunProgress),
}

/// Harte Grenzen (Konzept 7.3).
#[derive(Debug, Clone, Copy)]
pub struct AgentBudgets {
    /// Wanduhr-Timeout gesamt; `None` = unbegrenzt (Test/CLI).
    pub wall_clock_seconds: Option<u64>,
    /// Maximale Runden pro Rolle (Standard 1).
    pub max_rounds_per_role: u32,
}

impl Default for AgentBudgets {
    fn default() -> Self {
        Self {
            wall_clock_seconds: None,
            max_rounds_per_role: 1,
        }
    }
}

/// Endergebnis eines Laufs.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub run: AgentRun,
    /// Die Antwort, die dem Nutzer angezeigt werden soll. Bei Abbruch:
    /// bester bis dahin gesammelter Text (Konzept 7.3).
    pub final_answer: String,
    /// Frühabbruch nach Critic ohne substanzielle Befunde?
    pub early_stopped: bool,
}

/// Orchestriert den Lauf.
pub struct EscalationRunner {
    /// Checkliste für Self-Check / Critic. Default: allgemeine Punkte;
    /// pro Aufgabentyp (Code, Plan) austauschbar (Konzept 7.2).
    pub checklist: Vec<String>,
    pub budgets: AgentBudgets,
}

impl Default for EscalationRunner {
    fn default() -> Self {
        Self {
            checklist: [
                "Fehlerbehandlung und Randfälle",
                "Ressourcen freigegeben",
                "Nebenläufigkeit / Reentranz",
                "Eingabevalidierung",
                "unrealistische Annahmen",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            budgets: AgentBudgets::default(),
        }
    }
}

impl EscalationRunner {
    /// Führt eine Stufe durch.
    ///
    /// `now_unix_ms` wird für den Run-Zeitstempel benutzt; die einzelnen
    /// Rollen-Timings kommen von den `StageResult`s der Engine.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &self,
        run_id: &str,
        stage: Stage,
        user_task: &str,
        workspace_context: Option<&str>,
        now_unix_ms: i64,
        engine: &mut dyn EngineCall,
        store: &mut dyn AgentRunStore,
        callbacks: &mut RunCallbacks<'_>,
    ) -> Result<RunOutcome, AgentError> {
        let mut run = AgentRun {
            id: run_id.to_owned(),
            stage,
            user_task: user_task.to_owned(),
            created_unix_ms: now_unix_ms,
            updated_unix_ms: now_unix_ms,
            roles: Vec::new(),
            final_answer: None,
            aborted: false,
            early_stopped: false,
        };
        store.upsert(&run)?;
        (callbacks.on_progress)(RunProgress::Started {
            run_id: run_id.to_owned(),
            stage,
        });

        let check_continue = |callbacks: &RunCallbacks<'_>| -> bool {
            // Cast to remove the `mut` — we only read here.
            let ptr =
                callbacks.should_continue as *const dyn FnMut() -> bool as *mut dyn FnMut() -> bool;
            // SAFETY: single-threaded within the run scope.
            unsafe { (*ptr)() }
        };
        let _ = check_continue;

        // --- Proposer -------------------------------------------------
        if !(callbacks.should_continue)() {
            run.aborted = true;
            store.upsert(&run)?;
            (callbacks.on_progress)(RunProgress::Aborted {
                run_id: run_id.to_owned(),
            });
            return Ok(RunOutcome {
                run,
                final_answer: String::new(),
                early_stopped: false,
            });
        }
        let proposer_prompt_value = proposer_prompt(user_task, workspace_context);
        let proposer_result = engine.call(&proposer_prompt_value, None)?;
        let proposer_text = proposer_result.raw_output.clone();
        push_role(
            &mut run,
            &proposer_prompt_value,
            &proposer_result,
            now_unix_ms,
        );
        store.upsert(&run)?;
        emit_finished(callbacks, run_id, Role::Proposer, &proposer_result);

        // --- L0 endet hier -------------------------------------------
        if matches!(stage, Stage::L0Direct) {
            return finalize(run, store, callbacks, run_id, proposer_text, false);
        }

        // --- L1: Self-Check ------------------------------------------
        if matches!(stage, Stage::L1SelfCheck) {
            if !(callbacks.should_continue)() {
                run.aborted = true;
                store.upsert(&run)?;
                (callbacks.on_progress)(RunProgress::Aborted {
                    run_id: run_id.to_owned(),
                });
                return Ok(RunOutcome {
                    run,
                    final_answer: proposer_text,
                    early_stopped: false,
                });
            }
            let check_list: Vec<&str> = self.checklist.iter().map(String::as_str).collect();
            let check_prompt = self_check_prompt(&proposer_text, &check_list);
            let check_result = engine.call(&check_prompt, None)?;
            let final_text = check_result.raw_output.clone();
            push_role(&mut run, &check_prompt, &check_result, now_unix_ms);
            store.upsert(&run)?;
            emit_finished(callbacks, run_id, Role::SelfCheck, &check_result);
            return finalize(run, store, callbacks, run_id, final_text, false);
        }

        // --- L2/L3: Critic -------------------------------------------
        if !(callbacks.should_continue)() {
            run.aborted = true;
            store.upsert(&run)?;
            (callbacks.on_progress)(RunProgress::Aborted {
                run_id: run_id.to_owned(),
            });
            return Ok(RunOutcome {
                run,
                final_answer: proposer_text,
                early_stopped: false,
            });
        }
        let checklist_refs: Vec<&str> = self.checklist.iter().map(String::as_str).collect();
        let critic_prompt_value = critic_prompt(&proposer_text, &checklist_refs);
        let critic_result = engine.call(&critic_prompt_value, Some(grammar::CRITIC_GBNF))?;
        push_role(&mut run, &critic_prompt_value, &critic_result, now_unix_ms);
        store.upsert(&run)?;
        emit_finished(callbacks, run_id, Role::Critic, &critic_result);

        let critic_report = grammar::parse_critic(&critic_result.raw_output)?;
        if !critic_report.triggers_follow_up() {
            run.early_stopped = true;
            store.upsert(&run)?;
            (callbacks.on_progress)(RunProgress::EarlyStopped {
                run_id: run_id.to_owned(),
                reason: "Kritiker hat keinen Befund ≥ medium gefunden".to_owned(),
            });
            return finalize(run, store, callbacks, run_id, proposer_text, true);
        }

        // --- L3: Verifier --------------------------------------------
        let mut verifier_report: Option<VerifierReport> = None;
        if matches!(stage, Stage::L3Full) {
            if !(callbacks.should_continue)() {
                run.aborted = true;
                store.upsert(&run)?;
                (callbacks.on_progress)(RunProgress::Aborted {
                    run_id: run_id.to_owned(),
                });
                return Ok(RunOutcome {
                    run,
                    final_answer: proposer_text,
                    early_stopped: false,
                });
            }
            let verifier_prompt_value = verifier_prompt(&proposer_text, &critic_report);
            let verifier_result =
                engine.call(&verifier_prompt_value, Some(grammar::VERIFIER_GBNF))?;
            push_role(
                &mut run,
                &verifier_prompt_value,
                &verifier_result,
                now_unix_ms,
            );
            store.upsert(&run)?;
            emit_finished(callbacks, run_id, Role::Verifier, &verifier_result);
            verifier_report = Some(grammar::parse_verifier(&verifier_result.raw_output)?);
        }

        // --- Synthesizer ---------------------------------------------
        if !(callbacks.should_continue)() {
            run.aborted = true;
            store.upsert(&run)?;
            (callbacks.on_progress)(RunProgress::Aborted {
                run_id: run_id.to_owned(),
            });
            return Ok(RunOutcome {
                run,
                final_answer: proposer_text,
                early_stopped: false,
            });
        }
        let synth_prompt_value =
            synthesizer_prompt(&proposer_text, &critic_report, verifier_report.as_ref());
        let synth_result = engine.call(&synth_prompt_value, None)?;
        let final_text = synth_result.raw_output.clone();
        push_role(&mut run, &synth_prompt_value, &synth_result, now_unix_ms);
        store.upsert(&run)?;
        emit_finished(callbacks, run_id, Role::Synthesizer, &synth_result);
        finalize(run, store, callbacks, run_id, final_text, false)
    }
}

fn push_role(run: &mut AgentRun, prompt: &RolePrompt, result: &StageResult, now_unix_ms: i64) {
    run.roles.push(RoleTranscript {
        role: prompt.role,
        prompt_system: prompt.system.clone(),
        prompt_user: prompt.user.clone(),
        raw_output: result.raw_output.clone(),
        elapsed_ms: result.elapsed_ms,
        prompt_tokens: result.prompt_tokens,
        completion_tokens: result.completion_tokens,
    });
    run.updated_unix_ms = now_unix_ms + run.roles.len() as i64;
}

fn emit_finished(callbacks: &mut RunCallbacks<'_>, run_id: &str, role: Role, result: &StageResult) {
    (callbacks.on_progress)(RunProgress::RoleFinished {
        run_id: run_id.to_owned(),
        role,
        elapsed_ms: result.elapsed_ms,
        prompt_tokens: result.prompt_tokens,
        completion_tokens: result.completion_tokens,
    });
}

fn finalize(
    mut run: AgentRun,
    store: &mut dyn AgentRunStore,
    callbacks: &mut RunCallbacks<'_>,
    run_id: &str,
    final_answer: String,
    early_stopped: bool,
) -> Result<RunOutcome, AgentError> {
    run.final_answer = Some(final_answer.clone());
    run.early_stopped = early_stopped;
    store.upsert(&run)?;
    (callbacks.on_progress)(RunProgress::Finished {
        run_id: run_id.to_owned(),
        final_answer: final_answer.clone(),
    });
    Ok(RunOutcome {
        run,
        final_answer,
        early_stopped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::InMemoryAgentRunStore;

    struct ScriptedEngine {
        responses: Vec<Result<StageResult, AgentError>>,
    }

    impl EngineCall for ScriptedEngine {
        fn call(
            &mut self,
            _prompt: &RolePrompt,
            _grammar: Option<&str>,
        ) -> Result<StageResult, AgentError> {
            if self.responses.is_empty() {
                return Err(AgentError::Engine("keine Antworten mehr".into()));
            }
            self.responses.remove(0)
        }
    }

    fn ok(text: &str) -> Result<StageResult, AgentError> {
        Ok(StageResult {
            raw_output: text.to_owned(),
            elapsed_ms: 10,
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
        })
    }

    #[test]
    fn l0_returns_proposer_text_unchanged() {
        let mut engine = ScriptedEngine {
            responses: vec![ok("42 ist die Antwort.")],
        };
        let mut store = InMemoryAgentRunStore::new();
        let mut cont = || true;
        let mut prog: Vec<RunProgress> = Vec::new();
        let mut on_prog = |event: RunProgress| prog.push(event);
        let mut callbacks = RunCallbacks {
            should_continue: &mut cont,
            on_progress: &mut on_prog,
        };
        let runner = EscalationRunner::default();
        let outcome = runner
            .run(
                "run-l0",
                Stage::L0Direct,
                "Was ist 42?",
                None,
                1,
                &mut engine,
                &mut store,
                &mut callbacks,
            )
            .unwrap();
        assert_eq!(outcome.final_answer, "42 ist die Antwort.");
        assert!(!outcome.early_stopped);
        assert!(matches!(prog.last(), Some(RunProgress::Finished { .. })));
    }

    #[test]
    fn l2_early_stops_when_critic_finds_nothing() {
        let mut engine = ScriptedEngine {
            responses: vec![ok("Vorschlag: mach X, Y, Z."), ok(r#"{"findings":[]}"#)],
        };
        let mut store = InMemoryAgentRunStore::new();
        let mut cont = || true;
        let mut prog: Vec<RunProgress> = Vec::new();
        let mut on_prog = |event: RunProgress| prog.push(event);
        let mut callbacks = RunCallbacks {
            should_continue: &mut cont,
            on_progress: &mut on_prog,
        };
        let runner = EscalationRunner::default();
        let outcome = runner
            .run(
                "run-l2-early",
                Stage::L2Critique,
                "Prüfe meinen Plan",
                None,
                1,
                &mut engine,
                &mut store,
                &mut callbacks,
            )
            .unwrap();
        assert!(outcome.early_stopped);
        assert_eq!(outcome.final_answer, "Vorschlag: mach X, Y, Z.");
        assert!(prog
            .iter()
            .any(|p| matches!(p, RunProgress::EarlyStopped { .. })));
    }

    #[test]
    fn l2_runs_synth_when_critic_finds_medium_issue() {
        let mut engine = ScriptedEngine {
            responses: vec![
                ok("Vorschlag."),
                ok(
                    r#"{"findings":[{"befund":"unklar","schweregrad":"medium","betrifft":"x","vorschlag":"y"}]}"#,
                ),
                ok("Angepasste Antwort."),
            ],
        };
        let mut store = InMemoryAgentRunStore::new();
        let mut cont = || true;
        let mut on_prog = |_event: RunProgress| {};
        let mut callbacks = RunCallbacks {
            should_continue: &mut cont,
            on_progress: &mut on_prog,
        };
        let runner = EscalationRunner::default();
        let outcome = runner
            .run(
                "run-l2",
                Stage::L2Critique,
                "Bewerte",
                None,
                1,
                &mut engine,
                &mut store,
                &mut callbacks,
            )
            .unwrap();
        assert!(!outcome.early_stopped);
        assert_eq!(outcome.final_answer, "Angepasste Antwort.");
        assert_eq!(outcome.run.roles.len(), 3);
    }

    #[test]
    fn abort_between_roles_keeps_best_intermediate_result() {
        let mut engine = ScriptedEngine {
            responses: vec![ok("Erster Vorschlag.")],
        };
        let mut store = InMemoryAgentRunStore::new();
        // erst true (Proposer läuft), dann false (Abbruch vor Critic)
        let toggle = std::cell::Cell::new(0_u32);
        let mut cont = || {
            let value = toggle.get();
            toggle.set(value + 1);
            value == 0
        };
        let mut on_prog = |_event: RunProgress| {};
        let mut callbacks = RunCallbacks {
            should_continue: &mut cont,
            on_progress: &mut on_prog,
        };
        let runner = EscalationRunner::default();
        let outcome = runner
            .run(
                "run-abort",
                Stage::L2Critique,
                "Analyse",
                None,
                1,
                &mut engine,
                &mut store,
                &mut callbacks,
            )
            .unwrap();
        assert!(outcome.run.aborted);
        assert_eq!(outcome.final_answer, "Erster Vorschlag.");
    }

    #[test]
    fn l3_runs_all_four_roles_when_critic_finds_issue() {
        let mut engine = ScriptedEngine {
            responses: vec![
                ok("Vorschlag."),
                ok(
                    r#"{"findings":[{"befund":"x","schweregrad":"high","betrifft":"y","vorschlag":"z"}]}"#,
                ),
                ok(r#"{"claims":[{"aussage":"2+2=4","status":"confirmed","beleg":"calc"}]}"#),
                ok("Endergebnis."),
            ],
        };
        let mut store = InMemoryAgentRunStore::new();
        let mut cont = || true;
        let mut on_prog = |_event: RunProgress| {};
        let mut callbacks = RunCallbacks {
            should_continue: &mut cont,
            on_progress: &mut on_prog,
        };
        let runner = EscalationRunner::default();
        let outcome = runner
            .run(
                "run-l3",
                Stage::L3Full,
                "Berechne die Marge",
                None,
                1,
                &mut engine,
                &mut store,
                &mut callbacks,
            )
            .unwrap();
        assert_eq!(outcome.run.roles.len(), 4);
        assert_eq!(outcome.final_answer, "Endergebnis.");
    }

    #[test]
    fn persistence_stores_intermediate_state_after_every_role() {
        let mut engine = ScriptedEngine {
            responses: vec![ok("Vorschlag."), ok(r#"{"findings":[]}"#)],
        };
        let mut store = InMemoryAgentRunStore::new();
        let mut cont = || true;
        let mut on_prog = |_event: RunProgress| {};
        let mut callbacks = RunCallbacks {
            should_continue: &mut cont,
            on_progress: &mut on_prog,
        };
        let runner = EscalationRunner::default();
        let _ = runner
            .run(
                "run-persist",
                Stage::L2Critique,
                "Test",
                None,
                1,
                &mut engine,
                &mut store,
                &mut callbacks,
            )
            .unwrap();
        let stored = store.get("run-persist").unwrap().unwrap();
        assert!(
            stored.roles.len() >= 2,
            "Proposer + Critic sollten drin sein"
        );
        assert!(stored.early_stopped);
    }
}
