//! Führt einen von `pa-policy` geprüften Befehl aus.
//!
//! Der Ausführer nimmt nur einen [`AuthorizedCommand`] entgegen, den es nur über
//! `authorize_command` gibt. Er startet ohne Fenster und ohne Eingabe, sammelt die Ausgabe
//! begrenzt (bei Überlänge bleibt das Ende, dort stehen die Fehler) und beendet bei Zeitgrenze
//! oder Abbruch den **ganzen Prozessbaum**: Unter Windows über ein Job-Objekt, damit auch
//! Kindprozesse (etwa die Compiler hinter `cargo`) nicht verwaist weiterlaufen.

use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use pa_policy::command::AuthorizedCommand;

/// Ergebnis eines Laufs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Rückgabewert; `None`, wenn der Prozess beendet werden musste.
    pub exit_code: Option<i32>,
    /// Ausgabe und Fehlerausgabe in Ankunftsreihenfolge.
    pub output: String,
    /// Der Anfang der Ausgabe wurde wegen der Größengrenze verworfen.
    pub truncated: bool,
    pub timed_out: bool,
    pub cancelled: bool,
    pub millis: u64,
}

/// Fasst die Prozesse eines Befehls zusammen, damit sie gemeinsam enden.
#[cfg(windows)]
struct Job(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Job {
    fn new() -> Option<Self> {
        use windows::core::PCWSTR;
        use windows::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: Das Handle gehört dieser Struktur und wird in `Drop` geschlossen; die
        // übergebene Struktur lebt über den ganzen Aufruf.
        unsafe {
            let handle = CreateJobObjectW(None, PCWSTR::null()).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let job = Self(handle);
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(info).cast(),
                u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).ok()?,
            )
            .ok()?;
            Some(job)
        }
    }

    fn assign(&self, child: &std::process::Child) -> bool {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::AssignProcessToJobObject;
        // SAFETY: Das Prozess-Handle gehört `child` und bleibt während des Aufrufs gültig.
        unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())).is_ok() }
    }

    fn terminate(&self) {
        use windows::Win32::System::JobObjects::TerminateJobObject;
        // SAFETY: Gültiges Job-Handle dieser Struktur.
        unsafe {
            let _ = TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        // Beim Schließen enden dank `KILL_ON_JOB_CLOSE` auch alle noch laufenden Prozesse.
        // SAFETY: Das Handle wird genau einmal geschlossen.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Hängt gelesene Bytes an und behält höchstens `max` (das Ende).
fn append_capped(buffer: &Mutex<(Vec<u8>, bool)>, chunk: &[u8], max: usize) {
    let Ok(mut guard) = buffer.lock() else {
        return;
    };
    guard.0.extend_from_slice(chunk);
    if guard.0.len() > max {
        let excess = guard.0.len() - max;
        guard.0.drain(..excess);
        guard.1 = true;
    }
}

fn pump(
    mut source: impl Read + Send + 'static,
    buffer: Arc<Mutex<(Vec<u8>, bool)>>,
    max: usize,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0_u8; 4096];
        while let Ok(count) = source.read(&mut chunk) {
            if count == 0 {
                break;
            }
            append_capped(&buffer, &chunk[..count], max);
        }
    })
}

