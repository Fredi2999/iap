//! Prüfung externer Prozesse – derzeit ausschließlich das gebündelte MinGit
//! für den Worktree-Betrieb von Agent Flow (Konzept 10.5, Punkt 2).
//!
//! Ein Worktree ist keine Prozess-Sandbox. Deshalb gibt es hier **keine**
//! allgemeine Prozessfreigabe: genau ein Programm (das Git-Paket), eine feste
//! Liste von Unterbefehlen, geprüfte Pfade, geleerte Umgebung und eine
//! Zeitgrenze. Native Projekt-Tests (`cargo test` & Co.) haben bewusst
//! **keinen** Pfad; sie bleiben „nicht ausgeführt“, bis eine Netz- und
//! Pfad-Sandbox nachgewiesen ist.
//!
//! Ein [`AuthorizedProcess`] lässt sich nur über [`authorize_git`] erzeugen.
//! Der Ausführer nimmt nur diesen Nachweis entgegen und kann die
//! Prüfung so nicht umgehen.

use std::path::{Path, PathBuf};

use crate::{
    capability::{Capability, CapabilityAction, Decision},
    path::{normalize, resolve_absolute_in_scope, PathScope},
    PolicyError,
};

/// Erlaubte Git-Unterbefehle. Alles Netzwerkfähige (push, fetch, pull, clone,
/// remote, submodule) fehlt absichtlich, ebenso `stash`, `checkout`, `reset`
/// und `config`: Der ursprüngliche Arbeitsbaum darf nie verändert werden.
const ALLOWED_SUBCOMMANDS: &[&str] = &[
    "rev-parse",
    "status",
    "diff",
    "log",
    "show",
    "add",
    "commit",
    "commit-tree",
    "write-tree",
    "read-tree",
    "update-ref",
    "branch",
    "ls-files",
    "ls-tree",
    "cat-file",
    "check-ignore",
    "hash-object",
    "for-each-ref",
    "symbolic-ref",
    "worktree",
];

/// Erlaubte Unterbefehle von `git worktree`.
const ALLOWED_WORKTREE_SUB: &[&str] = &["add", "remove", "prune", "repair", "list"];

/// Optionen, die andere Programme starten oder außerhalb schreiben können.
const FORBIDDEN_OPTION_PREFIXES: &[&str] = &[
    "--exec",
    "--upload-pack",
    "--receive-pack",
    "--ext-diff",
    "--textconv",
    "--output",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--super-prefix",
];

/// `-c`-Schlüssel, die der Aufrufer setzen darf.
const ALLOWED_CONFIG_KEYS: &[&str] = &[
    "core.hooksPath",
    "core.fsmonitor",
    "protocol.allow",
    "core.longpaths",
    "core.symlinks",
    "core.autocrlf",
    "user.name",
    "user.email",
    "commit.gpgsign",
];

/// Umgebungsvariablen, die der Aufrufer setzen darf.
const ALLOWED_ENV_KEYS: &[&str] = &[
    "GIT_INDEX_FILE",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_CONFIG_GLOBAL",
    "GIT_TERMINAL_PROMPT",
    "GIT_OPTIONAL_LOCKS",
    "LC_ALL",
    "PATH",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
];

/// Was für jeden Aufruf gelten muss, damit keine Hooks, kein Netz und kein
/// Dateisystem-Monitor mitlaufen.
const REQUIRED_CONFIG: &[(&str, &str)] =
    &[("core.fsmonitor", "false"), ("protocol.allow", "never")];

/// Fest verdrahtete Regeln für den Git-Betrieb.
#[derive(Debug, Clone)]
pub struct ProcessPolicy {
    program: PathBuf,
    scopes: Vec<PathScope>,
    max_timeout_ms: u64,
}

