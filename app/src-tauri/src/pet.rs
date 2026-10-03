//! Kleines Desktop-Pet: ein transparentes Fenster desselben Prozesses.
//!
//! Das Schließen des Hauptfensters blendet dieses Fenster unten rechts oberhalb
//! der Taskleiste ein (Arbeitsfläche des Monitors, nicht Bildschirmrand). Es ist
//! kein Dienst und kein Autostart: „IAP vollständig beenden“ entfernt es mit
//! dem gesamten Prozess. Die Platzierung ist als reine Rechnung getrennt, damit
//! sie ohne Fenster getestet werden kann (mehrere Monitore, Skalierung).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use pa_types::avatar::WindowMode;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::{default_root, AppResult, AppState};

/// Fensterkennung; die Oberfläche wählt danach ihren Einstieg.
pub const PET_LABEL: &str = "pet";

/// Größe des zusammengeklappten Pets bei der Standardfigur (logische Pixel): nur der Avatar.
const COLLAPSED: (f64, f64) = (176.0, 196.0);
/// Größe mit Menü, Diktat und Antwort bei der Standardfigur (logische Pixel).
const EXPANDED: (f64, f64) = (360.0, 520.0);
/// Breite und zusätzliche Höhe der Sprechblase über der Figur (logische Pixel). Eine eigene,
/// kleine Stufe, damit eine kurze Antwort nicht das große Fenster über den Desktop legt.
const BUBBLE: (f64, f64) = (320.0, 170.0);
/// Standardgröße der Figur; die Fenstermaße oben gelten für diesen Wert.
const BASE_AVATAR: f64 = 124.0;
/// Höhe einer Anzeigezeile (RAM, CPU, ...) unter der Figur in logischen Pixeln.
const ROW_HEIGHT: f64 = 18.0;

/// Aktuelle Figurgröße und Zeilenzahl; der Wächter braucht sie, um die Größe ohne Rückfrage
/// an die Oberfläche wiederherzustellen.
static AVATAR_SIZE: AtomicU32 = AtomicU32::new(124);
static ROWS: AtomicU32 = AtomicU32::new(0);

/// Fenstergrößen für eine Figurgröße und die Zahl der Anzeigezeilen.
///
/// Rein rechnerisch und ohne Fensterzugriff, damit sich die Grenzen testen lassen. Die Werte
/// werden begrenzt, damit eine fehlerhafte Eingabe das Fenster nie über die Arbeitsfläche hinaus
/// wachsen lässt.
fn sizes(avatar: u32, rows: u32) -> ((f64, f64), (f64, f64)) {
    let avatar = f64::from(avatar.clamp(80, 180));
    let rows = f64::from(rows.min(4));
    let grow = avatar - BASE_AVATAR;
    let extra = rows * ROW_HEIGHT;
    let collapsed = (COLLAPSED.0 + grow.max(0.0), COLLAPSED.1 + grow + extra);
    let expanded = (EXPANDED.0, EXPANDED.1 + grow.max(0.0) + extra);
    (collapsed, expanded)
}

/// Fenstergröße mit Sprechblase: so breit wie die Blase, so hoch wie die Figur plus Blase.
fn bubble_size(collapsed: (f64, f64)) -> (f64, f64) {
    (collapsed.0.max(BUBBLE.0), collapsed.1 + BUBBLE.1)
}

fn current_sizes() -> ((f64, f64), (f64, f64)) {
    sizes(
        AVATAR_SIZE.load(Ordering::Relaxed),
        ROWS.load(Ordering::Relaxed),
    )
}
/// Abstand zum Rand der Arbeitsfläche (logische Pixel).
const MARGIN: f64 = 12.0;

/// Ein Rechteck in physischen Pixeln.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Area {
    fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }
    fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }
}

/// Position (links oben) für ein Fenster der Größe `window` unten rechts in `work`.
pub fn bottom_right(work: Area, window: (u32, u32), margin: u32) -> (i32, i32) {
    let x = work.right() - i64::from(window.0) - i64::from(margin);
    let y = work.bottom() - i64::from(window.1) - i64::from(margin);
    // Passt das Fenster nicht in die Fläche, bleibt es wenigstens links oben sichtbar.
    (
        i32::try_from(x.max(i64::from(work.x))).unwrap_or(work.x),
        i32::try_from(y.max(i64::from(work.y))).unwrap_or(work.y),
    )
}

