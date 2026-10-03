use std::time::Duration;

use crate::{
    config::ServerConfig,
    process::{ServerProcess, ServerState},
    InferenceError,
};

/// Begrenzt automatische Restarts und führt Modellwechsel als Stop-then-Ready-Transaktion aus.
pub struct Supervisor {
    config: ServerConfig,
    process: Option<ServerProcess>,
    restart_count: u8,
    max_restarts: u8,
}

/// Unterscheidet einen echten Restart von einem unverändert gesunden Prozess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureStatus {
    AlreadyRunning,
    Restarted,
}

impl Supervisor {
    /// Startet mit höchstens drei automatischen Wiederanläufen pro Supervisor-Lebenszeit.
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            process: None,
            restart_count: 0,
            max_restarts: 3,
        }
    }

    /// Meldet Erfolg erst nach einem realen lokalen Health-Check.
    pub fn start(&mut self, timeout: Duration) -> Result<(), InferenceError> {
        let mut process = ServerProcess::spawn(&self.config)?;
        process.wait_ready(timeout)?;
        self.process = Some(process);
        Ok(())
    }

    /// Startet nach Prozessende oder fehlgeschlagenem Health-Check begrenzt neu.
    pub fn ensure_running(&mut self, timeout: Duration) -> Result<EnsureStatus, InferenceError> {
        let needs_restart = match self.process.as_mut() {
            Some(process) => {
                process.refresh_state()? == ServerState::Crashed || !process.is_healthy()
            }
            None => true,
        };
        if !needs_restart {
            return Ok(EnsureStatus::AlreadyRunning);
        }
        if self.restart_count >= self.max_restarts {
            return Err(InferenceError::UnexpectedExit);
        }
        self.stop()?;
        std::thread::sleep(Duration::from_millis(100 * (1_u64 << self.restart_count)));
        self.restart_count += 1;
        self.start(timeout)?;
        Ok(EnsureStatus::Restarted)
    }

    /// Beendet den alten Child vollständig, bevor das neue Modell sichtbar Ready wird.
    pub fn switch_model(
        &mut self,
        config: ServerConfig,
        timeout: Duration,
    ) -> Result<(), InferenceError> {
        self.stop()?;
        self.config = config;
        self.restart_count = 0;
        self.start(timeout)
    }

    /// Schließt einen laufenden Server bewusst und unterdrückt dadurch Restart.
    pub fn stop(&mut self) -> Result<(), InferenceError> {
        if let Some(mut process) = self.process.take() {
            process.stop()?;
        }
        Ok(())
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
