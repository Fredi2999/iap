//! Kalender-Anbindung: Apple iCloud (CalDAV) und Google (geheime iCal-Adresse).
//!
//! Aufbau von unten nach oben: `xml` (kleiner Baum), `caldav` und `ics_url` (die zwei Protokolle,
//! beide über den einzigen HTTPS-Client in `net`), `sync` (Abgleich einer Quelle zu fertigen
//! Terminen). Die Verdrahtung mit Tresor, Policy und Oberfläche liegt in `calendar_cmds`.
//!
//! Nichts hier läuft von selbst: Jeder Netzaufruf geht auf einen Klick des Nutzers zurück
//! („Aktualisieren“, „Termin speichern“). Der Air Gap sperrt zweifach (Policy und `net`).

pub mod caldav;
pub mod ics_url;
pub mod sync;
pub mod xml;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::net::NetError;

/// Fehler der Kalender-Anbindung. Die Texte sind für den Nutzer lesbar und enthalten nie
/// Zugangsdaten, geheime Adressen oder Antwortinhalte.
#[derive(Debug, Error)]
pub enum CalError {
    /// Air Gap oder Host-Tabelle: es wurde nichts gesendet.
    #[error("{0}")]
    Blocked(String),
    #[error("Keine Verbindung zum Kalender-Server: {0}")]
    Net(String),
    #[error("Anmeldung abgelehnt. Prüfe Apple-ID und das app-spezifische Passwort.")]
    Auth,
    #[error("Der Kalender-Server hat die Anfrage abgelehnt (Status {0}).")]
    Rejected(u16),
    #[error("Unerwartete Antwort des Kalender-Servers: {0}")]
    Protocol(String),
    #[error("{0}")]
    Invalid(String),
    #[error("Unter dieser Kennung liegt im Kalender schon ein Termin.")]
    Exists,
}

impl From<NetError> for CalError {
    fn from(error: NetError) -> Self {
        match error {
            NetError::AirGap => Self::Blocked(
                "Air Gap ist eingeschaltet: Schalte ihn aus, um den Kalender zu verbinden oder zu aktualisieren."
                    .to_owned(),
            ),
            NetError::Blocked(reason) => Self::Blocked(reason),
            NetError::TooLarge => Self::Protocol("Die Antwort ist größer als erlaubt".to_owned()),
            other => Self::Net(other.to_string()),
        }
    }
}

/// Art einer verbundenen Quelle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Apple iCloud über CalDAV (lesen und anlegen).
    Icloud,
    /// Google über die geheime iCal-Adresse (nur lesen).
    GoogleIcs,
}

impl SourceKind {
    /// Ob in dieser Quelle Termine angelegt werden können.
    pub fn can_write(self) -> bool {
        matches!(self, Self::Icloud)
    }
}

/// Ein Kalender innerhalb einer Quelle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarRef {
    /// Server, auf dem der Kalender liegt (bei iCloud eine Gruppe `pNN-caldav.icloud.com`).
    pub host: String,
    /// Pfad des Kalenders, endet mit `/` (leer bei Google).
    pub href: String,
    pub name: String,
    /// `#RRGGBB`, wenn der Server eine Farbe nennt.
    #[serde(default)]
    pub color: Option<String>,
    /// Ob IAP hier Termine anlegen darf (Berechtigung des Kontos).
    #[serde(default)]
    pub can_write: bool,
}

/// Eine verbundene Quelle ohne Geheimnisse. Passwort bzw. geheime Adresse liegen getrennt im Tresor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub kind: SourceKind,
    pub label: String,
    /// Apple-ID (bei Google leer). Keine Geheimnisse.
    #[serde(default)]
    pub account: String,
    pub calendars: Vec<CalendarRef>,
    #[serde(default)]
    pub last_sync_unix_ms: Option<i64>,
    /// Letzter Fehler beim Abruf, lesbarer Text.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Serien, die nicht ausgewertet werden konnten (nur der erste Termin ist sichtbar).
    #[serde(default)]
    pub unsupported_rules: u32,
}

/// Ein Termin aus einer verbundenen Quelle, wie ihn die Oberfläche bekommt (schreibgeschützt).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEvent {
    pub id: String,
    pub source_id: String,
    pub source_label: String,
    pub calendar_name: String,
    #[serde(default)]
    pub color: Option<String>,
    pub title: String,
    pub start_unix_ms: i64,
    pub end_unix_ms: i64,
    pub all_day: bool,
    /// `YYYY-MM-DD`, nur Ganztag.
    #[serde(default)]
    pub start_date: Option<String>,
    /// `YYYY-MM-DD`, exklusiv, nur Ganztag.
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub recurring: bool,
}