impl ProcessPolicy {
    /// Bindet die Policy an das kanonische Git-Programm und die erlaubten
    /// Wurzeln (Repository, Worktree-Ordner).
    ///
    /// # Errors
    /// `PolicyError::Io`, wenn das Programm nicht existiert.
    pub fn new(
        program: impl AsRef<Path>,
        scopes: Vec<PathScope>,
        max_timeout_ms: u64,
    ) -> Result<Self, PolicyError> {
        Ok(Self {
            program: std::fs::canonicalize(program.as_ref())?,
            scopes,
            max_timeout_ms,
        })
    }
}

/// Ein Prozessstart vor der Prüfung.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    /// Aufzurufendes Programm.
    pub program: PathBuf,
    /// Argumente ohne Programmnamen.
    pub args: Vec<String>,
    /// Absolutes Arbeitsverzeichnis.
    pub cwd: PathBuf,
    /// Absolute Pfade, die in `args` oder `env` vorkommen dürfen (z. B. das
    /// Ziel von `worktree add`). Jeder muss in einer erlaubten Wurzel liegen.
    pub declared_paths: Vec<PathBuf>,
    /// Zusätzliche Umgebungsvariablen (die Umgebung wird sonst geleert).
    pub env: Vec<(String, String)>,
    /// Muss `true` sein: der Prozess erbt nichts vom Host.
    pub clear_env: bool,
    /// Zeitgrenze in Millisekunden.
    pub timeout_ms: u64,
}

/// Nachweis einer bestandenen Prüfung.
#[derive(Debug, Clone)]
pub struct AuthorizedProcess {
    program: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    timeout_ms: u64,
}

impl AuthorizedProcess {
    /// Kanonisches Programm.
    pub fn program(&self) -> &Path {
        &self.program
    }
    /// Geprüfte Argumente.
    pub fn args(&self) -> &[String] {
        &self.args
    }
    /// Kanonisches Arbeitsverzeichnis.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
    /// Geprüfte Zusatzumgebung.
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }
    /// Zeitgrenze in Millisekunden.
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }
}

/// Ergebnis der Prüfung.
#[derive(Debug, Clone)]
pub enum GitVerdict {
    /// Erlaubt; nur dieser Nachweis darf ausgeführt werden.
    Allowed(AuthorizedProcess),
    /// Abgelehnt mit Begründung.
    Denied(String),
}

impl GitVerdict {
    /// Sicht als [`Decision`] für das Audit-Log.
    pub fn decision(&self) -> Decision {
        match self {
            GitVerdict::Allowed(process) => Decision::Allow(Capability {
                action: CapabilityAction::GitOp,
                canonical_path: Some(process.cwd.clone()),
            }),
            GitVerdict::Denied(reason) => Decision::Deny(reason.clone()),
        }
    }
}

fn deny(reason: impl Into<String>) -> GitVerdict {
    GitVerdict::Denied(reason.into())
}

/// Prüft einen Git-Aufruf gegen die feste Allowlist.
pub fn authorize_git(policy: &ProcessPolicy, spec: &ProcessSpec) -> GitVerdict {
    match check_git(policy, spec) {
        Ok(process) => GitVerdict::Allowed(process),
        Err(reason) => deny(reason),
    }
}

fn check_git(policy: &ProcessPolicy, spec: &ProcessSpec) -> Result<AuthorizedProcess, String> {
    let program =
        std::fs::canonicalize(&spec.program).map_err(|_| "Programm nicht auffindbar".to_owned())?;
    if program != policy.program {
        return Err("Nur das gebündelte Git-Paket darf gestartet werden".to_owned());
    }
    if !spec.clear_env {
        return Err("Die Umgebung des Prozesses muss geleert werden".to_owned());
    }
    if spec.timeout_ms == 0 || spec.timeout_ms > policy.max_timeout_ms {
        return Err("Zeitgrenze fehlt oder ist zu groß".to_owned());
    }

    let cwd = resolve_in_scopes(policy, &spec.cwd)
        .map_err(|e| format!("Arbeitsverzeichnis nicht erlaubt: {e}"))?;

    let mut declared: Vec<String> = Vec::new();
    for path in &spec.declared_paths {
        resolve_in_scopes(policy, path).map_err(|e| format!("Pfad nicht erlaubt: {e}"))?;
        declared.push(path.to_string_lossy().into_owned());
    }

    check_env(policy, spec, &declared)?;
    check_args(&spec.args, &declared)?;

    Ok(AuthorizedProcess {
        program,
        args: spec.args.clone(),
        cwd,
        env: spec.env.clone(),
        timeout_ms: spec.timeout_ms,
    })
}

