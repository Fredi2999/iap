//! Git-Aufrufe über das gebündelte MinGit, ausschließlich mit Policy-Freigabe.
//!
//! `gix` liest, kann Worktrees aber noch nicht vollständig anlegen (siehe
//! Konzept 10.5). Für Schreibvorgänge im Worktree-Betrieb läuft deshalb ein
//! begrenztes Git-Paket. Jeder Aufruf geht durch
//! `pa_policy::process::authorize_git`: fester Programmpfad, Unterbefehl-
//! Allowlist, Pflichtoptionen gegen Hooks und Netz, deklarierte Pfade im Scope,
//! geleerte Umgebung, Zeitgrenze. Das ist **keine** Prozess-Sandbox; Netz und
//! Dateizugriff des Prozesses selbst werden nicht vom Betriebssystem begrenzt.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use pa_policy::{
    process::{authorize_git, GitVerdict, ProcessPolicy, ProcessSpec},
    Decision, PathScope,
};

use super::GitError;

/// Windows-Flag: kein Konsolenfenster für Hilfsprozesse.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Obergrenze für die Ausgabe eines einzelnen Aufrufs.
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

/// Rückmeldung für das Audit-Log: Unterbefehl (ohne Pfade) und Entscheidung.
pub type AuditHook = Arc<dyn Fn(&str, &Decision) + Send + Sync>;

/// Ergebnis eines Aufrufs.
#[derive(Debug, Clone)]
pub struct GitOutput {
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub code: i32,
}

impl GitOutput {
    /// Ausgabe als Text (verlustbehaftet), ohne abschließenden Zeilenumbruch.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim_end().to_owned()
    }
}

/// Ein Aufruf.
pub struct GitCall<'a> {
    pub cwd: &'a Path,
    pub args: Vec<String>,
    /// Absolute Pfade, die in `args` oder `env` vorkommen.
    pub declared: Vec<PathBuf>,
    /// Zusätzliche Umgebungsvariablen (nur erlaubte Schlüssel).
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    pub cancel: Option<&'a AtomicBool>,
}

impl<'a> GitCall<'a> {
    /// Aufruf ohne Sonderpfade mit 60 Sekunden Zeitgrenze.
    pub fn new(cwd: &'a Path, args: &[&str]) -> Self {
        Self {
            cwd,
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            declared: Vec::new(),
            env: Vec::new(),
            timeout: Duration::from_secs(60),
            cancel: None,
        }
    }
}

/// Der geprüfte Git-Läufer.
pub struct GitCli {
    policy: ProcessPolicy,
    program: PathBuf,
    hooks_dir: PathBuf,
    global_config: PathBuf,
    tool_path: String,
    audit: Option<AuditHook>,
}

