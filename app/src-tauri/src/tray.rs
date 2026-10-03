//! Sichtbares Symbol im Infobereich der Taskleiste.
//!
//! Es bietet genau zwei Einträge: IAP öffnen und IAP vollständig beenden.
//! So bleibt IAP auch dann erreichbar und beendbar, wenn das Pet verdeckt
//! ist. Das Symbol existiert nur, solange der Prozess läuft; es gibt keinen
//! Autostart und keinen separaten Dienst.

use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

use crate::{ui_language, AppState};

const ID_OPEN: &str = "tray-open";
const TRAY_ID: &str = "iap";
const ID_QUIT: &str = "tray-quit";
const ICON: &[u8] = include_bytes!("../icons/iap-logo.png");

/// Beschriftungen je Oberflächensprache: (Öffnen, Beenden, Hinweis).
fn labels(language: &str) -> (&'static str, &'static str, &'static str) {
    match language {
        "en" => ("Open IAP", "Quit IAP completely", "IAP"),
        "es" => ("Abrir IAP", "Cerrar IAP por completo", "IAP"),
        "fr" => ("Ouvrir IAP", "Quitter IAP complètement", "IAP"),
        "ja" => ("IAP を開く", "IAP を完全に終了", "IAP"),
        _ => ("IAP öffnen", "IAP vollständig beenden", "IAP"),
    }
}

fn build_menu(app: &AppHandle, open: &str, quit: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let open_item = MenuItem::with_id(app, ID_OPEN, open, true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, ID_QUIT, quit, true, None::<&str>)?;
    Menu::with_items(app, &[&open_item, &quit_item])
}

/// Beschriftet Menü und Hinweis neu, wenn der Nutzer die Sprache wechselt. Ohne das
/// bliebe das Symbol bis zum nächsten Start in der alten Sprache.
pub fn relabel(app: &AppHandle, language: &str) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };
    let (open, quit, tooltip) = labels(language);
    tray.set_menu(Some(build_menu(app, open, quit)?))?;
    tray.set_tooltip(Some(tooltip))?;
    Ok(())
}

/// Legt das Symbol an. Fehler (etwa fehlende Taskleiste) sind kein Grund, nicht zu starten.
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let language = {
        let state = app.state::<AppState>();
        let root = crate::lock(&state.package_root).map(|guard| guard.clone());
        match root {
            Ok(root) => ui_language::read(&root),
            Err(_) => "de",
        }
    };
    let (open, quit, tooltip) = labels(language);
    let menu = build_menu(app, open, quit)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON)?)
        .tooltip(tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            ID_OPEN => crate::pet::show_main(app),
            ID_QUIT => crate::lifecycle::request_full_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Ein Linksklick öffnet das Hauptfenster.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::pet::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::labels;

    #[test]
    fn every_language_has_both_entries() {
        for language in ["de", "en", "es", "fr", "ja"] {
            let (open, quit, _) = labels(language);
            assert!(!open.is_empty() && !quit.is_empty());
        }
        // Jede Fremdsprache muss sich wirklich vom Deutschen unterscheiden.
        for language in ["en", "es", "fr", "ja"] {
            assert_ne!(labels(language).0, labels("de").0);
        }
    }
}