fn resolve_in_scopes(policy: &ProcessPolicy, path: &Path) -> Result<PathBuf, PolicyError> {
    let mut last = None;
    for scope in &policy.scopes {
        match resolve_absolute_in_scope(scope, path) {
            Ok(resolved) => return Ok(resolved),
            Err(err) => last = Some(err),
        }
    }
    Err(last.unwrap_or_else(|| PolicyError::PathOutOfScope {
        path: path.to_path_buf(),
    }))
}

fn check_env(
    policy: &ProcessPolicy,
    spec: &ProcessSpec,
    declared: &[String],
) -> Result<(), String> {
    for (key, value) in &spec.env {
        if !ALLOWED_ENV_KEYS.contains(&key.as_str()) {
            return Err(format!("Umgebungsvariable `{key}` nicht erlaubt"));
        }
        if key == "GIT_INDEX_FILE" {
            if !declared.iter().any(|d| d == value) {
                return Err("GIT_INDEX_FILE muss ein deklarierter Pfad sein".to_owned());
            }
            resolve_in_scopes(policy, Path::new(value))
                .map_err(|e| format!("GIT_INDEX_FILE nicht erlaubt: {e}"))?;
        }
    }
    Ok(())
}

fn check_arg_value(value: &str, declared: &[String]) -> Result<(), String> {
    if value.contains('\0') || value.contains('\n') || value.contains('\r') {
        return Err("Steuerzeichen im Argument".to_owned());
    }
    if value.starts_with('~') {
        return Err("Tilde-Pfade sind nicht erlaubt".to_owned());
    }
    let path = Path::new(value);
    if path.is_absolute() {
        if !declared.iter().any(|d| d == value) {
            return Err(format!("Absoluter Pfad `{value}` wurde nicht deklariert"));
        }
        return Ok(());
    }
    normalize(path).map_err(|e| format!("Pfadargument abgelehnt: {e}"))
}