/// Ob das Fenster (mindestens zur Hälfte in jeder Richtung) in einer der Flächen liegt.
pub fn is_visible_in(areas: &[Area], position: (i32, i32), size: (u32, u32)) -> bool {
    let center_x = i64::from(position.0) + i64::from(size.0) / 2;
    let center_y = i64::from(position.1) + i64::from(size.1) / 2;
    areas.iter().any(|area| {
        center_x >= i64::from(area.x)
            && center_x < area.right()
            && center_y >= i64::from(area.y)
            && center_y < area.bottom()
    })
}

fn work_areas(app: &AppHandle) -> Vec<Area> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|monitor| {
            let work = monitor.work_area();
            Area {
                x: work.position.x,
                y: work.position.y,
                width: work.size.width,
                height: work.size.height,
            }
        })
        .collect()
}

/// Arbeitsfläche des Monitors, auf dem das Hauptfenster liegt, sonst des Hauptmonitors.
fn preferred_area(app: &AppHandle) -> Option<(Area, f64)> {
    let monitor = app
        .get_webview_window("main")
        .and_then(|window| window.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let work = monitor.work_area();
    Some((
        Area {
            x: work.position.x,
            y: work.position.y,
            width: work.size.width,
            height: work.size.height,
        },
        monitor.scale_factor(),
    ))
}

fn physical(size: (f64, f64), scale: f64) -> (u32, u32) {
    (
        (size.0 * scale).round().max(1.0) as u32,
        (size.1 * scale).round().max(1.0) as u32,
    )
}

/// Legt das Pet-Fenster einmalig an (unsichtbar) und liefert es.
fn ensure_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(PET_LABEL) {
        return Ok(window);
    }
    let webview_dir = default_root().join("AI").join("data").join("webview");
    WebviewWindowBuilder::new(app, PET_LABEL, WebviewUrl::default())
        .title("IAP")
        .inner_size(current_sizes().0 .0, current_sizes().0 .1)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .data_directory(webview_dir)
        .build()
}

/// Setzt das Fenster unten rechts in die Arbeitsfläche (mit der aktuellen Größe).
fn place(app: &AppHandle, window: &WebviewWindow, logical: (f64, f64)) {
    let Some((area, scale)) = preferred_area(app) else {
        return;
    };
    let size = physical(logical, scale);
    let (x, y) = bottom_right(area, size, (MARGIN * scale).round() as u32);
    let _ = window.set_size(PhysicalSize::new(size.0, size.1));
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Zeigt das Pet unten rechts. Legt das Fenster beim ersten Mal an.
pub fn show(app: &AppHandle) -> tauri::Result<()> {
    let window = ensure_window(app)?;
    place(app, &window, current_sizes().0);
    window.show()?;
    start_watchdog(app);
    Ok(())
}

/// Blendet das Pet aus.
pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(PET_LABEL) {
        let _ = window.hide();
    }
}

/// Bewahrt das Pet vor „verlorenen“ Positionen, wenn ein Monitor abgesteckt oder
/// die Skalierung geändert wird: liegt es außerhalb jeder Arbeitsfläche, kommt es
/// zurück nach unten rechts. Eine vom Nutzer verschobene Position bleibt erhalten.
fn start_watchdog(app: &AppHandle) {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3));
        let Some(window) = app.get_webview_window(PET_LABEL) else {
            continue;
        };
        if !window.is_visible().unwrap_or(false) {
            continue;
        }
        let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
            continue;
        };
        if !is_visible_in(
            &work_areas(&app),
            (position.x, position.y),
            (size.width, size.height),
        ) {
            let (collapsed, expanded_size) = current_sizes();
            let expanded = f64::from(size.height) > collapsed.1 * 2.0;
            place(
                &app,
                &window,
                if expanded { expanded_size } else { collapsed },
            );
        }
    });
}

/// Wo das zugeklappte Pet saß, als es aufgeklappt wurde, und wohin das aufgeklappte Fenster kam.
///
/// Warum: Sitzt das Pet nah am oberen Rand, wächst das Fenster nach oben nicht weit genug und wird
/// in die Arbeitsfläche geschoben. Beim Zuklappen aus der Ecke des (verschobenen) großen Fensters
/// zu rechnen, ließ das Pet bei jedem Auf- und Zuklappen ein Stück nach unten wandern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Expanded {
    anchor_right: i64,
    anchor_bottom: i64,
    position: (i32, i32),
}

