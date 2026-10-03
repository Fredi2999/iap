//! Gemeinsame Datentypen für das Memory-System nach Konzept 6.2.
//!
//! Die Strukturen leben hier, damit `pa-vault` (Persistenz) und
//! `pa-memory` (Retrieval- und Extraktionslogik) sie ohne
//! Kreisabhängigkeit teilen können. Alles ist serialisierbar, damit die
//! Tauri-IPC im „Memory"-Bereich der UI dieselben Typen sieht.

use serde::{Deserialize, Serialize};

/// Kategorien laut Konzept 6.2 — offen erweiterbar, aber Kern ist
/// „Vorliebe / Projekt / Person / Fähigkeit / Randbedingung".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactCategory {
    Preference,
    Project,
    Person,
    Skill,
    Constraint,
    Other,
}

impl FactCategory {
    /// Stabile Byte-Bezeichnung für Persistenz und IPC.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preference => "preference",
            Self::Project => "project",
            Self::Person => "person",
            Self::Skill => "skill",
            Self::Constraint => "constraint",
            Self::Other => "other",
        }
    }

    /// Fallback auf `Other`, wenn ein unbekannter Wert aus der DB kommt.
    pub fn from_str_or_other(value: &str) -> Self {
        match value {
            "preference" => Self::Preference,
            "project" => Self::Project,
            "person" => Self::Person,
            "skill" => Self::Skill,
            "constraint" => Self::Constraint,
            _ => Self::Other,
        }
    }
}

/// Extrahierter Fakt im semantischen Gedächtnis (Konzept 6.1 Ebene 3).
///
/// Widersprüche löschen nichts, sondern verweisen per `superseded_by` auf
/// den nachfolgenden Fakt — die Historie bleibt vollständig einsehbar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub text: String,
    pub category: FactCategory,
    /// 0.0–1.0; niedrig bei automatischer Extraktion, hoch bei
    /// nutzerbestätigten Fakten.
    pub confidence: f32,
    /// Nachricht, aus der der Fakt extrahiert wurde (nicht verpflichtend).
    pub source_message_id: Option<String>,
    pub valid_from_unix_ms: i64,
    /// `None` = weiterhin gültig.
    pub valid_until_unix_ms: Option<i64>,
    /// ID eines Nachfolger-Faktes, wenn dieser den vorliegenden ersetzt.
    pub superseded_by: Option<String>,
    pub user_verified: bool,
    pub access_count: i64,
    pub last_accessed_unix_ms: Option<i64>,
}

/// Textchunk für Dokumentgedächtnis (Konzept 6.1 Ebene 4).
///
/// `heading_path` erlaubt Retrieval, das die Kapitelhierarchie eines
/// Dokuments respektiert (Slash-getrennt: „Kapitel/Abschnitt").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub doc_id: String,
    pub ordinal: i64,
    pub text: String,
    pub heading_path: Option<String>,
    pub tokens: i64,
}

/// Kategorisiert einen Vektor- oder FTS-Eintrag. Wird im gemeinsamen
/// Retrieval verwendet, damit derselbe Suchpfad sowohl Fakten als auch
/// Dokumentchunks bedient.
///
/// `Ord`/`Hash` sind wichtig, weil `pa-memory` diese Werte in
/// `BTreeMap`-Schlüsseln als Teil der RRF-Aggregation nutzt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    Fact,
    Chunk,
}

impl ItemType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Chunk => "chunk",
        }
    }

    /// Bewusst nicht `from_str`, damit es nicht mit dem std-Trait
    /// `FromStr` kollidiert (dessen Rückgabe wäre ein Fehler, hier ist
    /// die Rückgabe ein `Option`, weil unbekannte Werte im DB-Kontext
    /// verworfen werden statt einen Fehler zu produzieren).
    pub fn parse_wire(value: &str) -> Option<Self> {
        match value {
            "fact" => Some(Self::Fact),
            "chunk" => Some(Self::Chunk),
            _ => None,
        }
    }
}

/// Ein 768-dim Embedding im MVP (Konzept 6.2). Wird als `f32[]` gespeichert
/// und im Vault als BLOB persistiert (little-endian, keine Kompression).
pub const EMBEDDING_DIM: usize = 768;

/// Wird als Rückgabetyp der Retrieval-Funktionen benutzt und in der UI
/// direkt angezeigt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryHit {
    pub item_type: ItemType,
    pub item_id: String,
    /// Der gerankten Ergebnisliste zugeordneter Reciprocal-Rank-Fusion-Score.
    pub score: f64,
    /// Kurzer, für die UI vorformatierter Textausriss (in Konzept 6.3
    /// als „hart auf ein Token-Budget begrenzt" beschrieben — die
    /// Kürzung passiert im Retrieval-Layer, nicht in der Datenbank).
    pub preview: String,
}

/// Art eines Knotens im Gedächtnis-Graphen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeKind {
    /// Ein gespeicherter Fakt.
    Fact,
    /// Sammelknoten einer Kategorie.
    Category,
    /// Ein `[[Link]]`, zu dem es (noch) keinen Fakt gibt.
    Ghost,
}

/// Art einer Kante.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeKind {
    /// Fakt gehört zu einer Kategorie.
    Category,
    /// Alter Fakt wurde durch einen neuen ersetzt (von alt nach neu).
    Supersedes,
    /// Ein `[[Link]]` im Text verweist auf einen anderen Fakt.
    Link,
}

/// Ein Knoten des Gedächtnis-Graphen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    /// `fact:<id>`, `category:<name>` oder `ghost:<kleingeschriebener Titel>`.
    pub id: String,
    pub kind: GraphNodeKind,
    /// Titel (erste Zeile des Fakts) bzw. Kategoriename.
    pub label: String,
    pub category: Option<FactCategory>,
    pub verified: bool,
    pub superseded: bool,
    pub confidence: f32,
    /// Wie oft der Fakt bei Fragen herangezogen wurde (bestimmt die Größe).
    pub access_count: i64,
    /// Nur Fakten: Zeitpunkt der Aufnahme, 0 wenn unbekannt.
    pub created_unix_ms: i64,
    /// Nur Fakten: stammt aus einer Unterhaltung (automatisch gelernt).
    pub learned: bool,
}

/// Eine Kante des Gedächtnis-Graphen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: GraphEdgeKind,
}

/// Gedächtnis als Graph: Knoten und Kanten, rein aus den gespeicherten Fakten abgeleitet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MemoryGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_round_trips_through_str() {
        for category in [
            FactCategory::Preference,
            FactCategory::Project,
            FactCategory::Person,
            FactCategory::Skill,
            FactCategory::Constraint,
            FactCategory::Other,
        ] {
            assert_eq!(FactCategory::from_str_or_other(category.as_str()), category);
        }
        assert_eq!(
            FactCategory::from_str_or_other("unknown"),
            FactCategory::Other
        );
    }

    #[test]
    fn item_type_round_trips() {
        for item in [ItemType::Fact, ItemType::Chunk] {
            assert_eq!(ItemType::parse_wire(item.as_str()), Some(item));
        }
        assert_eq!(ItemType::parse_wire("xyz"), None);
    }
}