/// Startet den Befehl und wartet auf sein Ende, die Zeitgrenze oder den Abbruch.
///
/// # Errors
/// Text, wenn der Prozess nicht gestartet werden kann.
pub fn run(
    command: &AuthorizedCommand,
    cancel: &AtomicBool,
    max_output_bytes: usize,
) -> Result<CommandOutput, String> {
    let mut process = Command::new(command.program());
    process
        .args(command.args())
        .current_dir(command.cwd())
        .env_clear()
        .envs(command.env().iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        process.creation_flags(CREATE_NO_WINDOW);
    }
    let started = Instant::now();
    let mut child = process
        .spawn()
        .map_err(|e| format!("Der Befehl lässt sich nicht starten: {e}"))?;

    #[cfg(windows)]
    let job = Job::new().filter(|job| job.assign(&child));

    let buffer = Arc::new(Mutex::new((Vec::new(), false)));
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(pump(out, Arc::clone(&buffer), max_output_bytes));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(pump(err, Arc::clone(&buffer), max_output_bytes));
    }

    let deadline = started + Duration::from_millis(command.timeout_ms());
    let mut timed_out = false;
    let mut cancelled = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => return Err(format!("Der Befehl lässt sich nicht überwachen: {error}")),
        }
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
        } else if Instant::now() >= deadline {
            timed_out = true;
        }
        if cancelled || timed_out {
            #[cfg(windows)]
            if let Some(job) = &job {
                job.terminate();
            }
            let _ = child.kill();
            break child.wait().ok();
        }
        thread::sleep(Duration::from_millis(40));
    };
    // Endet der Befehl regulär, sind seine Kindprozesse meist schon fertig; sonst beendet das
    // Schließen des Job-Objekts am Ende der Funktion sie, bevor die Leser hängen bleiben.
    #[cfg(windows)]
    if let Some(job) = &job {
        job.terminate();
    }
    for reader in readers {
        let _ = reader.join();
    }
    let (bytes, truncated) = buffer
        .lock()
        .map(|guard| (guard.0.clone(), guard.1))
        .unwrap_or_default();
    Ok(CommandOutput {
        exit_code: if timed_out || cancelled {
            None
        } else {
            status.and_then(|s| s.code())
        },
        output: String::from_utf8_lossy(&bytes).into_owned(),
        truncated,
        timed_out,
        cancelled,
        millis: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_policy::{
        command::{authorize_command, CommandPolicy, CommandSpec, CommandVerdict},
        PathScope,
    };

    fn authorized(
        program: &str,
        args: &[&str],
        timeout_ms: u64,
    ) -> (tempfile::TempDir, AuthorizedCommand) {
        let temp = tempfile::tempdir().expect("Temp");
        let policy =
            CommandPolicy::from_environment(PathScope::new(temp.path()).expect("Bereich"), 600_000);
        let spec = CommandSpec {
            program: program.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            cwd: ".".to_owned(),
            timeout_ms,
        };
        match authorize_command(&policy, &spec) {
            CommandVerdict::Allowed(command) => (temp, command),
            CommandVerdict::Denied(reason) => panic!("abgelehnt: {reason}"),
        }
    }

    /// Ein Befehl, der kurz Text ausgibt, und einer, der lange läuft (je Plattform).
    fn quick() -> (&'static str, Vec<&'static str>) {
        if cfg!(windows) {
            ("ping", vec!["-n", "1", "127.0.0.1"])
        } else {
            ("echo", vec!["hallo"])
        }
    }

    fn slow() -> (&'static str, Vec<&'static str>) {
        if cfg!(windows) {
            ("ping", vec!["-n", "30", "127.0.0.1"])
        } else {
            ("sleep", vec!["30"])
        }
    }

    #[test]
    fn a_short_command_returns_its_output_and_exit_code() {
        let (program, args) = quick();
        let (_temp, command) = authorized(program, &args, 20_000);
        let result = run(&command, &AtomicBool::new(false), 64 * 1024).expect("Lauf");
        assert_eq!(result.exit_code, Some(0), "{result:?}");
        assert!(!result.output.trim().is_empty(), "{result:?}");
        assert!(!result.timed_out && !result.cancelled && !result.truncated);
    }

    #[test]
    fn a_command_that_runs_too_long_is_ended_at_the_time_limit() {
        let (program, args) = slow();
        let (_temp, command) = authorized(program, &args, 600);
        let result = run(&command, &AtomicBool::new(false), 64 * 1024).expect("Lauf");
        assert!(result.timed_out, "{result:?}");
        assert_eq!(result.exit_code, None);
        assert!(result.millis < 10_000, "{result:?}");
    }

    #[test]
    fn cancelling_ends_a_running_command_quickly() {
        let (program, args) = slow();
        let (_temp, command) = authorized(program, &args, 60_000);
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::SeqCst);
        });
        let result = run(&command, &cancel, 64 * 1024).expect("Lauf");
        assert!(result.cancelled, "{result:?}");
        assert!(result.millis < 10_000, "{result:?}");
    }

    /// Anzahl laufender Prozesse mit diesem Namen, in deren Kommandozeile `marker` vorkommt.
    /// Die Kennung hält die anderen Tests (die ebenfalls `ping` starten) aus der Zählung heraus.
    #[cfg(windows)]
    fn running(name: &str, marker: &str) -> usize {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        system
            .processes()
            .values()
            .filter(|p| p.name().to_string_lossy().eq_ignore_ascii_case(name))
            .filter(|p| {
                p.cmd()
                    .iter()
                    .any(|part| part.to_string_lossy().contains(marker))
            })
            .count()
    }

    #[cfg(windows)]
    #[test]
    fn the_whole_process_tree_ends_with_the_command() {
        // Eine umbenannte Kopie der Shell startet `ping` als Kindprozess. Die Policy sperrt
        // `cmd` nach Namen; hier geht es nur um den Ausführer und das Job-Objekt.
        let temp = tempfile::tempdir().expect("Temp");
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let shell = std::env::var("SystemRoot").map_or_else(
            |_| std::path::PathBuf::from(r"C:WindowsSystem32cmd.exe"),
            |root| std::path::Path::new(&root).join("System32").join("cmd.exe"),
        );
        std::fs::copy(&shell, bin.join("huelle.exe")).expect("Kopie");
        let work = temp.path().join("w");
        std::fs::create_dir_all(&work).expect("Arbeitsordner");
        let policy = CommandPolicy::new(
            PathScope::new(&work).expect("Bereich"),
            60_000,
            vec![
                (
                    "SystemRoot".to_owned(),
                    std::env::var("SystemRoot").unwrap_or_default(),
                ),
                (
                    "PATH".to_owned(),
                    std::path::Path::new(&std::env::var("SystemRoot").unwrap_or_default())
                        .join("System32")
                        .to_string_lossy()
                        .into_owned(),
                ),
            ],
            vec![
                bin,
                std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_default())
                    .join("System32"),
            ],
        );
        let spec = CommandSpec {
            program: "huelle".to_owned(),
            args: vec!["/c".into(), "ping -n 37 127.0.0.1".into()],
            cwd: ".".to_owned(),
            timeout_ms: 1_000,
        };
        let CommandVerdict::Allowed(command) = authorize_command(&policy, &spec) else {
            panic!("abgelehnt");
        };
        let before = running("PING.EXE", " 37 ");
        let result = run(&command, &AtomicBool::new(false), 64 * 1024).expect("Lauf");
        assert!(result.timed_out, "{result:?}");
        // Ohne Job-Objekt hielte der Kindprozess die Ausgabe offen, und `run` wartete bis zu seinem
        // eigenen Ende (hier 37 Sekunden).
        assert!(
            result.millis < 8_000,
            "wartete auf den Kindprozess: {result:?}"
        );
        thread::sleep(Duration::from_millis(500));
        assert_eq!(
            running("PING.EXE", " 37 "),
            before,
            "der Kindprozess lief weiter"
        );
    }

    #[test]
    fn long_output_is_cut_to_the_tail_with_a_marker() {
        let buffer = Mutex::new((Vec::new(), false));
        append_capped(&buffer, b"0123456789", 6);
        append_capped(&buffer, b"abc", 6);
        let guard = buffer.lock().expect("Sperre");
        assert_eq!(guard.0, b"789abc");
        assert!(guard.1);
    }
}
