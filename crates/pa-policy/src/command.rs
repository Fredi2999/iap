//! Prüfung von Befehlen, die der Nutzer im freigegebenen Projektordner ausführen lässt
//! (Tests, Build, Linter), etwa `cargo test` oder `npm run build`.
//!
//! **Was diese Prüfung leistet und was nicht.** Sie sorgt dafür, dass ein Befehl nur als
//! Programm plus Argumente ohne Shell läuft, dass das Arbeitsverzeichnis im Projektordner liegt,
//! dass Systemwerkzeuge und Shells gesperrt sind, dass die Umgebung nur eine feste Liste von
//! Variablen enthält (keine Schlüssel aus der Umgebung des Nutzers) und dass es eine Zeitgrenze
//! gibt. Sie **kann nicht verhindern**, dass das gestartete Programm selbst Dateien außerhalb des
//! Ordners ändert oder das Netz nutzt (ein Build lädt Pakete, ein Skript kann beliebiges tun):
//! Es gibt keine nachgewiesene Prozess-Sandbox. Deshalb bestätigt der Nutzer jeden einzelnen
//! Befehl, und der Dialog sagt das offen. Argumente, die wie Pfade aussehen, werden nach bestem
//! Wissen gegen den Projektordner geprüft; das ist eine Hilfe, keine Sperre.
//!
//! Ein [`AuthorizedCommand`] lässt sich nur über [`authorize_command`] erzeugen. Der Ausführer
//! nimmt nur diesen Nachweis entgegen.

use std::path::{Component, Path, PathBuf};

use crate::path::{resolve_absolute_in_scope, PathScope};

/// Programme, die nie gestartet werden: Shells und Interpreter für Fernzugriff, Netz- und
/// Systemverwaltungswerkzeuge. Verglichen wird der Dateiname ohne Endung, kleingeschrieben.
const DENIED_PROGRAMS: &[&str] = &[
    "cmd",
    "powershell",
    "pwsh",
    "wscript",
    "cscript",
    "mshta",
    "rundll32",
    "regsvr32",
    "msiexec",
    "reg",
    "regedit",
    "sc",
    "schtasks",
    "net",
    "net1",
    "netsh",
    "bash",
    "sh",
    "zsh",
    "fish",
    "wsl",
    "curl",
    "wget",
    "ssh",
    "scp",
    "sftp",
    "ftp",
    "telnet",
    "nc",
    "ncat",
    "certutil",
    "bitsadmin",
    "taskkill",
    "shutdown",
    "format",
    "diskpart",
    "runas",
    "takeown",
    "icacls",
    "vssadmin",
    "sudo",
    "su",
    "doas",
];

/// Umgebungsvariablen, die ein Befehl erbt. Alles andere (API-Schlüssel, Tokens, …) fällt weg.
/// Cargo, npm und die meisten Werkzeuge brauchen diese, um ihre Caches zu finden.
const PASSED_ENV: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SystemRoot",
    "SYSTEMROOT",
    "windir",
    "TEMP",
    "TMP",
    "TMPDIR",
    "USERPROFILE",
    "HOME",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramData",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "LANG",
    "LC_ALL",
];

const MAX_ARGS: usize = 64;
const MAX_ARG_CHARS: usize = 2_000;

/// Grenzen und Umgebung für Befehle in einem Projektordner.
#[derive(Debug, Clone)]
pub struct CommandPolicy {
    scope: PathScope,
    max_timeout_ms: u64,
    env: Vec<(String, String)>,
    /// Verzeichnisse, in denen ein Programmname gesucht wird.
    search_path: Vec<PathBuf>,
}

impl CommandPolicy {
    /// Policy mit ausdrücklich angegebener Umgebung und Suchpfad (für Tests und feste Setups).
    pub fn new(
        scope: PathScope,
        max_timeout_ms: u64,
        env: Vec<(String, String)>,
        search_path: Vec<PathBuf>,
    ) -> Self {
        Self {
            scope,
            max_timeout_ms,
            env,
            search_path,
        }
    }

    /// Policy aus der Umgebung dieses Prozesses: nur die Variablen aus [`PASSED_ENV`], und der
    /// Suchpfad aus `PATH`.
    pub fn from_environment(scope: PathScope, max_timeout_ms: u64) -> Self {
        let env: Vec<(String, String)> = PASSED_ENV
            .iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| ((*name).to_owned(), value))
            })
            .collect();
        let search_path = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect())
            .unwrap_or_default();
        Self::new(scope, max_timeout_ms, env, search_path)
    }
}

