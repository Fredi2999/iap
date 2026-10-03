use pa_types::chat::{Message, MessageRole};
use serde_json::{json, Value};

use crate::{adapter::ModelAdapter, InferenceError};

const SYSTEM_PREFIX: &str = "Du bist IAP. Antworte präzise in der Sprache des Nutzers. Verwende nur die Werkzeuge, die dir in der aktuellen Anfrage ausdrücklich bereitgestellt werden.";
const STOPS: &[&str] = &[];

/// Lässt das GGUF-Chat-Template und die EOS-Marke des Servers für Ministral 3 wirken.
pub struct Ministral3Adapter {
    alias: String,
}

impl Ministral3Adapter {
    /// Bindet die Anfrage an den Alias des laufenden lokalen Servers.
    pub fn new(alias: impl Into<String>) -> Self {
        Self {
            alias: alias.into(),
        }
    }
}

impl ModelAdapter for Ministral3Adapter {
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
            "temperature": 0.05,
            "top_p": 0.9,
        }))
    }

    fn stop_sequences(&self) -> &[&str] {
        STOPS
    }
}
