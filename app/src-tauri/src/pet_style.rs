//! Aussehen und Anzeige des Pets: Form, Farbe, Größe und welche Systemwerte erscheinen.
//!
//! Die Werte liegen verschlüsselt im Vault (`ui.pet_style`). Das Backend prüft sie, weil Haupt-
//! und Pet-Fenster dieselben Daten lesen und ein kaputter Wert das Pet unbenutzbar machen
//! (zu groß für den Bildschirm, unlesbare Farbe) oder das Fenster falsch dimensionieren könnte.
//! Die Systemwerte werden nur lokal gemessen und nie gespeichert oder versendet.

use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};
use tauri::{AppHandle, Emitter, State};

use crate::{require_session, ui_state, AppError, AppResult, AppState};

/// Vault-Schlüssel (im Namensraum der Oberfläche).
const STYLE_KEY: &str = "ui.pet_style";

/// Erlaubte Formen; sie entsprechen `vendor/bloub/skins.ts`.
pub const SHAPES: [&str; 8] = [
    "cercle", "galet", "squircle", "capsule", "triangle", "hexagone", "nuage", "goutte",
];

/// Kleinste und größte Figur in Pixeln. Oben begrenzt, damit das Fenster auf kleinen
/// Bildschirmen nie größer als die Arbeitsfläche wird.
pub const MIN_AVATAR: u32 = 80;
pub const MAX_AVATAR: u32 = 180;
const DEFAULT_AVATAR: u32 = 124;

/// Aktualisierung der Systemwerte in Sekunden. Nie schneller als 1 s: Die CPU-Last braucht
/// einen Messabstand, und häufigeres Abfragen kostet auf schwachen Rechnern mehr als es zeigt.
pub const MIN_REFRESH_SECS: u32 = 1;
pub const MAX_REFRESH_SECS: u32 = 10;

/// Anpassbares Aussehen des Pets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PetStyle {
    /// Eine der Formen aus [`SHAPES`].
    pub shape: String,
    /// `auto` (folgt dem Farbschema) oder `#rrggbb`.
    pub color: String,
    /// Größe der Figur in Pixeln.
    pub size: u32,
    pub show_cpu: bool,
    pub show_ram: bool,
    pub show_storage: bool,
    pub show_task: bool,
    pub refresh_secs: u32,
}

impl Default for PetStyle {
    fn default() -> Self {
        Self {
            shape: "galet".to_owned(),
            color: "auto".to_owned(),
            size: DEFAULT_AVATAR,
            show_cpu: false,
            show_ram: false,
            show_storage: false,
            show_task: false,
            refresh_secs: 3,
        }
    }
}

impl PetStyle {
    /// Prüft alle Felder; liefert eine verständliche Meldung statt stillschweigend zu korrigieren.
    pub fn validate(&self) -> Result<(), String> {
        if !SHAPES.contains(&self.shape.as_str()) {
            return Err("Diese Form gibt es nicht.".to_owned());
        }
        if self.color != "auto" && !is_hex_color(&self.color) {
            return Err("Die Farbe muss `auto` oder ein Hexwert wie #3b93f0 sein.".to_owned());
        }
        if !(MIN_AVATAR..=MAX_AVATAR).contains(&self.size) {
            return Err(format!(
                "Die Größe muss zwischen {MIN_AVATAR} und {MAX_AVATAR} liegen."
            ));
        }
        if !(MIN_REFRESH_SECS..=MAX_REFRESH_SECS).contains(&self.refresh_secs) {
            return Err(format!(
                "Die Aktualisierung muss zwischen {MIN_REFRESH_SECS} und {MAX_REFRESH_SECS} Sekunden liegen."
            ));
        }
        Ok(())
    }
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Liest den gespeicherten Stil; fehlt er oder ist er ungültig, gelten die Standardwerte.
/// Ein beschädigter Wert soll das Pet nie blockieren.
fn load(state: &AppState) -> AppResult<PetStyle> {
    let storage_key = ui_state::storage_key(STYLE_KEY).map_err(AppError::Invalid)?;
    let session = require_session(state)?;
    let vault = session.vault_runtime.lock()?;
    let raw = vault.repository().setting(&storage_key)?;
    Ok(raw
        .and_then(|text| serde_json::from_str::<PetStyle>(&text).ok())
        .filter(|style| style.validate().is_ok())
        .unwrap_or_default())
}

/// Liefert den Stil des Pets.
#[tauri::command]
pub fn get_pet_style(state: State<'_, AppState>) -> AppResult<PetStyle> {
    load(&state)
}

/// Speichert den Stil und informiert alle Fenster (Pet und Einstellungen) sofort.
#[tauri::command]
pub fn set_pet_style(
    app: AppHandle,
    state: State<'_, AppState>,
    style: PetStyle,
) -> AppResult<PetStyle> {
    style.validate().map_err(AppError::Invalid)?;
    let storage_key = ui_state::storage_key(STYLE_KEY).map_err(AppError::Invalid)?;
    let text =
        serde_json::to_string(&style).map_err(|error| AppError::Internal(error.to_string()))?;
    {
        let session = require_session(&state)?;
        let mut vault = session.vault_runtime.lock()?;
        vault.repository_mut().set_setting(&storage_key, &text)?;
        let mut no_fault = pa_vault::hot_copy::NoFault;
        let _ = vault.sync(&mut no_fault);
    }
    let _ = app.emit("pet-style-changed", style.clone());
    Ok(style)
}

/// Gemessene Systemwerte für die Anzeige am Pet.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemStats {
    pub cpu_name: String,
    /// Last in Prozent; direkt nach dem ersten Aufruf noch 0, weil die Messung zwei Zeitpunkte braucht.
    pub cpu_percent: f32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    /// Name des Datenträgers, auf dem das Paket liegt (meist der Stick); leer, wenn unbekannt.
    pub disk_name: String,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
}

