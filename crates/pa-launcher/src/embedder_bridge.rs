//! Bindet `pa_inference::embeddings::EmbeddingClient` an den
//! `pa_memory::Embedder`-Trait. Lebt in pa-launcher, weil hier alle
//! beteiligten Crates schon zusammenkommen.

use pa_inference::embeddings::EmbeddingClient;
use pa_memory::embedder::{Embedder, EmbedderError, EmbeddingVector};
use pa_types::memory::EMBEDDING_DIM;

/// Adapter: sieht für `pa-memory` aus wie ein Embedder, führt intern aber
/// einen Loopback-JSON-POST an `llama-server /embeddings` aus.
pub struct LoopbackEmbedder {
    client: EmbeddingClient,
}

impl LoopbackEmbedder {
    /// Bindet die Bridge an einen bereits konfigurierten Client. `EMBEDDING_DIM`
    /// wird beim Bau des Clients erzwungen, damit ein falsch dimensioniertes
    /// Modell nicht durch die Cosinus-Suche rutscht.
    pub fn new(client: EmbeddingClient) -> Self {
        Self { client }
    }
}

impl Embedder for LoopbackEmbedder {
    fn embed(&self, text: &str) -> Result<EmbeddingVector, EmbedderError> {
        let vector = self
            .client
            .embed_single(text)
            .map_err(|error| EmbedderError::Transport(error.to_string()))?;
        if vector.len() != EMBEDDING_DIM {
            return Err(EmbedderError::DimensionMismatch {
                expected: EMBEDDING_DIM,
                actual: vector.len(),
            });
        }
        Ok(vector)
    }
}
