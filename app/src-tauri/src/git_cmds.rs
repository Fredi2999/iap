//! Git im Code-Bereich: Status, Verlauf und Commit über das gebündelte MinGit.
//!
//! Warum nicht mehr das Git aus dem PATH: Jeder Prozessstart muss durch pa-policy
//! (Invariante 2). `GitCli` prüft Programm, Unterbefehl, Optionen und Pfade, leert
//! die Umgebung und schaltet Hooks sowie Netz ab. Ohne Git-Paket meldet der Bereich
//! das ehrlich, statt still ein Host-Git zu benutzen.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pa_code::git::{
    cli::{simplify_path, AuditHook, GitCall, GitCli},
    flow::is_sensitive,
    GitError,
};
use pa_policy::{CapabilityAction, Decision, PathScope};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::{lifecycle, AppError, AppResult, AppState};

/// Eintrag der Statusliste: `status` ist das Git-Kürzel ohne Leerraum (`M`, `A`, `D`, `??`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GitStatusEntry {
    pub status: String,
    pub relative_path: String,
}

/// Ein Commit des Verlaufs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GitCommit {
    pub id: String,
    pub summary: String,
    pub author: String,
    pub unix_ts: i64,
}

/// Zustand der Git-Anbindung für die Oberfläche.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GitInfo {
    /// `true`, wenn Git-Paket und Repository im Arbeitsordner vorhanden sind.
    pub available: bool,
    /// Aktueller Zweig, soweit bekannt.
    pub branch: Option<String>,
    /// Warum Git nicht verfügbar ist (für die Anzeige).
    pub note: Option<String>,
}

/// Höchstzahl Pfade je `git add`, damit die Argumentliste klein bleibt.
const ADD_CHUNK: usize = 100;

/// Ein Git-Läufer für den Arbeitsordner. Der private Ordner für Hooks und
/// globale Konfiguration liegt im Temp-Ordner und verschwindet mit dem Läufer.
pub struct GitWorkspace {
    cli: GitCli,
    root: PathBuf,
    /// Pfad des Arbeitsordners innerhalb des Repositories (leer, wenn er die Wurzel ist).
    prefix: String,
    scratch: PathBuf,
}

impl Drop for GitWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

impl GitWorkspace {
    /// Öffnet den Läufer. `Ok(None)`, wenn der Arbeitsordner kein Repository ist.
    pub fn open(
        program: &Path,
        workspace: &Path,
        audit: Option<AuditHook>,
    ) -> Result<Option<Self>, GitError> {
        let scope = PathScope::new(workspace).map_err(|e| GitError::Policy(e.to_string()))?;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let scratch =
            std::env::temp_dir().join(format!("iap-code-git-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&scratch)?;
        let scratch_scope =
            PathScope::new(&scratch).map_err(|e| GitError::Policy(e.to_string()))?;
        let root = simplify_path(scope.root());
        let cli = GitCli::new(
            program,
            vec![scope, scratch_scope.clone()],
            &simplify_path(scratch_scope.root()),
            audit,
        );
        let cli = match cli {
            Ok(cli) => cli,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&scratch);
                return Err(error);
            }
        };
        // Drop räumt ab hier auf, auch bei frühem Verlassen.
        let mut git = Self {
            cli,
            root,
            prefix: String::new(),
            scratch,
        };
        match git.run(&["rev-parse", "--is-inside-work-tree"]) {
            Ok(text) if text.trim() == "true" => {}
            Ok(_) => return Ok(None),
            Err(GitError::Failed { .. }) => return Ok(None),
            Err(error) => return Err(error),
        }
        git.prefix = git.run(&["rev-parse", "--show-prefix"])?.trim().to_owned();
        Ok(Some(git))
    }

    fn run(&self, args: &[&str]) -> Result<String, GitError> {
        Ok(self.cli.run(GitCall::new(&self.root, args))?.text())
    }

    /// Zweigname; auch in einem Repository ohne Commits.
    pub fn branch(&self) -> Option<String> {
        self.run(&["symbolic-ref", "--short", "HEAD"])
            .ok()
            .filter(|name| !name.is_empty())
    }

    /// Geänderte und neue Dateien des Arbeitsordners (ohne ignorierte).
    pub fn status(&self) -> Result<Vec<GitStatusEntry>, GitError> {
        let out = self.cli.run(GitCall::new(
            &self.root,
            &["status", "--porcelain=v1", "-z", "-uall"],
        ))?;
        Ok(parse_status(&out.stdout, &self.prefix))
    }

    /// Letzte `limit` Commits; leer in einem Repository ohne Commits.
    pub fn log(&self, limit: u32) -> Result<Vec<GitCommit>, GitError> {
        let limit_arg = format!("-n{}", limit.clamp(1, 200));
        let raw = match self.run(&["log", &limit_arg, "--pretty=format:%H%x1f%s%x1f%an%x1f%at"]) {
            Ok(raw) => raw,
            Err(GitError::Failed { stderr, .. })
                if stderr.contains("does not have any commits") =>
            {
                return Ok(Vec::new())
            }
            Err(error) => return Err(error),
        };
        Ok(parse_log(&raw))
    }