/// Zugleich Sperre: Mehrere schnell aufeinanderfolgende Aufrufe (Doppelklick) dürfen nicht
/// gleichzeitig lesen und setzen, sonst rechnen sie mit der Größe vom vorigen Schritt.
static EXPANDED_STATE: Mutex<Option<Expanded>> = Mutex::new(None);

/// Wie weit die vom Fenstersystem gemeldete Position von der gesetzten abweichen darf und noch
/// als „vom Nutzer nicht verschoben“ gilt (Rundung bei Skalierung, Rahmen).
const MOVE_TOLERANCE: i32 = 3;

fn area_containing(areas: &[Area], x: i64, y: i64) -> Option<Area> {
    areas.iter().copied().find(|area| {
        x >= i64::from(area.x) && x < area.right() && y >= i64::from(area.y) && y < area.bottom()
    })
}

/// Linke obere Ecke für ein Fenster der Größe `target`, dessen untere rechte Ecke bei
/// (`right`, `bottom`) liegen soll, innerhalb von `area`.
///
/// Rein rechnerisch, damit sich das Randverhalten ohne Fenster testen lässt. Ist die Fläche zu
/// klein, bleibt das Fenster links oben sichtbar.
pub fn anchored_position(
    area: Option<Area>,
    right: i64,
    bottom: i64,
    target: (u32, u32),
) -> (i32, i32) {
    let mut x = right - i64::from(target.0);
    let mut y = bottom - i64::from(target.1);
    if let Some(area) = area {
        x = x
            .min(area.right() - i64::from(target.0))
            .max(i64::from(area.x));
        y = y
            .min(area.bottom() - i64::from(target.1))
            .max(i64::from(area.y));
    }
    (
        i32::try_from(x).unwrap_or(i32::MAX),
        i32::try_from(y).unwrap_or(i32::MAX),
    )
}

/// Klappt das Pet für Menü, Diktat und Antwort auf oder wieder zu; die untere
/// rechte Ecke des zugeklappten Pets bleibt an derselben Stelle. `bubble` wählt die kleine
/// Stufe für die Sprechblase, solange nichts Größeres offen ist.
#[tauri::command]
pub fn pet_expand(
    app: AppHandle,
    expanded: bool,
    avatar_size: Option<u32>,
    rows: Option<u32>,
    bubble: Option<bool>,
) -> AppResult<()> {
    let bubble = bubble.unwrap_or(false) && !expanded;
    let Ok(mut saved) = EXPANDED_STATE.lock() else {
        return Ok(());
    };
    if let Some(size) = avatar_size {
        AVATAR_SIZE.store(size.clamp(80, 180), Ordering::Relaxed);
    }
    if let Some(rows) = rows {
        ROWS.store(rows.min(4), Ordering::Relaxed);
    }
    let Some(window) = app.get_webview_window(PET_LABEL) else {
        return Ok(());
    };
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return Ok(());
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let (collapsed, expanded_size) = current_sizes();
    let wanted = if expanded {
        expanded_size
    } else if bubble {
        bubble_size(collapsed)
    } else {
        collapsed
    };
    let target = physical(wanted, scale);
    // Die Ecke des zugeklappten Pets: gemerkt, solange das Fenster nicht verschoben wurde,
    // sonst die aktuelle Ecke (der Nutzer hat es selbst irgendwohin gezogen).
    let (anchor_right, anchor_bottom) = match *saved {
        Some(state)
            if (state.position.0 - position.x).abs() <= MOVE_TOLERANCE
                && (state.position.1 - position.y).abs() <= MOVE_TOLERANCE =>
        {
            (state.anchor_right, state.anchor_bottom)
        }
        _ => (
            i64::from(position.x) + i64::from(size.width),
            i64::from(position.y) + i64::from(size.height),
        ),
    };
    let area = area_containing(&work_areas(&app), anchor_right - 1, anchor_bottom - 1);
    let (x, y) = anchored_position(area, anchor_right, anchor_bottom, target);
    let _ = window.set_size(PhysicalSize::new(target.0, target.1));
    let _ = window.set_position(PhysicalPosition::new(x, y));
    *saved = (expanded || bubble).then_some(Expanded {
        anchor_right,
        anchor_bottom,
        position: (x, y),
    });
    Ok(())
}

