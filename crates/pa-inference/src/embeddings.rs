//! Klient für den `/embeddings`-Endpunkt einer zweiten `llama-server`-Instanz.
//!
//! Der Client benutzt denselben `LoopbackEndpoint`-Typ wie der Chat-Pfad;
//! damit gilt auch hier: keine DNS-Auflösung, keine externen Hosts,
//! nur 127.0.0.1 mit vergebenem Zugriffstoken. Der Chatserver bleibt
//! auf seiner eigenen Instanz laufen, der Embedder auf einem eigenen
//! Port mit eigenem Token.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::{loopback::LoopbackEndpoint, InferenceError};

/// Dünner Wrapper um einen Loopback-Endpunkt, spezialisiert auf JSON-
/// Embedding-Anfragen. Trennt die Verantwortlichkeit vom Chat-Streaming.
pub struct EmbeddingClient {
    endpoint: LoopbackEndpoint,
    request_timeout: Duration,
    expected_dim: usize,
}

impl EmbeddingClient {
    /// Bindet den Client an eine bereits erzeugte Loopback-Endpoint-Instanz.
    ///
    /// `expected_dim` hilft, ein falsches Modell früh zu erkennen (Konzept
    /// 6.2 legt 768 fest; wir vergleichen die Antwort).
    pub fn new(endpoint: LoopbackEndpoint, expected_dim: usize) -> Self {
        Self {
            endpoint,
            request_timeout: Duration::from_secs(30),
            expected_dim,
        }
    }

    /// Feineinstellung des Response-Timeouts (z. B. für Tests).
    pub fn with_timeout(mut self, request_timeout: Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }

    /// Führt einen Embedding-Aufruf durch. Der Server-Body richtet sich
    /// nach der llama-server-API: `POST /embeddings { "content": "..." }`
    /// mit Antwort `{"embedding": [floats...]}` (oder Liste bei Batch).
    pub fn embed_single(&self, text: &str) -> Result<Vec<f32>, InferenceError> {
        let body = serde_json::to_vec(&json!({ "content": text }))
            .map_err(|error| InferenceError::Http(format!("JSON-Body: {error}")))?;
        let response =
            self.endpoint
                .post_json_collect("/embeddings", &body, self.request_timeout)?;
        parse_single_embedding(&response, self.expected_dim)
    }
}

/// Aus dem Antwort-Body eines llama-server-`/embeddings`-Aufrufs den
/// Vektor extrahieren. Unterstützt beide dokumentierten Formen:
/// `{"embedding": [...]}` und `[{"embedding": [...]}]`.
pub fn parse_single_embedding(
    bytes: &[u8],
    expected_dim: usize,
) -> Result<Vec<f32>, InferenceError> {
    #[derive(Deserialize)]
    struct SingleResponse {
        embedding: Vec<f32>,
    }
    #[derive(Deserialize)]
    struct BatchEntry {
        embedding: Vec<f32>,
    }

    if let Ok(single) = serde_json::from_slice::<SingleResponse>(bytes) {
        return validate(single.embedding, expected_dim);
    }
    if let Ok(batch) = serde_json::from_slice::<Vec<BatchEntry>>(bytes) {
        if let Some(first) = batch.into_iter().next() {
            return validate(first.embedding, expected_dim);
        }
    }
    Err(InferenceError::Http(
        "Antwort enthält kein `embedding`-Feld".to_owned(),
    ))
}

fn validate(vector: Vec<f32>, expected_dim: usize) -> Result<Vec<f32>, InferenceError> {
    if vector.len() != expected_dim {
        return Err(InferenceError::Http(format!(
            "Embedding-Dimension {} passt nicht zu erwarteten {}",
            vector.len(),
            expected_dim
        )));
    }
    Ok(vector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_response_format_is_parsed() {
        let body = br#"{"embedding":[0.1,0.2,0.3]}"#;
        let vector = parse_single_embedding(body, 3).unwrap();
        assert_eq!(vector, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn batch_response_format_is_parsed_as_first_entry() {
        let body = br#"[{"embedding":[0.1,0.2,0.3]},{"embedding":[0.4,0.5,0.6]}]"#;
        let vector = parse_single_embedding(body, 3).unwrap();
        assert_eq!(vector, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn dimension_mismatch_becomes_a_hard_error() {
        let body = br#"{"embedding":[0.1,0.2,0.3]}"#;
        let error = parse_single_embedding(body, 5).unwrap_err();
        match error {
            InferenceError::Http(message) => assert!(message.contains("Dimension")),
            other => panic!("unerwarteter Fehler: {other:?}"),
        }
    }

    #[test]
    fn missing_field_returns_error_instead_of_empty_vector() {
        let body = br#"{"nothing":true}"#;
        assert!(parse_single_embedding(body, 3).is_err());
    }
}