    /// Legt einen Commit an. Leere `paths` heißt: alle Änderungen des Arbeitsordners, aber
    /// ohne Dateien, die nach Geheimnissen aussehen (`.env`, Schlüssel); die müssen bewusst
    /// einzeln gewählt werden.
    pub fn commit(&self, message: &str, paths: &[String]) -> Result<String, GitError> {
        let message = message.trim();
        if message.is_empty() {
            return Err(GitError::Repo(
                "Die Commit-Nachricht darf nicht leer sein.".to_owned(),
            ));
        }
        if message.contains(['\n', '\r', '\0']) {
            return Err(GitError::Repo(
                "Die Commit-Nachricht darf nur eine Zeile haben.".to_owned(),
            ));
        }
        let selected: Vec<String> = if paths.is_empty() {
            self.status()?
                .into_iter()
                .map(|entry| entry.relative_path)
                .filter(|path| !is_sensitive(path))
                .collect()
        } else {
            paths
                .iter()
                .map(|path| {
                    pa_code::patch::validate_relative_path(path.trim_end_matches('/'))
                        .map_err(GitError::from)
                })
                .collect::<Result<_, _>>()?
        };
        if selected.is_empty() {
            return Err(GitError::Repo("Es gibt nichts zu committen.".to_owned()));
        }
        for chunk in selected.chunks(ADD_CHUNK) {
            let mut args = vec!["add", "--"];
            args.extend(chunk.iter().map(String::as_str));
            self.run(&args)?;
        }
        self.run(&["commit", "-m", message])?;
        self.run(&["rev-parse", "--short", "HEAD"])
    }
}

/// Liest `git status --porcelain=v1 -z`. Pfade sind relativ zur Repository-Wurzel;
/// `prefix` begrenzt auf den Arbeitsordner und schneidet ihn ab.
fn parse_status(raw: &[u8], prefix: &str) -> Vec<GitStatusEntry> {
    let text = String::from_utf8_lossy(raw).into_owned();
    let mut parts = text.split('\0').filter(|part| !part.is_empty());
    let mut entries = Vec::new();
    while let Some(part) = parts.next() {
        if part.len() < 4 {
            continue;
        }
        let code = &part[..2];
        let path = &part[3..];
        if code.starts_with('R') || code.starts_with('C') {
            // Bei Umbenennung folgt der alte Pfad als eigener Eintrag.
            let _ = parts.next();
        }
        if code == "!!" {
            continue;
        }
        let Some(relative) = path.strip_prefix(prefix) else {
            continue;
        };
        entries.push(GitStatusEntry {
            status: code.trim().to_owned(),
            relative_path: relative.to_owned(),
        });
    }
    entries
}

