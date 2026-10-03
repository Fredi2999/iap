//! Beendet IAP und alle Hilfsprozesse auf dem Stick, bevor er entfernt wird.
//! Das Werkzeug gibt es nur unter Windows (taskkill, Laufwerks-Flush); auf
//! anderen Systemen baut der Workspace trotzdem, damit Tests und Clippy laufen.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod windows;

#[cfg(windows)]
fn main() {
    windows::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("pa-stop ist nur unter Windows verfügbar.");
}
