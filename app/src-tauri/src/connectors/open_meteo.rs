//! Open-Meteo: Ortssuche und Wettervorhersage, kostenlos und ohne Schlüssel.
//!
//! Formate geprüft am 2026-10-01 (echte Antworten):
//! - Ort: `GET geocoding-api.open-meteo.com/v1/search?name=…&count=…&language=de&format=json`
//!   → `{"results":[{name, latitude, longitude, country, admin1, timezone}]}`
//! - Wetter: `GET api.open-meteo.com/v1/forecast?latitude=…&longitude=…&current=…&daily=…&timezone=auto&forecast_days=…`
//!   → `{current:{time,temperature_2m,weather_code,wind_speed_10m}, daily:{time[],weather_code[],
//!   temperature_2m_max[],temperature_2m_min[],precipitation_sum[]}}`
//!
//! Nutzungsbedingung: kostenlos nur für nicht-kommerzielle Nutzung (siehe open-meteo.com/en/pricing);
//! die Konnektor-Karte weist darauf hin.

use std::sync::Arc;

use pa_policy::egress::{Connector, PublicText};
use serde::Deserialize;

use super::{get_json, ConnectorError};
use crate::net::{SystemTransport, Transport};

const SERVICE: &str = "Open-Meteo";
const GEO_HOST: &str = "geocoding-api.open-meteo.com";
const API_HOST: &str = "api.open-meteo.com";

#[derive(Debug, Deserialize)]
struct GeoResponse {
    #[serde(default)]
    results: Vec<Place>,
}

/// Ein gefundener Ort.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub admin1: Option<String>,
}

impl Place {
    fn label(&self) -> String {
        let mut parts = vec![self.name.clone()];
        if let Some(region) = self
            .admin1
            .as_deref()
            .filter(|r| !r.is_empty() && *r != self.name)
        {
            parts.push(region.to_owned());
        }
        if let Some(country) = self.country.as_deref().filter(|c| !c.is_empty()) {
            parts.push(country.to_owned());
        }
        parts.join(", ")
    }
}

#[derive(Debug, Deserialize)]
struct Forecast {
    #[serde(default)]
    current: Option<Current>,
    #[serde(default)]
    daily: Option<Daily>,
}

#[derive(Debug, Deserialize)]
struct Current {
    #[serde(default)]
    temperature_2m: Option<f64>,
    #[serde(default)]
    weather_code: Option<i64>,
    #[serde(default)]
    wind_speed_10m: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct Daily {
    #[serde(default)]
    time: Vec<String>,
    #[serde(default)]
    weather_code: Vec<Option<i64>>,
    #[serde(default)]
    temperature_2m_max: Vec<Option<f64>>,
    #[serde(default)]
    temperature_2m_min: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_sum: Vec<Option<f64>>,
}

/// Beschreibung eines WMO-Wettercodes (die Codes, die Open-Meteo liefert).
pub fn wmo_text(code: i64) -> &'static str {
    match code {
        0 => "klar",
        1 => "überwiegend klar",
        2 => "teilweise bewölkt",
        3 => "bedeckt",
        45 | 48 => "Nebel",
        51 | 53 | 55 => "Nieselregen",
        56 | 57 => "gefrierender Nieselregen",
        61 => "leichter Regen",
        63 => "Regen",
        65 => "starker Regen",
        66 | 67 => "gefrierender Regen",
        71 => "leichter Schneefall",
        73 => "Schneefall",
        75 => "starker Schneefall",
        77 => "Schneegriesel",
        80 | 81 => "Regenschauer",
        82 => "heftige Regenschauer",
        85 | 86 => "Schneeschauer",
        95 => "Gewitter",
        96 | 99 => "Gewitter mit Hagel",
        _ => "unbekanntes Wetter",
    }
}

pub struct OpenMeteoClient {
    transport: Arc<dyn Transport>,
}

