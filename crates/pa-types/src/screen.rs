//! Verträge für „Bildschirm ansehen“: genau eine Aufnahme je Auftrag.

use serde::{Deserialize, Serialize};

/// Was aufgenommen werden soll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScreenTarget {
    Monitor {
        index: u32,
    },
    Window {
        handle: i64,
    },
    /// Rechteck in physischen Pixeln des Monitors mit dem Index `monitor`.
    Region {
        monitor: u32,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
}

/// Ein wählbarer Monitor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub index: u32,
    pub name: String,
    /// Linke obere Ecke in physischen Pixeln des gesamten Desktops.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

/// Ein wählbares Fenster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub handle: i64,
    pub title: String,
}

/// Warum „Bildschirm ansehen“ nicht möglich ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenBlock {
    /// Kein Bildpfad (Modell + Projektor + Laufzeit): „Bildschirmverständnis nicht eingerichtet“.
    NotSetUp,
    /// Vision auf T0 bleibt bis zum Freigabetor aus.
    TierBlocked,
    VaultLocked,
}

/// Verfügbarkeit für die Oberfläche.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenStatus {
    pub available: bool,
    pub block: Option<ScreenBlock>,
    /// Klartext, was konkret fehlt.
    pub detail: Option<String>,
}

/// Auswahlliste vor der Aufnahme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenSources {
    pub monitors: Vec<MonitorInfo>,
    pub windows: Vec<WindowInfo>,
}

/// Ergebnis eines Auftrags. Das Bild selbst verlässt das Backend nie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenAnswer {
    pub conversation_id: String,
    pub message_id: String,
    pub captured_unix_ms: i64,
    pub source_label: String,
    pub answer: String,
}