/// Wechselt zum Pet: Flow-Arbeit und Aufnahme stoppen, Hauptfenster ausblenden.
///
/// Ergebnisse und Kandidaten bleiben erhalten; es wird nichts automatisch
/// fortgesetzt. Neue Aufträge braucht eine bewusste Nutzeraktion im Pet.
pub fn enter_pet_mode(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    crate::lifecycle::stop_flow_work(&state);
    state.voice.stop_everything(app);
    state.flow.set_window_mode(WindowMode::Pet);
    state.flow.touch_activity();
    show(app)?;
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.hide();
    }
    let _ = app.emit("window-mode", WindowMode::Pet);
    Ok(())
}

/// Geometrie von Pet und Monitoren als Text (nur für die Debug-Prüfung).
#[cfg(debug_assertions)]
pub fn debug_report(app: &AppHandle) -> String {
    let mut out = String::new();
    for (index, monitor) in app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let work = monitor.work_area();
        out.push_str(&format!(
            "Monitor {index}: Position {:?}, Größe {:?}, Arbeitsfläche {:?}/{:?}, Skalierung {}\n",
            monitor.position(),
            monitor.size(),
            work.position,
            work.size,
            monitor.scale_factor()
        ));
    }
    match app.get_webview_window(PET_LABEL) {
        Some(window) => out.push_str(&format!(
            "Pet: sichtbar {:?}, Position {:?}, Größe {:?}\n",
            window.is_visible(),
            window.outer_position(),
            window.outer_size()
        )),
        None => out.push_str("Pet: nicht angelegt\n"),
    }
    out
}

/// Gleicht das Pet mit dem Hauptfenster ab, solange dieses nicht geschlossen (Pet-Modus) ist.
///
/// Warum kein `enter_pet_mode`: Minimieren ist kein Beenden. Workflows, Agent Flow und
/// Aufnahmen laufen weiter; das Pet macht nur sichtbar, dass IAP da ist, und
/// nimmt Anfragen an. Ohne offenen Tresor gibt es nichts anzuzeigen.
pub fn sync_with_main_window(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.flow.window_mode() == WindowMode::Pet {
        return;
    }
    let has_session = state.session.lock().is_ok_and(|guard| guard.is_some());
    let Some(main) = app.get_webview_window("main") else {
        return;
    };
    let minimized = main.is_minimized().unwrap_or(false);
    let pet_visible = app
        .get_webview_window(PET_LABEL)
        .is_some_and(|window| window.is_visible().unwrap_or(false));
    if minimized && has_session && !state.quitting.load(std::sync::atomic::Ordering::SeqCst) {
        if !pet_visible {
            state.flow.touch_activity();
            if let Err(error) = show(app) {
                eprintln!("Pet beim Minimieren nicht möglich: {error}");
            }
        }
    } else if !minimized && pet_visible {
        hide(app);
    }
}

