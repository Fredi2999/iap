//! Retrieval- und Extraktionslogik des Memory-Systems (Konzept 6).
//!
//! pa-memory kennt `pa-vault` ausschließlich über das dünne Persistenz-Modul
//! `pa_vault::memory` und die geteilte `Arc<Mutex<HotVault>>`-Sperre, so
//! wie schon der Konversationspfad. Die eigentliche Inferenz
//! (`Embedder`-Trait) lebt in `pa-inference`; die Faktenextraktion nutzt
//! einen `FactExtractor`, der ebenfalls extern verankert ist.
//!
//! Retrieval folgt strikt Konzept 6.3:
//! 1. BM25 über FTS5 (via `pa_vault::memory::search_bm25`)
//! 2. Vektorsuche über Rust-Cosinus (`vector::cosine_similarity`)
//! 3. Reciprocal Rank Fusion `score = Σ 1/(60 + rang_i)`
//! 4. Optionaler Recency- und Pin-Boost
//! 5. Harte Kappung auf ein Token-Budget

pub mod embedder;
pub mod extractor;
pub mod graph;
pub mod store;
pub mod vector;

use thiserror::Error;

pub use embedder::{Embedder, EmbedderError, EmbeddingVector};
pub use extractor::{ExtractedFact, FactExtractor, FactExtractorError, TriggerReason};
pub use store::{IngestFact, MemoryStore, RetrievalOptions};

/// Bündelt Fehler aus Persistenz, Inferenz und Extraktion; Aufrufer wollen
/// sie an derselben Stelle behandeln.
#[derive(Debug, Error)]
pub enum MemoryError {
    /// Persistenz-Fehler aus pa-vault (Schema, SQLCipher, Fremdschlüssel).
    #[error("Vault: {0}")]
    Vault(#[from] pa_vault::VaultError),
    /// Inferenz-Fehler beim Erzeugen von Embeddings.
    #[error("Embedder: {0}")]
    Embedder(#[from] EmbedderError),
    /// Fehler in der Faktenextraktion (LLM oder Parser).
    #[error("Extraktor: {0}")]
    Extractor(#[from] FactExtractorError),
    /// Vault-Mutex vergiftet — der Prozess sollte sauber beendet werden.
    #[error("Vault-Mutex vergiftet")]
    Poisoned,
    /// Konfigurationsfehler, z. B. inkompatible Embedding-Dimension.
    #[error("ungültige Memory-Konfiguration: {0}")]
    InvalidConfiguration(String),
    /// Es gibt keinen Fakt mit dieser Kennung.
    #[error("Fakt `{0}` nicht gefunden")]
    NotFound(String),
}
