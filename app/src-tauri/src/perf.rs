//! Leistungs-Check je PC (Feature 3).
//!
//! Gemessen wird nur, was sich wirklich messen lässt: Startzeit des Modells,
//! Token pro Sekunde laut `llama-server` und der Arbeitsspeicher des Modell-
//! prozesses. Fehlt ein Wert, bleibt er leer und die Oberfläche zeigt
//! „nicht gemessen“ statt einer Schätzung.

use serde::{Deserialize, Serialize};

/// Ergebnis eines Checks, gespeichert im Tresor je PC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerfReport {
    pub measured_at_unix_ms: i64,
    pub model_id: String,
    pub model_name: String,
    /// Zeit vom Start des Modellprozesses bis zur Bereitschaft.
    pub model_start_ms: Option<u64>,
    pub prompt_per_second: Option<f64>,
    pub tokens_per_second: Option<f64>,
    /// Höchster Arbeitsspeicher des Modellprozesses während der Messung.
    pub ram_peak_bytes: Option<u64>,
    pub total_ram_bytes: u64,
    /// Empfohlene Denkstufe als interner Wert (`kurz`, `standard`, `sorgfältig`).
    pub recommended_thinking: Option<String>,
    /// Kleineres installiertes Modell, falls das aktuelle zu langsam ist.
    pub recommended_model_id: Option<String>,
}

/// Schlüssel der Vault-Einstellung für das Ergebnis dieses PCs.
pub fn setting_key(host_identifier: &str) -> String {
    let prefix: String = host_identifier.chars().take(16).collect();
    format!("host.{prefix}.perf")
}

/// Denkstufe passend zur gemessenen Geschwindigkeit.
///
/// Warum diese Grenzen: Unter 4 Token/s wartet man auf eine Antwort mit
/// Denkphase deutlich über eine Minute; ab 10 Token/s bleibt auch eine
/// gründlichere Stufe angenehm.
pub fn recommend_thinking(tokens_per_second: Option<f64>) -> Option<&'static str> {
    let speed = tokens_per_second?;
    Some(if speed < 4.0 {
        "kurz"
    } else if speed < 10.0 {
        "standard"
    } else {
        "sorgfältig"
    })
}

/// Empfiehlt das kleinste installierte Modell, wenn das aktuelle langsam ist
/// und es ein kleineres gibt. `models` sind (ID, Dateigröße).
pub fn recommend_model(
    tokens_per_second: Option<f64>,
    current_id: &str,
    models: &[(String, u64)],
) -> Option<String> {
    if tokens_per_second? >= 4.0 {
        return None;
    }
    let current_size = models.iter().find(|(id, _)| id == current_id)?.1;
    models
        .iter()
        .filter(|(id, size)| id != current_id && *size < current_size)
        .min_by_key(|(_, size)| *size)
        .map(|(id, _)| id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_follows_measured_speed() {
        assert_eq!(recommend_thinking(None), None);
        assert_eq!(recommend_thinking(Some(2.5)), Some("kurz"));
        assert_eq!(recommend_thinking(Some(6.0)), Some("standard"));
        assert_eq!(recommend_thinking(Some(18.0)), Some("sorgfältig"));
    }

    #[test]
    fn smaller_model_only_when_slow() {
        let models = vec![
            ("gross".to_owned(), 3_000),
            ("klein".to_owned(), 1_000),
            ("mittel".to_owned(), 2_000),
        ];
        assert_eq!(
            recommend_model(Some(2.0), "gross", &models),
            Some("klein".to_owned())
        );
        assert_eq!(recommend_model(Some(8.0), "gross", &models), None);
        assert_eq!(recommend_model(Some(2.0), "klein", &models), None);
        assert_eq!(recommend_model(None, "gross", &models), None);
    }

    #[test]
    fn setting_key_is_per_host() {
        assert_eq!(
            setting_key("abcdef0123456789zzz"),
            "host.abcdef0123456789.perf"
        );
    }
}