impl OpenMeteoClient {
    pub fn new() -> Self {
        Self::with_transport(Arc::new(SystemTransport))
    }

    pub fn with_transport(transport: Arc<dyn Transport>) -> Self {
        Self { transport }
    }

    /// Sucht einen Ort und nimmt den besten Treffer.
    ///
    /// # Errors
    /// Netz-, Status-, Formatfehler; `NotFound`, wenn es keinen Ort gibt.
    pub fn find_place(&self, name: &PublicText) -> Result<Place, ConnectorError> {
        let response: GeoResponse = get_json(
            self.transport.as_ref(),
            Connector::OpenMeteo,
            SERVICE,
            GEO_HOST,
            "/v1/search",
            &[
                ("name", name.as_str()),
                ("count", "1"),
                ("language", "de"),
                ("format", "json"),
            ],
            &[("Accept", "application/json")],
        )?;
        response.results.into_iter().next().ok_or_else(|| {
            ConnectorError::NotFound(format!("Ich kenne keinen Ort „{}“.", name.as_str()))
        })
    }

    /// Wetterbericht als Text: aktuell und für die nächsten `days` Tage.
    ///
    /// # Errors
    /// Netz-, Status- und Formatfehler.
    pub fn report(&self, place: &Place, days: u32) -> Result<String, ConnectorError> {
        let days = days.clamp(1, 7).to_string();
        let latitude = format!("{:.4}", place.latitude);
        let longitude = format!("{:.4}", place.longitude);
        let forecast: Forecast = get_json(
            self.transport.as_ref(),
            Connector::OpenMeteo,
            SERVICE,
            API_HOST,
            "/v1/forecast",
            &[
                ("latitude", &latitude),
                ("longitude", &longitude),
                ("current", "temperature_2m,weather_code,wind_speed_10m"),
                (
                    "daily",
                    "weather_code,temperature_2m_max,temperature_2m_min,precipitation_sum",
                ),
                ("timezone", "auto"),
                ("forecast_days", &days),
            ],
            &[("Accept", "application/json")],
        )?;
        Ok(format_report(place, &forecast))
    }
}

impl Default for OpenMeteoClient {
    fn default() -> Self {
        Self::new()
    }
}