fn parse_log(raw: &str) -> Vec<GitCommit> {
    raw.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\u{1f}').collect();
            (parts.len() >= 4).then(|| GitCommit {
                id: parts[0].to_owned(),
                summary: parts[1].to_owned(),
                author: parts[2].to_owned(),
                unix_ts: parts[3].parse().unwrap_or(0),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Anbindung an die App
// ---------------------------------------------------------------------------

fn audit_hook(app: &AppHandle) -> AuditHook {
    let handle = app.clone();
    Arc::new(move |subcommand: &str, decision: &Decision| {
        let state = handle.state::<AppState>();
        lifecycle::audit_decision(
            &state,
            CapabilityAction::GitOp,
            Some(subcommand.to_owned()),
            decision,
            "Git-Aufruf im Code-Bereich",
        );
    })
}

fn invalid(error: impl std::fmt::Display) -> AppError {
    AppError::Invalid(error.to_string())
}

/// Öffnet den Läufer. `Err(Some(Hinweis))`: Git steht nicht zur Verfügung (kein Paket).
fn open(app: &AppHandle) -> AppResult<Result<GitWorkspace, String>> {
    let state = app.state::<AppState>();
    let workspace = crate::code_roots::active_root(&state)?.path;
    let program = match crate::agent_flow::git_program(&state) {
        Ok(program) => program,
        Err(_) => {
            return Ok(Err(
                "Das Git-Paket ist nicht eingerichtet (Einstellungen, Pakete).".to_owned(),
            ))
        }
    };
    match GitWorkspace::open(&program, &workspace, Some(audit_hook(app))).map_err(invalid)? {
        Some(git) => Ok(Ok(git)),
        None => Ok(Err("Der Arbeitsordner ist kein Git-Repository.".to_owned())),
    }
}

fn require(app: &AppHandle) -> AppResult<GitWorkspace> {
    open(app)?.map_err(AppError::Invalid)
}

/// Meldet, ob Git im Code-Bereich nutzbar ist, mit Zweig oder Grund.
#[tauri::command(async)]
pub async fn git_info(app: AppHandle) -> AppResult<GitInfo> {
    Ok(match open(&app)? {
        Ok(git) => GitInfo {
            available: true,
            branch: git.branch(),
            note: None,
        },
        Err(note) => GitInfo {
            available: false,
            branch: None,
            note: Some(note),
        },
    })
}

/// Geänderte und neue Dateien des Arbeitsordners.
#[tauri::command(async)]
pub async fn git_status(app: AppHandle) -> AppResult<Vec<GitStatusEntry>> {
    require(&app)?.status().map_err(invalid)
}

/// Letzte `limit` Commits (Standard: 20).
#[tauri::command(async)]
pub async fn git_log(app: AppHandle, limit: Option<u32>) -> AppResult<Vec<GitCommit>> {
    require(&app)?.log(limit.unwrap_or(20)).map_err(invalid)
}

/// Legt einen Commit an; leere `paths` = alle Änderungen ohne Geheimnis-Dateien.
#[tauri::command(async)]
pub async fn git_commit(app: AppHandle, message: String, paths: Vec<String>) -> AppResult<String> {
    require(&app)?.commit(&message, &paths).map_err(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Git für die Tests: `IAP_GIT` oder das Git des Systems. Ohne beides werden die
    /// Prozesstests übersprungen und das auf der Konsole gemeldet.
    fn git_program() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("IAP_GIT") {
            return Some(PathBuf::from(path));
        }
        [
            "/usr/bin/git",
            "/usr/local/bin/git",
            "/opt/homebrew/bin/git",
        ]
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
    }

    fn raw_git(cwd: &Path, args: &[&str]) {
        let status = Command::new(git_program().expect("Git"))
            .args(["-c", "user.name=Test", "-c", "user.email=t@example.org"])
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("git startet");
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn status_lines_are_parsed_and_limited_to_the_workspace() {
        let raw = b" M src/a.rs\0?? notes.md\0R  neu.txt\0alt.txt\0!! target/\0 D weg.txt\0";
        let all = parse_status(raw, "");
        let names: Vec<(&str, &str)> = all
            .iter()
            .map(|e| (e.status.as_str(), e.relative_path.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("M", "src/a.rs"),
                ("??", "notes.md"),
                ("R", "neu.txt"),
                ("D", "weg.txt")
            ]
        );
        // Arbeitsordner `src/` im größeren Repository: nur seine Dateien, ohne Präfix.
        let inner = parse_status(raw, "src/");
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0].relative_path, "a.rs");
    }

    #[test]
    fn log_lines_are_parsed() {
        let raw = "abc\u{1f}Erster\u{1f}IAP\u{1f}1700000000\nkaputt";
        let commits = parse_log(raw);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].summary, "Erster");
        assert_eq!(commits[0].unix_ts, 1_700_000_000);
    }

    #[test]
    fn a_folder_without_repository_is_reported_as_not_available() {
        let Some(program) = git_program() else {
            eprintln!("Git nicht gefunden: Test übersprungen");
            return;
        };
        let dir = tempfile::tempdir().expect("Ordner");
        assert!(GitWorkspace::open(&program, dir.path(), None)
            .expect("kein Fehler")
            .is_none());
    }

    #[test]
    fn status_log_and_commit_run_through_the_policy_runner() {
        let Some(program) = git_program() else {
            eprintln!("Git nicht gefunden: Test übersprungen");
            return;
        };
        let dir = tempfile::tempdir().expect("Ordner");
        raw_git(dir.path(), &["init", "-q"]);
        std::fs::write(dir.path().join("a.txt"), "eins\n").expect("Datei");
        std::fs::write(dir.path().join(".env"), "GEHEIM=1\n").expect("Datei");

        let audited = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = Arc::clone(&audited);
        let hook: AuditHook = Arc::new(move |sub: &str, _: &Decision| {
            sink.lock().expect("Mutex").push(sub.to_owned());
        });
        let git = GitWorkspace::open(&program, dir.path(), Some(hook))
            .expect("öffnen")
            .expect("Repository");

        // Leeres Repository: kein Fehler, nur eine leere Liste.
        assert!(git.log(10).expect("log").is_empty());
        let status = git.status().expect("status");
        assert_eq!(status.len(), 2);
        assert!(status.iter().all(|e| e.status == "??"));

        // „Alles committen“ lässt die Geheimnis-Datei aus.
        git.commit("Start", &[]).expect("commit");
        let log = git.log(10).expect("log");
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].summary, "Start");
        let after = git.status().expect("status");
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].relative_path, ".env");

        // Mehrzeilige Nachricht und Pfadausbruch werden abgelehnt.
        assert!(git.commit("zwei\nZeilen", &[]).is_err());
        assert!(git.commit("x", &["../draussen.txt".to_owned()]).is_err());
        assert!(git.commit("   ", &[]).is_err());

        // Jeder Aufruf lief durch den Audit-Hook.
        let seen = audited.lock().expect("Mutex");
        assert!(seen.iter().any(|s| s == "commit") && seen.iter().any(|s| s == "status"));
    }
}
