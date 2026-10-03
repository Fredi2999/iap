use std::{
    collections::VecDeque,
    io::{BufRead, BufReader},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::{config::ServerConfig, InferenceError};

const LOG_LINES: usize = 200;
const REMOVED_ENVIRONMENT: &[&str] = &[
    "LLAMA_ARG_TOOLS",
    "LLAMA_ARG_AGENT",
    "LLAMA_ARG_MCP_SERVERS_CONFIG",
    "LLAMA_ARG_MCP_SERVERS_JSON",
    "HF_TOKEN",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

/// Macht bewussten Stop von einem unerwarteten Prozessende unterscheidbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    Starting,
    Ready,
    Stopped,
    Crashed,
}

/// Besitzt Child und begrenzte Diagnoseringe, damit Ausgaben keine Pipes blockieren.
pub struct ServerProcess {
    child: Child,
    state: ServerState,
    logs: Arc<Mutex<VecDeque<String>>>,
    port: u16,
    api_key: String,
}

impl ServerProcess {
    /// Startet nur die bereits gehärtete Konfiguration und entfernt gefährliche Env-Overrides.
    pub fn spawn(config: &ServerConfig) -> Result<Self, InferenceError> {
        let mut command = Command::new(&config.executable);
        command
            .args(config.arguments())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            // Der Server darf das Ctrl+C des Launchers nicht erben: Das erste
            // Signal schließt nur dessen SSE-Verbindung und bewahrt Teiltext.
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }
        for name in REMOVED_ENVIRONMENT {
            command.env_remove(name);
        }
        let mut child = command.spawn().map_err(|error| InferenceError::Process {
            path: config.executable.clone(),
            reason: error.to_string(),
        })?;
        let logs = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_LINES)));
        if let Some(stdout) = child.stdout.take() {
            drain(stdout, Arc::clone(&logs));
        }
        if let Some(stderr) = child.stderr.take() {
            drain(stderr, Arc::clone(&logs));
        }
        Ok(Self {
            child,
            state: ServerState::Starting,
            logs,
            port: config.port,
            api_key: config.api_key.clone(),
        })
    }

    /// Wartet begrenzt auf `/health` und erkennt einen frühen Child-Exit sofort.
    pub fn wait_ready(&mut self, timeout: Duration) -> Result<(), InferenceError> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.child.try_wait()?.is_some() {
                self.state = ServerState::Crashed;
                return Err(InferenceError::UnexpectedExit);
            }
            if health(self.port, &self.api_key) {
                self.state = ServerState::Ready;
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(InferenceError::Http(
            "llama-server Health-Timeout".to_owned(),
        ))
    }

    /// Beendet bewusst und wartet, damit keine gesperrten Modelldateien zurückbleiben.
    pub fn stop(&mut self) -> Result<(), InferenceError> {
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
            let _ = self.child.wait()?;
        }
        self.state = ServerState::Stopped;
        Ok(())
    }

    /// Prüft den Child ohne einen absichtlichen Stop als Crash umzudeuten.
    pub fn refresh_state(&mut self) -> Result<ServerState, InferenceError> {
        if self.child.try_wait()?.is_some() && self.state != ServerState::Stopped {
            self.state = ServerState::Crashed;
        }
        Ok(self.state)
    }

    /// Gibt höchstens die letzten 200 Zeilen aus und enthält nie den API-Schlüssel.
    pub fn logs(&self) -> Vec<String> {
        self.logs
            .lock()
            .map(|logs| {
                logs.iter()
                    .map(|line| line.replace(&self.api_key, "[REDACTED]"))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Prüft neben der Prozesshülle auch, ob der lokale Server noch HTTP-Anfragen beantwortet.
    pub fn is_healthy(&self) -> bool {
        health(self.port, &self.api_key)
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn drain(reader: impl std::io::Read + Send + 'static, logs: Arc<Mutex<VecDeque<String>>>) {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            if let Ok(mut guard) = logs.lock() {
                if guard.len() == LOG_LINES {
                    guard.pop_front();
                }
                guard.push_back(line);
            }
        }
    });
}

fn health(port: u16, token: &str) -> bool {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let Ok(mut stream) = TcpStream::connect_timeout(&address.into(), Duration::from_millis(200))
    else {
        return false;
    };
    if stream
        .set_read_timeout(Some(Duration::from_millis(200)))
        .is_err()
    {
        return false;
    }
    let request = format!(
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    if std::io::Write::write_all(&mut stream, request.as_bytes()).is_err() {
        return false;
    }
    let mut first_line = String::new();
    BufReader::new(stream).read_line(&mut first_line).is_ok() && first_line.contains(" 200 ")
}