fn format_report(place: &Place, forecast: &Forecast) -> String {
    let mut out = format!("Wetter für {}:\n", place.label());
    if let Some(now) = &forecast.current {
        if let (Some(temp), Some(code)) = (now.temperature_2m, now.weather_code) {
            out.push_str(&format!("Jetzt: {temp:.0} °C, {}", wmo_text(code)));
            if let Some(wind) = now.wind_speed_10m {
                out.push_str(&format!(", Wind {wind:.0} km/h"));
            }
            out.push('\n');
        }
    }
    if let Some(daily) = &forecast.daily {
        for (i, day) in daily.time.iter().enumerate() {
            let code = daily.weather_code.get(i).copied().flatten();
            let max = daily.temperature_2m_max.get(i).copied().flatten();
            let min = daily.temperature_2m_min.get(i).copied().flatten();
            let rain = daily.precipitation_sum.get(i).copied().flatten();
            out.push_str(&format!("{day}: "));
            out.push_str(code.map_or("unbekannt", wmo_text));
            if let (Some(min), Some(max)) = (min, max) {
                out.push_str(&format!(", {min:.0} bis {max:.0} °C"));
            }
            if let Some(rain) = rain {
                out.push_str(&format!(", Niederschlag {rain:.1} mm"));
            }
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::testing::{open_net, Fake};

    fn public(text: &str) -> PublicText {
        PublicText::new(text, pa_policy::egress::DataOrigin::UserPublic).unwrap()
    }

    const GEO: &str = r#"{"results":[{"id":2950159,"name":"Berlin","latitude":52.52437,"longitude":13.41053,"country":"Deutschland","admin1":"Land Berlin","timezone":"Europe/Berlin"}],"generationtime_ms":0.6}"#;
    const FORECAST: &str = r#"{"latitude":52.52,"longitude":13.41,"timezone":"Europe/Berlin","current_units":{"temperature_2m":"°C"},"current":{"time":"2026-10-01T11:45","interval":900,"temperature_2m":20.4,"weather_code":2,"wind_speed_10m":12.3},"daily_units":{"time":"iso8601"},"daily":{"time":["2026-10-01","2026-10-02"],"weather_code":[3,61],"temperature_2m_max":[24.0,19.4],"temperature_2m_min":[14.4,15.1],"precipitation_sum":[0.00,0.10]}}"#;

    #[test]
    fn a_place_is_found_and_the_forecast_is_requested_with_encoded_coordinates() {
        let _guard = open_net();
        let fake = Fake::new(&[(200, GEO), (200, FORECAST)]);
        let client = OpenMeteoClient::with_transport(fake.clone());
        let place = client.find_place(&public("Berlin Mitte")).expect("Ort");
        let text = client.report(&place, 99).expect("Bericht");
        assert!(
            text.starts_with("Wetter für Berlin, Land Berlin, Deutschland:\n"),
            "{text}"
        );
        assert!(
            text.contains("Jetzt: 20 °C, teilweise bewölkt, Wind 12 km/h"),
            "{text}"
        );
        assert!(
            text.contains("2026-10-02: leichter Regen, 15 bis 19 °C, Niederschlag 0.1 mm"),
            "{text}"
        );

        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0].0, "geocoding-api.open-meteo.com");
        assert_eq!(
            seen[0].2,
            "/v1/search?name=Berlin%20Mitte&count=1&language=de&format=json"
        );
        assert_eq!(seen[1].0, "api.open-meteo.com");
        assert!(seen[1].2.starts_with("/v1/forecast?latitude=52.5244&longitude=13.4105&current=temperature_2m%2Cweather_code%2Cwind_speed_10m&daily="), "{}", seen[1].2);
        assert!(
            seen[1].2.ends_with("&timezone=auto&forecast_days=7"),
            "{}",
            seen[1].2
        );
    }

    #[test]
    fn an_unknown_place_is_a_clear_message_and_bad_answers_are_errors() {
        let _guard = open_net();
        let fake = Fake::new(&[
            (200, r#"{"generationtime_ms":0.1}"#),
            (500, "{}"),
            (200, "kein json"),
        ]);
        let client = OpenMeteoClient::with_transport(fake);
        let error = client.find_place(&public("Nirgendwo")).unwrap_err();
        assert!(
            matches!(&error, ConnectorError::NotFound(m) if m.contains("Nirgendwo")),
            "{error}"
        );
        let place = Place {
            name: "X".into(),
            latitude: 1.0,
            longitude: 2.0,
            country: None,
            admin1: None,
        };
        assert!(client
            .report(&place, 1)
            .unwrap_err()
            .to_string()
            .contains("nicht erreichbar"));
        assert!(matches!(
            client.report(&place, 1),
            Err(ConnectorError::BadResponse(_))
        ));
    }

    #[test]
    fn a_report_with_missing_values_still_reads_well() {
        let place = Place {
            name: "Ort".into(),
            latitude: 0.0,
            longitude: 0.0,
            country: None,
            admin1: Some("Ort".into()),
        };
        let forecast: Forecast = serde_json::from_str(r#"{"daily":{"time":["2026-10-03"],"weather_code":[null],"temperature_2m_max":[],"temperature_2m_min":[],"precipitation_sum":[]}}"#).unwrap();
        let text = format_report(&place, &forecast);
        assert_eq!(text, "Wetter für Ort:\n2026-10-03: unbekannt\n");
        assert_eq!(wmo_text(95), "Gewitter");
        assert_eq!(wmo_text(1234), "unbekanntes Wetter");
    }
}