impl GitCli {
    /// Bindet den Läufer an das MinGit-Programm (`…/cmd/git.exe`).
    ///
    /// `private_dir` liegt in einer der Wurzeln (`scopes`) und nimmt einen leeren
    /// Hook-Ordner sowie eine leere globale Konfiguration auf, damit weder Hooks
    /// noch Nutzer- oder Systemkonfiguration des Hosts wirken.
    pub fn new(
        program: &Path,
        scopes: Vec<PathScope>,
        private_dir: &Path,
        audit: Option<AuditHook>,
    ) -> Result<Self, GitError> {
        let hooks_dir = private_dir.join("nohooks");
        std::fs::create_dir_all(&hooks_dir)?;
        let global_config = private_dir.join("global.gitconfig");
        if !global_config.exists() {
            std::fs::write(&global_config, b"")?;
        }
        let policy = ProcessPolicy::new(program, scopes, 10 * 60 * 1000)
            .map_err(|e| GitError::Policy(e.to_string()))?;
        // MinGit bringt seine Hilfsprogramme in `mingw64\bin` und `usr\bin` mit.
        let root = program
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| GitError::Process("MinGit-Pfad ungültig".to_owned()))?;
        let tool_path = [
            root.join("mingw64").join("bin"),
            root.join("usr").join("bin"),
            root.join("cmd"),
        ]
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(";");
        Ok(Self {
            policy,
            program: program.to_path_buf(),
            hooks_dir,
            global_config,
            tool_path,
            audit,
        })
    }

    /// Führt einen Aufruf nach Prüfung aus.
    pub fn run(&self, call: GitCall<'_>) -> Result<GitOutput, GitError> {
        let subcommand = call
            .args
            .iter()
            .find(|a| !a.starts_with('-'))
            .cloned()
            .unwrap_or_default();
        let mut args: Vec<String> = vec![
            "-c".to_owned(),
            format!("core.hooksPath={}", self.hooks_dir.display()),
            "-c".to_owned(),
            "core.fsmonitor=false".to_owned(),
            "-c".to_owned(),
            "protocol.allow=never".to_owned(),
            "-c".to_owned(),
            "commit.gpgsign=false".to_owned(),
            "-c".to_owned(),
            "user.name=IAP".to_owned(),
            "-c".to_owned(),
            "user.email=iap@localhost".to_owned(),
        ];
        args.extend(call.args.iter().cloned());

        let mut declared = call.declared.clone();
        declared.push(self.hooks_dir.clone());
        let mut env = vec![
            ("GIT_CONFIG_NOSYSTEM".to_owned(), "1".to_owned()),
            (
                "GIT_CONFIG_GLOBAL".to_owned(),
                self.global_config.to_string_lossy().into_owned(),
            ),
            ("GIT_TERMINAL_PROMPT".to_owned(), "0".to_owned()),
            ("GIT_OPTIONAL_LOCKS".to_owned(), "0".to_owned()),
            ("LC_ALL".to_owned(), "C".to_owned()),
            ("PATH".to_owned(), self.tool_path.clone()),
        ];
        for key in ["SYSTEMROOT", "TEMP", "TMP"] {
            if let Ok(value) = std::env::var(key) {
                env.push((key.to_owned(), value));
            }
        }
        env.extend(call.env.iter().cloned());

        let spec = ProcessSpec {
            program: self.program.clone(),
            args,
            cwd: call.cwd.to_path_buf(),
            declared_paths: declared,
            env,
            clear_env: true,
            timeout_ms: u64::try_from(call.timeout.as_millis()).unwrap_or(u64::MAX),
        };
        let verdict = authorize_git(&self.policy, &spec);
        if let Some(audit) = &self.audit {
            audit(&subcommand, &verdict.decision());
        }
        let process = match verdict {
            GitVerdict::Allowed(process) => process,
            GitVerdict::Denied(reason) => return Err(GitError::Policy(reason)),
        };

        let mut command = Command::new(process.program());
        command
            .args(process.args())
            .current_dir(simplify_path(process.cwd()))
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in process.env() {
            command.env(key, value);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|e| GitError::Process(format!("git startet nicht: {e}")))?;
        let stdout = read_limited(child.stdout.take());
        let stderr = read_limited(child.stderr.take());

        let deadline = Instant::now() + Duration::from_millis(process.timeout_ms());
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(e) => return Err(GitError::Process(e.to_string())),
            }
            if call.cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GitError::Cancelled);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GitError::Timeout(subcommand));
            }
            thread::sleep(Duration::from_millis(15));
        };
        let stdout = stdout.join().unwrap_or_default();
        let stderr = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).into_owned();
        let code = status.code().unwrap_or(-1);
        if code != 0 {
            return Err(GitError::Failed {
                subcommand,
                code,
                stderr: stderr.trim().to_owned(),
            });
        }
        Ok(GitOutput {
            stdout,
            stderr,
            code,
        })
    }
}

/// Entfernt das Windows-Präfix `\\?\` von Laufwerkspfaden. Die Policy arbeitet mit
/// kanonischen Pfaden; als Arbeitsverzeichnis eines Prozesses sind sie unzuverlässig.
pub fn simplify_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// Liest den Kanal vollständig leer (sonst blockiert Git bei voller Pipe), behält aber
/// höchstens [`MAX_OUTPUT_BYTES`].
fn read_limited<R: Read + Send + 'static>(stream: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut out = Vec::new();
        let Some(mut stream) = stream else {
            return out;
        };
        let mut chunk = [0_u8; 64 * 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let room = MAX_OUTPUT_BYTES.saturating_sub(out.len());
                    out.extend_from_slice(&chunk[..read.min(room)]);
                }
            }
        }
        out
    })
}