fn check_args(args: &[String], declared: &[String]) -> Result<(), String> {
    let mut index = 0;
    let mut config_seen: Vec<String> = Vec::new();
    let mut hooks_path_set = false;

    // Globale Optionen vor dem Unterbefehl: nur `-c key=value` und `-C pfad`.
    while index < args.len() {
        match args[index].as_str() {
            "-c" => {
                let pair = args.get(index + 1).ok_or("`-c` ohne Wert")?;
                let (key, value) = pair.split_once('=').ok_or("`-c` braucht key=value")?;
                if !ALLOWED_CONFIG_KEYS.contains(&key) {
                    return Err(format!("Git-Konfiguration `{key}` nicht erlaubt"));
                }
                check_arg_value(value, declared)?;
                if key == "core.hooksPath" {
                    if !Path::new(value).is_absolute() {
                        return Err("core.hooksPath muss ein deklarierter Pfad sein".to_owned());
                    }
                    hooks_path_set = true;
                }
                config_seen.push(pair.clone());
                index += 2;
            }
            "-C" => {
                let path = args.get(index + 1).ok_or("`-C` ohne Pfad")?;
                check_arg_value(path, declared)?;
                index += 2;
            }
            other if other.starts_with('-') => {
                return Err(format!("Globale Option `{other}` nicht erlaubt"));
            }
            _ => break,
        }
    }

    for (key, value) in REQUIRED_CONFIG {
        let wanted = format!("{key}={value}");
        if !config_seen.contains(&wanted) {
            return Err(format!("Pflichtoption `-c {wanted}` fehlt"));
        }
    }
    if !hooks_path_set {
        return Err("Pflichtoption `-c core.hooksPath=<leerer Ordner>` fehlt".to_owned());
    }

    let subcommand = args.get(index).ok_or("Kein Git-Unterbefehl")?;
    if !ALLOWED_SUBCOMMANDS.contains(&subcommand.as_str()) {
        return Err(format!("Git-Unterbefehl `{subcommand}` nicht erlaubt"));
    }
    index += 1;

    if subcommand == "worktree" {
        let sub = args.get(index).ok_or("`worktree` ohne Unterbefehl")?;
        if !ALLOWED_WORKTREE_SUB.contains(&sub.as_str()) {
            return Err(format!("`worktree {sub}` nicht erlaubt"));
        }
        index += 1;
    }

    let mut previous_is_message = false;
    for arg in &args[index..] {
        // Commit-Nachrichten dürfen Zeilenumbrüche und beliebigen Text enthalten;
        // sie sind reine Daten für Git, kein Pfad.
        if previous_is_message {
            previous_is_message = false;
            if arg.contains('\0') {
                return Err("NUL-Byte in Commit-Nachricht".to_owned());
            }
            continue;
        }
        if arg == "-m" {
            previous_is_message = true;
            continue;
        }
        if FORBIDDEN_OPTION_PREFIXES.iter().any(|p| arg.starts_with(p)) {
            return Err(format!("Option `{arg}` nicht erlaubt"));
        }
        // `--opt=wert`: der Wert wird wie ein Pfadargument geprüft.
        let value = match arg.split_once('=') {
            Some((opt, v)) if opt.starts_with("--") => v,
            _ => arg.as_str(),
        };
        check_arg_value(value, declared)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _temp: tempfile::TempDir,
        policy: ProcessPolicy,
        program: PathBuf,
        repo: PathBuf,
        hooks: PathBuf,
    }

    fn fixture() -> Fixture {
        let temp = tempfile::TempDir::with_prefix("pa-policy-proc").expect("temp");
        let repo = temp.path().join("repo");
        let hooks = temp.path().join("nohooks");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&hooks).unwrap();
        let program = temp.path().join("git.exe");
        std::fs::write(&program, b"").unwrap();
        let scope = PathScope::new(temp.path()).unwrap();
        let policy = ProcessPolicy::new(&program, vec![scope], 60_000).unwrap();
        Fixture {
            _temp: temp,
            policy,
            program,
            repo,
            hooks,
        }
    }

    fn base_args(f: &Fixture, rest: &[&str]) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "-c".into(),
            format!("core.hooksPath={}", f.hooks.display()),
            "-c".into(),
            "core.fsmonitor=false".into(),
            "-c".into(),
            "protocol.allow=never".into(),
        ];
        args.extend(rest.iter().map(|s| (*s).to_owned()));
        args
    }

    fn spec(f: &Fixture, rest: &[&str]) -> ProcessSpec {
        ProcessSpec {
            program: f.program.clone(),
            args: base_args(f, rest),
            cwd: f.repo.clone(),
            declared_paths: vec![f.hooks.clone()],
            env: vec![],
            clear_env: true,
            timeout_ms: 10_000,
        }
    }

    fn allowed(f: &Fixture, s: &ProcessSpec) -> bool {
        matches!(authorize_git(&f.policy, s), GitVerdict::Allowed(_))
    }

    #[test]
    fn status_is_allowed() {
        let f = fixture();
        assert!(allowed(&f, &spec(&f, &["status", "--porcelain"])));
    }

    #[test]
    fn network_and_state_changing_subcommands_are_denied() {
        let f = fixture();
        for cmd in [
            "push",
            "fetch",
            "pull",
            "clone",
            "remote",
            "stash",
            "checkout",
            "reset",
            "config",
            "submodule",
        ] {
            assert!(
                !allowed(&f, &spec(&f, &[cmd])),
                "{cmd} muss verweigert werden"
            );
        }
    }

    #[test]
    fn worktree_add_needs_declared_target() {
        let f = fixture();
        let target = f
            .repo
            .parent()
            .unwrap()
            .join("repo.iap-worktrees")
            .join("k1");
        let mut with_target = spec(
            &f,
            &[
                "worktree",
                "add",
                &target.to_string_lossy(),
                "-b",
                "iap/t/k1",
            ],
        );
        assert!(!allowed(&f, &with_target), "undeklarierter absoluter Pfad");
        with_target.declared_paths.push(target);
        assert!(allowed(&f, &with_target));
    }

    #[test]
    fn declared_path_outside_scope_is_denied() {
        let f = fixture();
        let outside = tempfile::TempDir::with_prefix("pa-policy-out").unwrap();
        let mut s = spec(&f, &["status"]);
        s.declared_paths.push(outside.path().join("x"));
        assert!(!allowed(&f, &s));
    }

    #[test]
    fn parent_dir_and_tilde_arguments_are_denied() {
        let f = fixture();
        assert!(!allowed(&f, &spec(&f, &["add", "--", "../secret.txt"])));
        assert!(!allowed(&f, &spec(&f, &["add", "--", "~/x"])));
    }

    #[test]
    fn dangerous_options_are_denied() {
        let f = fixture();
        assert!(!allowed(&f, &spec(&f, &["diff", "--ext-diff"])));
        assert!(!allowed(&f, &spec(&f, &["diff", "--output=/tmp/x"])));
        assert!(!allowed(&f, &spec(&f, &["log", "--upload-pack=calc"])));
    }

    #[test]
    fn required_hardening_options_must_be_present() {
        let f = fixture();
        let mut s = spec(&f, &["status"]);
        s.args.drain(0..2); // hooksPath entfernt
        assert!(!allowed(&f, &s));
        let mut s = spec(&f, &["status"]);
        let position = s
            .args
            .iter()
            .position(|a| a == "protocol.allow=never")
            .unwrap();
        s.args.drain(position - 1..=position);
        assert!(!allowed(&f, &s));
    }

    #[test]
    fn foreign_program_or_uncleared_env_or_bad_timeout_is_denied() {
        let f = fixture();
        let other = f.repo.join("evil.exe");
        std::fs::write(&other, b"").unwrap();
        let mut s = spec(&f, &["status"]);
        s.program = other;
        assert!(!allowed(&f, &s));
        let mut s = spec(&f, &["status"]);
        s.clear_env = false;
        assert!(!allowed(&f, &s));
        let mut s = spec(&f, &["status"]);
        s.timeout_ms = 10_000_000;
        assert!(!allowed(&f, &s));
    }

    #[test]
    fn env_allowlist_and_index_file_scope() {
        let f = fixture();
        let mut s = spec(&f, &["write-tree"]);
        s.env.push(("LD_PRELOAD".into(), "x".into()));
        assert!(!allowed(&f, &s));

        let index = f.repo.parent().unwrap().join("tmp.index");
        let mut s = spec(&f, &["write-tree"]);
        s.env.push((
            "GIT_INDEX_FILE".into(),
            index.to_string_lossy().into_owned(),
        ));
        assert!(!allowed(&f, &s), "undeklariert");
        s.declared_paths.push(index);
        assert!(allowed(&f, &s));
    }

    #[test]
    fn cwd_outside_scope_is_denied() {
        let f = fixture();
        let outside = tempfile::TempDir::with_prefix("pa-policy-cwd").unwrap();
        let mut s = spec(&f, &["status"]);
        s.cwd = outside.path().to_path_buf();
        assert!(!allowed(&f, &s));
    }
}
