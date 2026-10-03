//! Zwischenstand-Persistenz für Agentenläufe.
//!
//! Konzept 7.1 + 8.5 (`on_agent_stage_done`): „Zwischenstand nach jeder
//! Rolle persistieren, damit ein Lauf nach Absturz fortsetzbar ist."
//! Diese Datei definiert nur den Trait und einen In-Memory-Store; die
//! Vault-Anbindung folgt später in einer eigenen Session (analog zur
//! `VaultAuditSink`-Bridge aus Phase 2).

use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use crate::{roles::RoleTranscript, runner::Stage, AgentError};

/// Vollständiger Lauf einer Eskalationsstufe.
///
/// Wird bei jedem Rollen-Ende überschrieben. Der letzte gespeicherte
/// Zustand vor einem Absturz ist damit rekonstruierbar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: String,
    pub stage: Stage,
    pub user_task: String,
    pub created_unix_ms: i64,
    pub updated_unix_ms: i64,
    pub roles: Vec<RoleTranscript>,
    pub final_answer: Option<String>,
    pub aborted: bool,
    pub early_stopped: bool,
}

/// Speicher für Agentenläufe.
///
/// `AgentError::Persistence` bleibt der einzige Fehlerkanal; Aufrufer
/// entscheiden, ob sie den Lauf trotzdem fortsetzen oder abbrechen.
pub trait AgentRunStore: Send + Sync {
    fn upsert(&mut self, run: &AgentRun) -> Result<(), AgentError>;
    fn get(&self, id: &str) -> Result<Option<AgentRun>, AgentError>;
    fn list(&self) -> Result<Vec<AgentRun>, AgentError>;
}

/// Prozessinterner Speicher — für Tests und Prototypen, bis der
/// Vault-basierte Store dazu kommt.
#[derive(Default)]
pub struct InMemoryAgentRunStore {
    inner: Mutex<Vec<AgentRun>>,
}

impl InMemoryAgentRunStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AgentRunStore for InMemoryAgentRunStore {
    fn upsert(&mut self, run: &AgentRun) -> Result<(), AgentError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|error: PoisonError<_>| AgentError::Persistence(error.to_string()))?;
        if let Some(existing) = inner.iter_mut().find(|entry| entry.id == run.id) {
            *existing = run.clone();
        } else {
            inner.push(run.clone());
        }
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<AgentRun>, AgentError> {
        let inner = self
            .inner
            .lock()
            .map_err(|error| AgentError::Persistence(error.to_string()))?;
        Ok(inner.iter().find(|run| run.id == id).cloned())
    }

    fn list(&self) -> Result<Vec<AgentRun>, AgentError> {
        let inner = self
            .inner
            .lock()
            .map_err(|error| AgentError::Persistence(error.to_string()))?;
        Ok(inner.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::Role;

    fn sample_run(id: &str) -> AgentRun {
        AgentRun {
            id: id.to_owned(),
            stage: Stage::L2Critique,
            user_task: "Bewerte meinen Plan".into(),
            created_unix_ms: 1,
            updated_unix_ms: 2,
            roles: vec![RoleTranscript {
                role: Role::Proposer,
                prompt_system: "sys".into(),
                prompt_user: "usr".into(),
                raw_output: "answer".into(),
                elapsed_ms: 100,
                prompt_tokens: Some(50),
                completion_tokens: Some(80),
            }],
            final_answer: None,
            aborted: false,
            early_stopped: false,
        }
    }

    #[test]
    fn upsert_inserts_new_run() {
        let mut store = InMemoryAgentRunStore::new();
        store.upsert(&sample_run("run-1")).unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(store.get("run-1").unwrap().unwrap().id, "run-1");
    }

    #[test]
    fn upsert_replaces_existing_run_by_id() {
        let mut store = InMemoryAgentRunStore::new();
        let mut run = sample_run("run-1");
        store.upsert(&run).unwrap();
        run.final_answer = Some("finale Antwort".into());
        store.upsert(&run).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].final_answer.as_deref(), Some("finale Antwort"));
    }
}