/// Gehaltene Messung: `System` wird einmal angelegt und wiederverwendet, weil die CPU-Last
/// nur aus dem Unterschied zwischen zwei Messungen entsteht.
static SYSTEM: Mutex<Option<System>> = Mutex::new(None);

/// Wählt den Datenträger, dessen Einhängepunkt der längste Präfix des Pfads ist.
fn pick_disk(mounts: &[(String, &Path, u64, u64)], root: &Path) -> Option<usize> {
    mounts
        .iter()
        .enumerate()
        .filter(|(_, (_, mount, _, _))| root.starts_with(mount))
        .max_by_key(|(_, (_, mount, _, _))| mount.as_os_str().len())
        .map(|(index, _)| index)
}

/// Misst Prozessor, Arbeitsspeicher und den Datenträger des Pakets. Nur lokale Abfrage des
/// Betriebssystems, nie `System::new_all`, damit keine Prozessliste geladen wird.
#[tauri::command]
pub fn system_stats(state: State<'_, AppState>) -> AppResult<SystemStats> {
    let root = crate::lock(&state.package_root)?.clone();
    let mut guard = SYSTEM
        .lock()
        .map_err(|_| AppError::Internal("Systemmessung gesperrt".to_owned()))?;
    let system = guard.get_or_insert_with(System::new);
    system.refresh_cpu_usage();
    system.refresh_memory();

    let disks = Disks::new_with_refreshed_list();
    let mounts: Vec<(String, &Path, u64, u64)> = disks
        .list()
        .iter()
        .map(|disk| {
            (
                disk.name().to_string_lossy().into_owned(),
                disk.mount_point(),
                disk.total_space(),
                disk.available_space(),
            )
        })
        .collect();
    let (disk_name, disk_total, disk_free) = pick_disk(&mounts, &root)
        .map(|index| (mounts[index].0.clone(), mounts[index].2, mounts[index].3))
        .unwrap_or_default();

    Ok(SystemStats {
        cpu_name: system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().trim().to_owned())
            .unwrap_or_default(),
        cpu_percent: system.global_cpu_usage(),
        ram_used_bytes: system.used_memory(),
        ram_total_bytes: system.total_memory(),
        disk_name,
        disk_used_bytes: disk_total.saturating_sub(disk_free),
        disk_total_bytes: disk_total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_style_is_valid_and_shows_nothing() {
        let style = PetStyle::default();
        assert!(style.validate().is_ok());
        assert!(!style.show_cpu && !style.show_ram && !style.show_storage && !style.show_task);
    }

    #[test]
    fn invalid_values_are_rejected_with_a_message() {
        let bad = |change: fn(&mut PetStyle)| {
            let mut style = PetStyle::default();
            change(&mut style);
            style.validate().is_err()
        };
        assert!(bad(|s| s.shape = "wuerfel".into()));
        assert!(bad(|s| s.color = "rot".into()));
        assert!(bad(|s| s.color = "#12345".into()));
        assert!(bad(|s| s.color = "#12345g".into()));
        assert!(bad(|s| s.size = MIN_AVATAR - 1));
        assert!(bad(|s| s.size = MAX_AVATAR + 1));
        assert!(bad(|s| s.refresh_secs = 0));
        assert!(bad(|s| s.refresh_secs = MAX_REFRESH_SECS + 1));
    }

    #[test]
    fn valid_custom_values_pass() {
        let style = PetStyle {
            shape: "goutte".into(),
            color: "#3B93f0".into(),
            size: MAX_AVATAR,
            show_cpu: true,
            show_ram: true,
            show_storage: false,
            show_task: true,
            refresh_secs: MIN_REFRESH_SECS,
        };
        assert!(style.validate().is_ok());
    }

    #[test]
    fn a_partial_stored_value_falls_back_per_field() {
        let style: PetStyle =
            serde_json::from_str(r##"{"shape":"nuage","show_ram":true}"##).unwrap();
        assert_eq!(style.shape, "nuage");
        assert!(style.show_ram);
        assert_eq!(style.size, DEFAULT_AVATAR);
        assert!(style.validate().is_ok());
    }

    #[test]
    fn every_shape_in_the_list_exists_in_the_frontend() {
        let skins = include_str!("../../src/lib/vendor/bloub/skins.ts");
        for shape in SHAPES {
            assert!(
                skins.contains(&format!("'{shape}'")),
                "{shape} fehlt in skins.ts"
            );
        }
    }

    #[test]
    fn the_disk_with_the_longest_matching_mount_wins() {
        let root = Path::new("/media/usb/iap");
        let root_mount = Path::new("/");
        let usb_mount = Path::new("/media/usb");
        let other = Path::new("/media/other");
        let mounts = vec![
            ("sda".to_owned(), root_mount, 100, 50),
            ("usb".to_owned(), usb_mount, 32, 8),
            ("other".to_owned(), other, 64, 1),
        ];
        assert_eq!(pick_disk(&mounts, root), Some(1));
        assert_eq!(pick_disk(&mounts, Path::new("/home/x")), Some(0));
        assert_eq!(pick_disk(&[], root), None);
    }

    #[test]
    fn stats_are_plausible_on_this_machine() {
        let mut system = System::new();
        system.refresh_cpu_usage();
        system.refresh_memory();
        assert!(system.total_memory() > 0);
        assert!(system.used_memory() <= system.total_memory());
    }
}
