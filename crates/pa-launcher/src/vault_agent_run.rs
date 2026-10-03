//! Bridge zwischen [`pa_agents::AgentRunStore`] und dem Vault-Tisch
//! `agent_runs` (Schema V4).
//!
//! Konzept 7.3: „nach jeder Rolle wird der Zwischenstand persistiert, damit
//! ein Lauf nach Absturz fortsetzbar ist." Diese Bridge hält keine
//! pa-agents-Typen in pa-vault (Kreisabhängigkeit), sondern serialisiert den
//! `AgentRun` hier zu JSON und persistiert ihn zusammen mit den
//! Indexspalten (`stage`, `updated_unix_ms`, `aborted`, `early_stopped`), die
//! die UI ohne JSON-Parsing nutzen kann.

use std::sync::{Arc, Mutex};

use pa_agents::{persist::AgentRunStore, AgentError, AgentRun};
use pa_vault::{agent_run::AgentRunRow, hot_copy::HotVault};

/// Persistiert Agentenläufe im Vault; nimmt den Vault-Lock nur pro Aufruf.
///
/// Der Runner überschreibt nach jeder Rolle den ganzen Datensatz — ein
/// harter Absturz danach findet beim nächsten Start entweder den vorletzten
/// oder den letzten konsistenten Stand.
pub struct VaultAgentRunStore {
    vault: Arc<Mutex<HotVault>>,
}

impl VaultAgentRunStore {
    /// Bindet den Store an einen bereits geöffneten Vault.
    pub fn new(vault: Arc<Mutex<HotVault>>) -> Self {
        Self { vault }
    }

    fn to_row(run: &AgentRun) -> Result<AgentRunRow, AgentError> {
        let payload = serde_json::to_string(run)
            .map_err(|error| AgentError::Persistence(error.to_string()))?;
        Ok(AgentRunRow {
            id: run.id.clone(),
            stage: run.stage.as_str().to_owned(),
            created_unix_ms: run.created_unix_ms,
            updated_unix_ms: run.updated_unix_ms,
            aborted: run.aborted,
            early_stopped: run.early_stopped,
            payload,
        })
    }

    fn from_row(row: AgentRunRow) -> Result<AgentRun, AgentError> {
        serde_json::from_str::<AgentRun>(&row.payload)
            .map_err(|error| AgentError::Persistence(error.to_string()))
    }
}

impl AgentRunStore for VaultAgentRunStore {
    fn upsert(&mut self, run: &AgentRun) -> Result<(), AgentError> {
        let row = Self::to_row(run)?;
        let mut vault = self
            .vault
            .lock()
            .map_err(|_| AgentError::Persistence("Vault-Mutex vergiftet".to_owned()))?;
        vault
            .repository_mut()
            .upsert_agent_run(&row)
            .map_err(|error| AgentError::Persistence(error.to_string()))
    }

    fn get(&self, id: &str) -> Result<Option<AgentRun>, AgentError> {
        let vault = self
            .vault
            .lock()
            .map_err(|_| AgentError::Persistence("Vault-Mutex vergiftet".to_owned()))?;
        let row = vault
            .repository()
            .agent_run(id)
            .map_err(|error| AgentError::Persistence(error.to_string()))?;
        row.map(Self::from_row).transpose()
    }

