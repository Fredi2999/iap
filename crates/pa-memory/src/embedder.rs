//! Trait-Port für Embedding-Erzeugung.
//!
//! Der eigentliche Aufruf des `/embeddings`-Endpunkts von llama-server liegt
//! in `pa-inference`; hier wird nur die abstrakte Schnittstelle beschrieben,
//! damit `pa-memory` frei von HTTP- und JSON-Details bleibt und in Tests mit
//! einem deterministischen Mock-Embedder gefüttert werden kann.

use pa_types::memory::EMBEDDING_DIM;
use thiserror::Error;

/// Ergebnis eines Embedder-Aufrufs. Länge muss `EMBEDDING_DIM` entsprechen —
/// die Prüfung erfolgt bereits in `pa-vault::memory::upsert_embedding`.
pub type EmbeddingVector = Vec<f32>;

/// Fehlerklasse einer Embedding-Anfrage.
#[derive(Debug, Error)]
pub enum EmbedderError {
    /// Der Server ist nicht erreichbar oder das Modell fehlt.
    #[error("Embedder nicht verfügbar: {0}")]
    Unavailable(String),
    /// Der Server lieferte eine andere Dimension als erwartet.
    #[error("Embedding-Dimension {actual} passt nicht zum konfigurierten Wert {expected}")]
    DimensionMismatch { expected: usize, actual: usize },
    /// Sonstiger Transportfehler; bewahrt die Ursache im Text.
    #[error("Embedder-Transport: {0}")]
    Transport(String),
}

/// Wird von `pa-inference` implementiert. Für Tests und den ersten Wurf
/// stellt `HashingEmbedder` einen deterministischen, aber semantisch
/// bedeutungslosen Vektor zur Verfügung; er reicht, um die Cosinus- und
/// RRF-Logik testbar zu machen und den Vault-Pfad zu belegen, bevor das
/// echte GGUF-Embedding-Modell ins Manifest kommt.
pub trait Embedder: Send + Sync {
    /// Berechnet einen Vektor mit `EMBEDDING_DIM` Elementen für den Text.
    fn embed(&self, text: &str) -> Result<EmbeddingVector, EmbedderError>;
}

/// Rein deterministischer Mock-Embedder — nur für Tests und den bewussten
/// „Vault-Pfad ohne Modell"-Fall gedacht. Er nutzt eine einfache
/// Hash-basierte Ableitung, sodass identische Texte identische Vektoren
/// liefern und ähnliche Texte NICHT ähnliche Vektoren erzeugen (das ist
/// gewollt, damit man in Tests nicht versehentlich semantische Nähe
/// simuliert).
pub struct HashingEmbedder;

impl Embedder for HashingEmbedder {
    fn embed(&self, text: &str) -> Result<EmbeddingVector, EmbedderError> {
        let mut vector = vec![0.0_f32; EMBEDDING_DIM];
        // Rolling FNV-ähnlicher Hash über die Bytes; deterministisch und ohne
        // externe Abhängigkeit.
        let mut state: u64 = 0xcbf29ce484222325;
        for (index, byte) in text.as_bytes().iter().enumerate() {
            state ^= *byte as u64;
            state = state.wrapping_mul(0x100000001b3);
            let bucket = index % EMBEDDING_DIM;
            vector[bucket] += ((state & 0xffff) as f32 / 65535.0) - 0.5;
        }
        // Auf L2-Norm 1 normalisieren, damit Cosinus-Vergleiche sinnvoll bleiben.
        let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in vector.iter_mut() {
                *value /= norm;
            }
        }
        Ok(vector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_embedder_is_deterministic_and_normalized() {
        let embedder = HashingEmbedder;
        let a = embedder.embed("Rust ist cool").unwrap();
        let b = embedder.embed("Rust ist cool").unwrap();
        assert_eq!(a, b);
        let norm = a.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 0.01,
            "L2-Norm sollte ~1 sein, war {norm}"
        );
    }

    #[test]
    fn hashing_embedder_returns_the_expected_dimension() {
        let embedder = HashingEmbedder;
        let vector = embedder.embed("hello").unwrap();
        assert_eq!(vector.len(), EMBEDDING_DIM);
    }
}
