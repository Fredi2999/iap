//! Globales Tastenkürzel (Strg+Alt+V), das aus jedem Programm heraus das Schreibfeld des Pets öffnet.
//!
//! Warum optional: Ein globales Kürzel gehört dem ganzen System. Ein anderes Programm kann es
//! schon belegen, und wer es nicht will, soll es auch nicht still bekommen. Es ist deshalb aus,
//! bis der Nutzer es in den Einstellungen des Pets einschaltet, und lässt sich jederzeit
//! abschalten. Es löst nur das Öffnen des Feldes aus; gesendet wird erst nach Eingabe und Enter.
//! Es gibt keinen Netzzugriff und keine Aufzeichnung von Tasten außer genau diesem Kürzel.

use pa_types::avatar::WindowMode;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::{error_code, pet, AppError, AppResult, AppState};

/// Anzeigename des Kürzels für Meldungen.
const HOTKEY_LABEL: &str = "Strg+Alt+V";

fn hotkey() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV)
}

/// Reaktion auf das Kürzel: Im Pet-Betrieb das Pet nach vorn holen und das Feld öffnen lassen,
/// sonst das Hauptfenster nach vorn holen.
fn pressed(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.flow.touch_activity();
    if state.flow.window_mode() == WindowMode::Pet {
        if let Some(window) = app.get_webview_window(pet::PET_LABEL) {
            let _ = window.show();
            let _ = window.set_focus();
        }
        let _ = app.emit_to(pet::PET_LABEL, "pet-open-panel", ());
    } else if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
}

/// Schaltet das Kürzel ein oder aus. Liefert, ob es danach registriert ist. Mehrfaches Einschalten
/// ist harmlos. Belegt ein anderes Programm das Kürzel, kommt eine verständliche Meldung.
#[tauri::command]
pub fn set_pet_hotkey(app: AppHandle, enabled: bool) -> AppResult<bool> {
    let shortcuts = app.global_shortcut();
    let shortcut = hotkey();
    let registered = shortcuts.is_registered(shortcut);
    if enabled && !registered {
        shortcuts
            .on_shortcut(shortcut, |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    pressed(app);
                }
            })
            .map_err(|_| AppError::Coded {
                code: error_code::HOTKEY_TAKEN,
                message: format!(
                    "Das Tastenkürzel {HOTKEY_LABEL} ist bereits von einem anderen Programm belegt."
                ),
            })?;
    } else if !enabled && registered {
        shortcuts.unregister(shortcut).map_err(|error| {
            AppError::Internal(format!("Tastenkürzel nicht freigegeben: {error}"))
        })?;
    }
    Ok(shortcuts.is_registered(shortcut))
}
