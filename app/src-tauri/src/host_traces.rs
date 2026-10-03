//! Spuren von IAP auf dem Host-PC finden und messen.
//!
//! IAP legt auf einem Rechner nur an klar benannten Orten Daten ab: die
//! Modellkopie für schnellere Starts (`%LOCALAPPDATA%\IAP\model-cache`) und
//! die verschlüsselte Arbeitskopie des Tresors (`%TEMP%\PortableAI`). Ältere
//! Versionen haben zusätzlich Browserdaten der Oberfläche im Nutzerprofil
//! abgelegt. Dieses Modul sammelt diese Orte; gelöscht wird in `lib.rs`, weil
//! jede Löschung über pa-policy geprüft und im Vault protokolliert werden muss.

use std::{
    fs,
    path::{Path, PathBuf},
};

use pa_types::ipc::HostTrace;

/// Ort einer Spur, aufgeteilt in den Prüfbereich für pa-policy und den
/// relativen Pfad darin. So kann die Policy den Pfad normalisieren und
/// sicherstellen, dass eine Löschung den Bereich nicht verlässt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceLocation {
    pub id: &'static str,
    pub scope: PathBuf,
    pub relative: PathBuf,
}

impl TraceLocation {
    /// Vollständiger Pfad, nur für Anzeige und Messung; gelöscht wird der von
    /// pa-policy kanonisierte Pfad.
    pub fn full_path(&self) -> PathBuf {
        self.scope.join(&self.relative)
    }
}

/// Schlüssel der Vault-Einstellung „auf diesem PC nicht mehr fragen“.
///
/// Je Host getrennt, weil dieselbe Person den Stick auf eigenen und fremden
/// Rechnern nutzt. Das Präfix der Host-Kennung reicht zur Unterscheidung und
/// hält den Schlüssel kurz.
pub fn ask_setting_key(host_identifier: &str) -> String {
    let prefix: String = host_identifier.chars().take(16).collect();
    format!("host.{prefix}.ask_on_exit")
}

/// Alle Orte, an denen IAP Spuren hinterlassen kann.
///
/// Warum Parameter statt Umgebungsvariablen: So sind die Orte in Tests mit
/// temporären Verzeichnissen prüfbar.
pub fn known_locations(
    local_app_data: Option<&Path>,
    temp_dir: &Path,
    webview_identifier: &str,
) -> Vec<TraceLocation> {
    let mut locations = Vec::new();
    if let Some(local) = local_app_data {
        locations.push(TraceLocation {
            id: "model_cache",
            scope: local.to_path_buf(),
            relative: Path::new("IAP").join("model-cache"),
        });
        locations.push(TraceLocation {
            id: "legacy_webview",
            scope: local.to_path_buf(),
            relative: PathBuf::from(webview_identifier),
        });
    }
    locations.push(TraceLocation {
        id: "vault_hot",
        scope: temp_dir.to_path_buf(),
        relative: PathBuf::from("PortableAI"),
    });
    locations
}

/// Misst die vorhandenen Orte. Fehlende Orte und symbolische Links werden
/// übersprungen, damit nie etwas außerhalb der eigenen Ordner gezählt wird.
pub fn collect(locations: &[TraceLocation]) -> Vec<HostTrace> {
    locations
        .iter()
        .filter_map(|location| {
            let path = location.full_path();
            let metadata = fs::symlink_metadata(&path).ok()?;
            if metadata.file_type().is_symlink() {
                return None;
            }
            let size_bytes = if metadata.is_dir() {
                directory_size(&path)
            } else {
                metadata.len()
            };
            Some(HostTrace {
                id: location.id.to_owned(),
                path: path.display().to_string(),
                size_bytes,
            })
        })
        .collect()
}

/// Summe aller Dateigrößen unterhalb eines Ordners, ohne Links zu folgen.
/// Nicht lesbare Einträge zählen als 0, weil die Anzeige nur eine Orientierung ist.
pub fn directory_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("iap-host-traces-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("Testordner anlegen");
        root
    }

    #[test]
    fn setting_key_uses_short_host_prefix() {
        assert_eq!(
            ask_setting_key("0123456789abcdef0123456789abcdef"),
            "host.0123456789abcdef.ask_on_exit"
        );
        assert_eq!(ask_setting_key("kurz"), "host.kurz.ask_on_exit");
    }

    #[test]
    fn locations_without_local_app_data_only_cover_temp() {
        let locations = known_locations(None, Path::new("T"), "at.iap.desktop");
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].id, "vault_hot");
        assert_eq!(locations[0].full_path(), Path::new("T").join("PortableAI"));
    }

    #[test]
    fn collect_measures_existing_folders_and_skips_missing() {
        let root = temp_root("collect");
        let local = root.join("local");
        let temp = root.join("temp");
        let cache = local.join("IAP").join("model-cache").join("abc");
        fs::create_dir_all(&cache).expect("Cache anlegen");
        fs::write(cache.join("model.gguf"), vec![0u8; 1000]).expect("Datei schreiben");
        fs::write(cache.join("hosts.json"), vec![0u8; 24]).expect("Datei schreiben");
        fs::create_dir_all(&temp).expect("Temp anlegen");

        let traces = collect(&known_locations(Some(&local), &temp, "at.iap.desktop"));
        assert_eq!(traces.len(), 1, "nur der Modell-Cache existiert");
        assert_eq!(traces[0].id, "model_cache");
        assert_eq!(traces[0].size_bytes, 1024);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn directory_size_of_missing_folder_is_zero() {
        assert_eq!(directory_size(Path::new("gibt-es-nicht-iap")), 0);
    }
}
