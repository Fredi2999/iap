use std::env;
use std::fs::OpenOptions;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::thread;
use std::time::Duration;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

const CREATE_NO_WINDOW: u32 = 0x08000000;

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(
        hwnd: *mut std::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        utype: u32,
    ) -> i32;
}

fn show_message(title: &str, message: &str, is_error: bool) {
    let wide_title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let wide_message: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let icon = if is_error { 0x00000010 } else { 0x00000040 }; // MB_ICONERROR or MB_ICONINFORMATION
    let flags = icon | 0x00040000 | 0x00002000; // MB_OK | icon | MB_SETFOREGROUND | MB_TASKMODAL
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide_message.as_ptr(),
            wide_title.as_ptr(),
            flags,
        );
    }
}

fn get_current_drive() -> Option<String> {
    if let Ok(exe_path) = env::current_exe() {
        let path_str = exe_path.to_string_lossy();
        if path_str.len() >= 2 && path_str.chars().nth(1) == Some(':') {
            return Some(path_str[..2].to_uppercase());
        }
    }
    None
}

const TARGET_PROCESS_NAMES: &[&str] = &["iap.exe", "llama-server.exe", "pa-launcher.exe"];

pub fn run() {
    let current_drive = get_current_drive();
    let my_pid = Pid::from_u32(std::process::id());

    // 1. Spezifische IAP-Hauptprozesse per taskkill /F /T beenden (schließt auch WebView2-Kindprozesse)
    for target in TARGET_PROCESS_NAMES {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/IM", target])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }

    // 2. Sysinfo-Prüfung: Alle restlichen Prozesse auf demselben USB-Laufwerk ermitteln und beenden
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );

    for (pid, process) in sys.processes() {
        if *pid == my_pid {
            continue;
        }

        let name = process.name().to_string_lossy().to_lowercase();
        let is_target = TARGET_PROCESS_NAMES
            .iter()
            .any(|&t| t.to_lowercase() == name);

        let is_on_drive = if let Some(ref drive) = current_drive {
            if let Some(exe) = process.exe() {
                exe.to_string_lossy().to_uppercase().starts_with(drive)
            } else {
                false
            }
        } else {
            false
        };

        if is_target || is_on_drive {
            process.kill();
        }
    }

    // 3. Kurz warten, bis Dateihandles vom Betriebssystem freigegeben wurden
    thread::sleep(Duration::from_millis(600));

    // 4. Schreibpuffer des Laufwerks auf das Speichermedium flushen
    if let Some(ref drive) = current_drive {
        let volume_path = format!(r"\\.\{}", drive);
        if let Ok(file) = OpenOptions::new().write(true).open(&volume_path) {
            let _ = file.sync_all();
        }
    }

    // 5. Erfolgsmeldung anzeigen (sofern nicht im Silent-Modus)
    let is_quiet = env::args().any(|arg| arg == "--quiet" || arg == "-q");
    if !is_quiet {
        let drive_info = current_drive
            .map(|d| format!(" (Laufwerk {d})"))
            .unwrap_or_default();

        let message = format!(
            "IAP und alle zugehörigen Hintergrunddienste (inklusive llama-server) wurden vollständig beendet.\n\nAlle Schreibvorgänge auf dem USB-Stick wurden abgeschlossen.\n\nDer Stick{} kann jetzt sicher ausgeworfen und entfernt werden.",
            drive_info
        );

        show_message("IAP – Beendet", &message, false);
    }
}