/// Zeigt das Hauptfenster wieder und blendet das Pet aus.
pub fn show_main(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.flow.set_window_mode(WindowMode::Main);
    state.flow.touch_activity();
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
    hide(app);
    let _ = app.emit("window-mode", WindowMode::Main);
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Area = Area {
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
    };

    #[test]
    fn default_sizes_match_the_original_window() {
        assert_eq!(sizes(124, 0), (COLLAPSED, EXPANDED));
    }

    #[test]
    fn rows_and_figure_size_grow_the_window_within_limits() {
        let (small, _) = sizes(80, 0);
        let (big, big_open) = sizes(180, 4);
        assert!(small.1 < COLLAPSED.1);
        assert!(big.0 > COLLAPSED.0 && big.1 > COLLAPSED.1 + 4.0 * ROW_HEIGHT);
        assert!(
            big_open.1 < 700.0,
            "auch ganz aufgeklappt bleibt das Fenster klein genug"
        );
        // Unsinnige Eingaben werden begrenzt.
        assert_eq!(sizes(10_000, 99), sizes(180, 4));
        assert_eq!(sizes(0, 0), sizes(80, 0));
    }

    #[test]
    fn bubble_stage_sits_between_collapsed_and_expanded() {
        let (collapsed, expanded) = sizes(124, 0);
        let bubble = bubble_size(collapsed);
        assert!(bubble.1 > collapsed.1 && bubble.1 < expanded.1);
        assert!(bubble.0 >= collapsed.0 && bubble.0 <= expanded.0);
        // Auch mit der größten Figur und vier Zeilen bleibt die Blase kleiner als das große Fenster.
        let (big, big_open) = sizes(180, 4);
        assert!(bubble_size(big).1 < big_open.1);
    }

    #[test]
    fn pet_sits_bottom_right_above_the_taskbar() {
        // Die Arbeitsfläche ist 40 Pixel niedriger als der Bildschirm: darunter liegt die Taskleiste.
        assert_eq!(
            bottom_right(WORK, (176, 196), 12),
            (1920 - 176 - 12, 1040 - 196 - 12)
        );
    }

    #[test]
    fn scaling_is_applied_before_placement() {
        let work = Area {
            x: 0,
            y: 0,
            width: 2880,
            height: 1560,
        };
        let size = physical((176.0, 196.0), 1.5);
        assert_eq!(size, (264, 294));
        assert_eq!(
            bottom_right(work, size, 18),
            (2880 - 264 - 18, 1560 - 294 - 18)
        );
    }

    #[test]
    fn second_monitor_to_the_left_uses_its_own_origin() {
        let left = Area {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let (x, y) = bottom_right(left, (176, 196), 12);
        assert!(x >= left.x && x + 176 <= left.x + 1920);
        assert!(y + 196 <= 1040);
    }

    #[test]
    fn too_small_work_area_keeps_the_window_at_the_top_left() {
        let tiny = Area {
            x: 100,
            y: 50,
            width: 100,
            height: 100,
        };
        assert_eq!(bottom_right(tiny, (176, 196), 12), (100, 50));
    }

    #[test]
    fn expanding_near_the_top_is_pushed_into_the_work_area() {
        // Pet sitzt weit oben: unten rechts bei (1900, 300), aufgeklappt 360 x 520.
        let (x, y) = anchored_position(Some(WORK), 1900, 300, (360, 520));
        assert_eq!((x, y), (1540, 0));
    }

    #[test]
    fn expanding_keeps_the_corner_when_there_is_room() {
        let (x, y) = anchored_position(Some(WORK), 1900, 900, (360, 520));
        assert_eq!((i64::from(x) + 360, i64::from(y) + 520), (1900, 900));
    }

    #[test]
    fn collapsing_returns_to_the_remembered_corner_not_the_pushed_window() {
        // Auf- und Zuklappen darf das Pet nicht verschieben, auch wenn das große Fenster gedrückt wurde.
        let (right, bottom) = (1900_i64, 300_i64);
        let big = anchored_position(Some(WORK), right, bottom, (360, 520));
        assert_eq!(big.1, 0, "das große Fenster wurde nach unten geschoben");
        let (x, y) = anchored_position(Some(WORK), right, bottom, (176, 196));
        assert_eq!((i64::from(x) + 176, i64::from(y) + 196), (right, bottom));
    }

    #[test]
    fn expanding_never_leaves_the_work_area_at_the_bottom_or_right() {
        let (x, y) = anchored_position(Some(WORK), 2500, 1300, (360, 520));
        assert!(i64::from(x) + 360 <= 1920 && i64::from(y) + 520 <= 1040);
    }

    #[test]
    fn too_small_area_keeps_the_expanded_window_at_the_top_left() {
        let tiny = Area {
            x: 100,
            y: 50,
            width: 100,
            height: 100,
        };
        assert_eq!(
            anchored_position(Some(tiny), 200, 150, (360, 520)),
            (100, 50)
        );
    }

    #[test]
    fn window_on_a_removed_monitor_is_detected() {
        let areas = [WORK];
        assert!(is_visible_in(&areas, (1700, 800), (176, 196)));
        assert!(!is_visible_in(&areas, (2200, 300), (176, 196)));
        assert!(!is_visible_in(&areas, (-400, 0), (176, 196)));
        assert!(!is_visible_in(&[], (0, 0), (10, 10)));
    }
}
