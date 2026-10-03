use pa_types::chat::{Message, MessageRole};
use serde_json::{json, Value};

use crate::{adapter::ModelAdapter, InferenceError};

const SYSTEM_PREFIX: &str = "Du bist PortableAI. Antworte hilfreich, präzise und in der Sprache des Nutzers. Du hast in dieser MVP-Phase keine Werkzeuge und behauptest keine Werkzeugaufrufe.";
const STOPS: &[&str] = &["<end_of_turn>"];

/// Formatiert ausschließlich das gemessene Standardmodell Gemma 4 E2B.
pub struct Gemma4Adapter {
    alias: String,
}

impl Gemma4Adapter {
    /// Bindet den Adapter an den Alias des aktuell geladenen Servers.
    pub fn new(alias: impl Into<String>) -> Self {
        Self {
            alias: alias.into(),
        }
    }
}

impl ModelAdapter for Gemma4Adapter {
    fn request(&self, messages: &[Message]) -> Result<Value, InferenceError> {
        let mut wire_messages = vec![json!({"role": "system", "content": SYSTEM_PREFIX})];
        for message in messages {
            let role = match message.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
            };
            wire_messages.push(json!({"role": role, "content": message.content}));
        }
        Ok(json!({
            "model": self.alias,
            "messages": wire_messages,
            "stream": true,
            "parse_tool_calls": false,
            "temperature": 0.7,
            "stop": STOPS,
        }))
    }

    fn stop_sequences(&self) -> &[&str] {
        STOPS
    }
}