    fn list(&self) -> Result<Vec<AgentRun>, AgentError> {
        let vault = self
            .vault
            .lock()
            .map_err(|_| AgentError::Persistence("Vault-Mutex vergiftet".to_owned()))?;
        let rows = vault
            .repository()
            .agent_runs(None)
            .map_err(|error| AgentError::Persistence(error.to_string()))?;
        rows.into_iter().map(Self::from_row).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_agents::{persist::AgentRunStore, InMemoryAgentRunStore, Role, RoleTranscript, Stage};
    use pa_vault::{
        hot_copy::HotVault,
        key::derive_key,
        meta::{Argon2Parameters, VaultMeta},
    };

    fn fast_meta(salt: [u8; 16]) -> VaultMeta {
        VaultMeta::new(
            salt,
            Argon2Parameters {
                memory_kib: 8 * 1024,
                iterations: 1,
                parallelism: 1,
            },
        )
    }

    fn open_vault(dir: &std::path::Path, name: &str) -> HotVault {
        let portable = dir.join(format!("{name}-vault.db"));
        let hot = dir.join(format!("{name}-hot"));
        let meta = fast_meta([5; 16]);
        let key = derive_key("bridge", &meta).unwrap();
        HotVault::start(&portable, &hot, key).unwrap()
    }

    fn sample_run(id: &str, updated: i64) -> AgentRun {
        AgentRun {
            id: id.to_owned(),
            stage: Stage::L2Critique,
            user_task: "Bewerte meinen Plan".to_owned(),
            created_unix_ms: 100,
            updated_unix_ms: updated,
            roles: vec![RoleTranscript {
                role: Role::Proposer,
                prompt_system: "sys".into(),
                prompt_user: "usr".into(),
                raw_output: "vorschlag".into(),
                elapsed_ms: 250,
                prompt_tokens: Some(80),
                completion_tokens: Some(30),
            }],
            final_answer: None,
            aborted: false,
            early_stopped: false,
        }
    }

    #[test]
    fn round_trip_matches_in_memory_store_semantics() {
        let temp = tempfile::tempdir().unwrap();
        let vault = Arc::new(Mutex::new(open_vault(temp.path(), "roundtrip")));
        let mut bridge = VaultAgentRunStore::new(Arc::clone(&vault));
        let mut memory = InMemoryAgentRunStore::new();

        let mut run = sample_run("run-alpha", 10);
        bridge.upsert(&run).unwrap();
        memory.upsert(&run).unwrap();
        assert_eq!(
            bridge.get("run-alpha").unwrap(),
            memory.get("run-alpha").unwrap()
        );

        // Zweiter Rollen-Turn: `updated_unix_ms` steigt, `final_answer` gesetzt.
        run.updated_unix_ms = 20;
        run.final_answer = Some("Ergebnis".into());
        bridge.upsert(&run).unwrap();
        memory.upsert(&run).unwrap();
        assert_eq!(
            bridge.get("run-alpha").unwrap(),
            memory.get("run-alpha").unwrap()
        );
    }

    #[test]
    fn list_returns_newest_first() {
        let temp = tempfile::tempdir().unwrap();
        let vault = Arc::new(Mutex::new(open_vault(temp.path(), "listing")));
        let mut bridge = VaultAgentRunStore::new(Arc::clone(&vault));
        bridge.upsert(&sample_run("older", 100)).unwrap();
        bridge.upsert(&sample_run("newer", 200)).unwrap();
        let listed = bridge.list().unwrap();
        assert_eq!(listed[0].id, "newer");
        assert_eq!(listed[1].id, "older");
    }

    #[test]
    fn run_survives_reopen_of_vault() {
        let temp = tempfile::tempdir().unwrap();
        let vault_path = temp.path().join("survive-vault.db");
        let hot_path = temp.path().join("survive-hot");
        let meta = fast_meta([7; 16]);
        let key = derive_key("bridge", &meta).unwrap();
        let first = HotVault::start(&vault_path, &hot_path, key).unwrap();
        let first_arc = Arc::new(Mutex::new(first));
        VaultAgentRunStore::new(Arc::clone(&first_arc))
            .upsert(&sample_run("survive", 42))
            .unwrap();
        // Sauber schließen (schreibt zurück auf den Stick).
        let owned = Arc::try_unwrap(first_arc)
            .map_err(|_| ())
            .expect("kein zweiter Referent");
        owned
            .into_inner()
            .unwrap()
            .shutdown(&mut pa_vault::hot_copy::NoFault)
            .unwrap();

        // Vault mit demselben Schlüssel neu öffnen; der Lauf muss noch da sein.
        let key = derive_key("bridge", &meta).unwrap();
        let second = HotVault::start(&vault_path, &hot_path, key).unwrap();
        let second_arc = Arc::new(Mutex::new(second));
        let bridge = VaultAgentRunStore::new(second_arc);
        let restored = bridge.get("survive").unwrap().unwrap();
        assert_eq!(restored.updated_unix_ms, 42);
        assert_eq!(restored.user_task, "Bewerte meinen Plan");
    }
}
