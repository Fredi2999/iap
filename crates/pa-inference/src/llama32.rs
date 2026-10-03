//! Adapter für Llama-3.2-3B-Instruct-Abliterated (Konzept 3.2).
//!
//! „Abliterated" bedeutet, dass die Ablehnungsrichtung in den Aktivierungen
//! unterdrückt wurde. Dokumentierte Nebenwirkungen: **schwächere
//! Instruktionsbefolgung, mehr Halluzination, instabileres strukturiertes
//! Output**. Konzept 3.2: „als manuell wählbares Zweitmodell anbieten,
//! nie als Agent-Mode-Default, und bei Nutzung eine strengere Grammar-
//! Erzwingung (GBNF) aktivieren."
//!
//! Umsetzung dieses Adapters:
//! - Chat-Template im Llama-3-Format (`<|begin_of_text|>`,
//!   `<|start_header_id|>role<|end_header_id|>`,
//!   `<|eot_id|>`). Llama-3.2 verwendet dieselben Tokens wie Llama-3.
//! - **Niedrigere Temperatur** (0.4 statt 0.7) und explizite
//!   `top_p`/`repetition_penalty`, weil Abliterated bei hoher Temperatur
//!   deutlich mehr driftet.
//! - **Standardgrammatik** [`STRICT_JSON_ENVELOPE_GBNF`] als Vorgabe, wenn
//!   die App eine Envelope-Antwort erzwingen will. Der Adapter aktiviert
//!   sie *nicht* automatisch — die Werkzeug- und Agent-Schleife setzt sie
//!   je Aufruf gezielt (`ChatOptions.grammar`).
//! - `parse_tool_calls: false` — Llama-3.2 hat kein natives Tool-Format
//!   wie Gemma 4; die Werkzeug-Envelope wird über den Prompt geregelt.

use pa_types::chat::{Message, MessageRole};
use serde_json::{json, Value};

use crate::{adapter::ModelAdapter, InferenceError};

const SYSTEM_PREFIX: &str = "Du bist PortableAI mit dem Zweitmodell Llama-3.2-3B-Abliterated. \
Antworte hilfreich, praezise, in der Sprache des Nutzers. Halluziniere nicht: sag lieber \
'ich weiss es nicht', statt zu raten. Bei Werkzeug-Anweisungen aus dem System-Prompt: \
antworte ausschliesslich als JSON-Envelope, ohne umgebende Prosa.";

const STOPS: &[&str] = &["<|eot_id|>", "<|end_of_text|>"];

/// Strengere GBNF-Grammatik für Envelope-Antworten, die die Werkzeugschleife
/// bei Bedarf durchreichen kann. Konzept 3.2 verlangt das explizit für
/// Abliterated. Gleiche Struktur wie [`pa_core::tool_loop::TOOL_ENVELOPE_GBNF`],
/// aber hier ist sie beim Adapter der Family verankert, damit sie auch für
/// einzelne Turns außerhalb der Werkzeugschleife greifbar bleibt.
pub const STRICT_JSON_ENVELOPE_GBNF: &str = r#"
root ::= call | answer
call ::= "{\"action\":\"call\",\"tool\":\"" name "\",\"arguments\":" object "}"
answer ::= "{\"action\":\"answer\",\"text\":" string "}"
name ::= "\"" [a-zA-Z_][a-zA-Z0-9_]* "\""
object ::= "{}" | "{" string ":" value ("," string ":" value)* "}"
array ::= "[]" | "[" value ("," value)* "]"
value ::= object | array | string | number | "true" | "false" | "null"
string ::= "\"" ([^"\\] | "\\" ["\\/bfnrt] | "\\u" [0-9a-fA-F]{4})* "\""
number ::= "-"? ("0" | [1-9][0-9]*) ("." [0-9]+)? ([eE][+-]?[0-9]+)?
"#;

/// Formatiert Prompts für Llama-3.2-3B-Abliterated.
pub struct Llama32Adapter {
    alias: String,
}

impl Llama32Adapter {
    pub fn new(alias: impl Into<String>) -> Self {
        Self {
            alias: alias.into(),
        }
    }
}

impl ModelAdapter for Llama32Adapter {
    fn request(&self, messages: &[Message]) -> Result<Value, InferenceError> {
        // Wir setzen das System-Prefix immer als erste System-Nachricht;
        // eventuelle Nutzer-System-Nachrichten kommen danach, damit sie das
        // Prefix nicht ersetzen. Llama-3-Chat-Template lässt das zu.
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
            // Konservativere Sampling-Werte gegen den bekannten Drift.
            "temperature": 0.4,
            "top_p": 0.85,
            "repeat_penalty": 1.1,
            "stop": STOPS,
        }))
    }

    fn stop_sequences(&self) -> &[&str] {
        STOPS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_types::chat::MessageStatus;

    fn msg(role: MessageRole, content: &str) -> Message {
        Message {
            id: role_as_id(role).to_owned(),
            conversation_id: String::new(),
            position: 0,
            role,
            content: content.to_owned(),
            status: MessageStatus::Complete,
            created_at_unix_ms: 0,
        }
    }

    fn role_as_id(role: MessageRole) -> &'static str {
        match role {
            MessageRole::System => "s",
            MessageRole::User => "u",
            MessageRole::Assistant => "a",
        }
    }

    #[test]
    fn injects_system_prefix_and_lowered_temperature() {
        let adapter = Llama32Adapter::new("llama32-3b-abl");
        let request = adapter.request(&[msg(MessageRole::User, "Hallo")]).unwrap();
        let messages = request["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert!(messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("Zweitmodell"));
        assert_eq!(request["temperature"].as_f64().unwrap(), 0.4);
        assert_eq!(request["parse_tool_calls"], false);
    }

    #[test]
    fn stop_sequences_are_llama3_eot_and_eos() {
        let adapter = Llama32Adapter::new("l");
        let stops = adapter.stop_sequences();
        assert!(stops.contains(&"<|eot_id|>"));
        assert!(stops.contains(&"<|end_of_text|>"));
    }

    #[test]
    fn strict_grammar_is_available_as_const() {
        assert!(STRICT_JSON_ENVELOPE_GBNF.contains("root ::= call | answer"));
    }
}
