use pa_types::chat::{Message, MessageRole};
use serde_json::{json, Value};

use crate::{adapter::ModelAdapter, InferenceError};

const SYSTEM_PREFIX: &str = "Du bist IAP. Antworte präzise in der Sprache des Nutzers. Verwende nur die Werkzeuge, die dir in der aktuellen Anfrage ausdrücklich bereitgestellt werden.";
const STOPS: &[&str] = &["<|im_end|>"];

/// Hält die ChatML-Endmarke des lokalen Qwen3-Instruct-Modells vom Antworttext fern.
pub struct Qwen3Adapter {
    alias: String,
}

impl Qwen3Adapter {
    /// Bindet die Anfrage an den Alias des laufenden lokalen Servers.
    pub fn new(alias: impl Into<String>) -> Self {
        Self {
            alias: alias.into(),
        }
    }
}

impl ModelAdapter for Qwen3Adapter {
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
            "top_p": 0.8,
            "stop": STOPS,
        }))
    }

    fn stop_sequences(&self) -> &[&str] {
        STOPS
    }
}