/// Ein gewünschter Befehl vor der Prüfung.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    /// Programmname (wird im Suchpfad gesucht) oder Pfad.
    pub program: String,
    pub args: Vec<String>,
    /// Arbeitsverzeichnis relativ zum Projektordner; leer oder `.` ist die Wurzel.
    pub cwd: String,
    pub timeout_ms: u64,
}

/// Nachweis einer bestandenen Prüfung.
#[derive(Debug, Clone)]
pub struct AuthorizedCommand {
    program: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    timeout_ms: u64,
}

impl AuthorizedCommand {
    pub fn program(&self) -> &Path {
        &self.program
    }
    pub fn args(&self) -> &[String] {
        &self.args
    }
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
    /// Die vollständige, geleerte Umgebung des Prozesses.
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// Der Befehl als eine Zeile für Dialog und Audit-Log.
    pub fn display(&self) -> String {
        // Ohne Endung: Der Nutzer liest `cargo test`, nicht `cargo.exe test`.
        let name = self.program.file_stem().map_or_else(
            || self.program.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let mut line = quote(&name);
        for arg in &self.args {
            line.push(' ');
            line.push_str(&quote(arg));
        }
        line
    }
}

fn quote(text: &str) -> String {
    if !text.is_empty() && !text.contains([' ', '\t', '"', '\'']) {
        text.to_owned()
    } else {
        format!("\"{}\"", text.replace('"', "\\\""))
    }
}

/// Ergebnis der Prüfung.
#[derive(Debug, Clone)]
pub enum CommandVerdict {
    Allowed(AuthorizedCommand),
    Denied(String),
}

/// Prüft einen Befehl. Siehe die Moduldokumentation für Umfang und Grenzen.
pub fn authorize_command(policy: &CommandPolicy, spec: &CommandSpec) -> CommandVerdict {
    match check(policy, spec) {
        Ok(command) => CommandVerdict::Allowed(command),
        Err(reason) => CommandVerdict::Denied(reason),
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Sucht ein Programm: ein Pfad wird kanonisiert, ein Name im Suchpfad gesucht (unter Windows mit
/// den üblichen Endungen).
fn find_program(policy: &CommandPolicy, program: &str) -> Option<PathBuf> {
    let has_separator = program.contains(['/', '\\']);
    if has_separator || Path::new(program).is_absolute() {
        let candidate = if Path::new(program).is_absolute() {
            PathBuf::from(program)
        } else {
            policy.scope.root().join(program)
        };
        return std::fs::canonicalize(candidate)
            .ok()
            .filter(|p| p.is_file());
    }
    let extensions: &[&str] = if cfg!(windows) {
        &["exe", "cmd", "bat", "com"]
    } else {
        &[""]
    };
    for dir in &policy.search_path {
        let direct = dir.join(program);
        if Path::new(program).extension().is_some() && direct.is_file() {
            return std::fs::canonicalize(direct).ok();
        }
        for ext in extensions {
            let candidate = if ext.is_empty() {
                dir.join(program)
            } else {
                dir.join(format!("{program}.{ext}"))
            };
            if candidate.is_file() {
                return std::fs::canonicalize(candidate).ok();
            }
        }
    }
    None
}

/// Sieht das Argument wie ein absoluter Pfad aus (auch hinter `--option=`)?
fn absolute_looking(arg: &str) -> Option<&str> {
    let value = arg.split_once('=').map_or(arg, |(_, v)| v);
    let value = value.trim_matches('"');
    let bytes = value.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    let rooted = value.starts_with('/') || value.starts_with("\\\\");
    // `/ ` allein oder Schalter wie `/C` (Windows) sind keine Pfade.
    let switch_like = value.len() <= 3 && !drive && value.starts_with('/');
    (drive || rooted).then_some(value).filter(|_| !switch_like)
}

fn has_parent_component(value: &str) -> bool {
    Path::new(value)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
}

fn check(policy: &CommandPolicy, spec: &CommandSpec) -> Result<AuthorizedCommand, String> {
    if spec.program.trim().is_empty() {
        return Err("Es fehlt das Programm.".to_owned());
    }
    if spec.timeout_ms == 0 || spec.timeout_ms > policy.max_timeout_ms {
        return Err(format!(
            "Die Zeitgrenze muss zwischen 1 und {} Sekunden liegen.",
            policy.max_timeout_ms / 1000
        ));
    }
    if spec.args.len() > MAX_ARGS {
        return Err(format!("Höchstens {MAX_ARGS} Argumente."));
    }
    for arg in &spec.args {
        if arg.chars().count() > MAX_ARG_CHARS || arg.contains('\0') {
            return Err("Ein Argument ist zu lang oder enthält unzulässige Zeichen.".to_owned());
        }
    }

    let program = find_program(policy, spec.program.trim()).ok_or_else(|| {
        format!(
            "Das Programm „{}“ gibt es auf diesem PC nicht.",
            spec.program.trim()
        )
    })?;
    if DENIED_PROGRAMS.contains(&stem(&program).as_str()) {
        return Err(format!(
            "„{}“ ist gesperrt: Shells, Netz- und Systemwerkzeuge startet IAP nicht.",
            stem(&program)
        ));
    }

    let cwd_relative = spec.cwd.trim();
    let cwd_relative = if cwd_relative.is_empty() {
        "."
    } else {
        cwd_relative
    };
    let cwd = resolve_absolute_in_scope(&policy.scope, &policy.scope.root().join(cwd_relative))
        .map_err(|e| format!("Arbeitsordner nicht erlaubt: {e}"))?;
    if !cwd.is_dir() {
        return Err("Der Arbeitsordner existiert nicht.".to_owned());
    }

    // Pfade in Argumenten nach bestem Wissen prüfen (siehe Moduldoku: eine Hilfe, keine Sperre).
    for arg in &spec.args {
        if let Some(absolute) = absolute_looking(arg) {
            resolve_absolute_in_scope(&policy.scope, Path::new(absolute))
                .map_err(|_| format!("Das Argument „{arg}“ zeigt aus dem Projektordner heraus."))?;
        } else {
            let value = arg.split_once('=').map_or(arg.as_str(), |(_, v)| v);
            if has_parent_component(value) {
                let joined = cwd.join(value);
                resolve_absolute_in_scope(&policy.scope, &joined).map_err(|_| {
                    format!("Das Argument „{arg}“ zeigt aus dem Projektordner heraus.")
                })?;
            }
        }
    }

    Ok(AuthorizedCommand {
        program,
        args: spec.args.clone(),
        cwd,
        env: policy.env.clone(),
        timeout_ms: spec.timeout_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fx {
        _temp: tempfile::TempDir,
        root: PathBuf,
        bin: PathBuf,
        policy: CommandPolicy,
    }

    fn exe_name(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_owned()
        }
    }

    fn fixture() -> Fx {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("projekt");
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        for tool in ["cargo", "npm", "cmd", "powershell", "bash", "curl"] {
            std::fs::write(bin.join(exe_name(tool)), "").unwrap();
        }
        let scope = PathScope::new(&root).unwrap();
        let policy = CommandPolicy::new(
            scope,
            600_000,
            vec![("PATH".to_owned(), "x".to_owned())],
            vec![bin.clone()],
        );
        Fx {
            root: std::fs::canonicalize(&root).unwrap(),
            bin,
            policy,
            _temp: temp,
        }
    }

    fn spec(program: &str, args: &[&str]) -> CommandSpec {
        CommandSpec {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            cwd: ".".to_owned(),
            timeout_ms: 60_000,
        }
    }

    fn allowed(fx: &Fx, spec: &CommandSpec) -> AuthorizedCommand {
        match authorize_command(&fx.policy, spec) {
            CommandVerdict::Allowed(command) => command,
            CommandVerdict::Denied(reason) => panic!("abgelehnt: {reason}"),
        }
    }

    fn denied(fx: &Fx, spec: &CommandSpec) -> String {
        match authorize_command(&fx.policy, spec) {
            CommandVerdict::Allowed(command) => panic!("erlaubt: {}", command.display()),
            CommandVerdict::Denied(reason) => reason,
        }
    }

    #[test]
    fn a_normal_build_command_is_allowed_and_resolved_in_the_search_path() {
        let fx = fixture();
        let command = allowed(&fx, &spec("cargo", &["test", "--workspace"]));
        assert_eq!(
            command.program().parent().unwrap(),
            std::fs::canonicalize(&fx.bin).unwrap()
        );
        assert_eq!(command.cwd(), fx.root);
        assert_eq!(command.display(), "cargo test --workspace");
        assert_eq!(command.timeout_ms(), 60_000);
    }

    #[test]
    fn shells_and_system_tools_are_never_started() {
        let fx = fixture();
        for tool in ["cmd", "powershell", "bash", "curl"] {
            let reason = denied(&fx, &spec(tool, &["x"]));
            assert!(reason.contains("gesperrt"), "{tool}: {reason}");
        }
        // Auch ein Pfad zu einer Shell oder eine umbenannte Endung ändert daran nichts.
        let path = fx.bin.join(exe_name("cmd"));
        assert!(denied(&fx, &spec(&path.to_string_lossy(), &[])).contains("gesperrt"));
    }

    #[test]
    fn unknown_programs_and_bad_limits_are_denied_with_a_reason() {
        let fx = fixture();
        assert!(denied(&fx, &spec("gibt-es-nicht", &[])).contains("gibt es auf diesem PC nicht"));
        assert!(denied(&fx, &spec("", &[])).contains("fehlt"));
        let mut slow = spec("cargo", &["build"]);
        slow.timeout_ms = 0;
        assert!(denied(&fx, &slow).contains("Zeitgrenze"));
        slow.timeout_ms = 601_000;
        assert!(denied(&fx, &slow).contains("Zeitgrenze"));
        let many: Vec<String> = (0..65).map(|i| i.to_string()).collect();
        let mut spec_many = spec("cargo", &[]);
        spec_many.args = many;
        assert!(denied(&fx, &spec_many).contains("Höchstens"));
    }

    #[test]
    fn the_working_folder_must_stay_inside_the_project() {
        let fx = fixture();
        let mut inside = spec("cargo", &["test"]);
        inside.cwd = "src".to_owned();
        assert_eq!(allowed(&fx, &inside).cwd(), fx.root.join("src"));
        for bad in ["..", "../draussen", "src/../..", "C:\\Windows", "/etc"] {
            let mut outside = spec("cargo", &["test"]);
            outside.cwd = bad.to_owned();
            assert!(denied(&fx, &outside).contains("Arbeitsordner"), "{bad}");
        }
        let mut missing = spec("cargo", &["test"]);
        missing.cwd = "nicht-da".to_owned();
        assert!(authorize_command(&fx.policy, &missing).clone_is_denied());
    }

    #[test]
    fn path_arguments_that_leave_the_project_are_refused_best_effort() {
        let fx = fixture();
        for bad in [
            "C:\\Windows\\system32\\drivers\\etc\\hosts",
            "--manifest-path=/etc/passwd",
            "../../geheim",
            "--out-dir=..\\..\\x",
        ] {
            let reason = denied(&fx, &spec("cargo", &["build", bad]));
            assert!(
                reason.contains("zeigt aus dem Projektordner heraus"),
                "{bad}: {reason}"
            );
        }
        // Normale Argumente, Schalter und Pfade im Projekt sind in Ordnung.
        let inside = fx.root.join("Cargo.toml");
        let inside_arg = format!("--manifest-path={}", inside.display());
        for fine in [
            "--release",
            "-p",
            "pa-core",
            "src/lib.rs",
            "/C",
            "--features=a,b",
            inside_arg.as_str(),
        ] {
            allowed(&fx, &spec("cargo", &["build", fine]));
        }
    }

    #[test]
    fn the_process_gets_only_the_listed_environment() {
        let fx = fixture();
        let command = allowed(&fx, &spec("cargo", &["test"]));
        assert_eq!(command.env(), [("PATH".to_owned(), "x".to_owned())]);
        // Die Umgebung dieses Testprozesses (z. B. mit Schlüsseln) gelangt nicht hinein.
        std::env::set_var("PA_POLICY_TEST_SECRET_TOKEN", "geheim");
        let from_env = CommandPolicy::from_environment(PathScope::new(&fx.root).unwrap(), 60_000);
        assert!(from_env
            .env
            .iter()
            .all(|(name, _)| PASSED_ENV.contains(&name.as_str())));
        assert!(from_env
            .env
            .iter()
            .all(|(name, _)| name != "PA_POLICY_TEST_SECRET_TOKEN"));
    }

    #[test]
    fn display_quotes_arguments_with_spaces() {
        let fx = fixture();
        let command = allowed(&fx, &spec("npm", &["run", "build one", "--x=\"y\""]));
        assert_eq!(command.display(), "npm run \"build one\" \"--x=\\\"y\\\"\"");
    }

    trait IsDenied {
        fn clone_is_denied(&self) -> bool;
    }
    impl IsDenied for CommandVerdict {
        fn clone_is_denied(&self) -> bool {
            matches!(self, CommandVerdict::Denied(_))
        }
    }
}
