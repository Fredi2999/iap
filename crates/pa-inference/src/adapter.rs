use pa_types::chat::Message;
use serde_json::Value;

use crate::{
    gemma4::Gemma4Adapter, llama32::Llama32Adapter, ministral3::Ministral3Adapter,
    qwen3::Qwen3Adapter, InferenceError,
};

/// Beschränkt Modellunterschiede im MVP auf Requestformat und Stop-Sequenzen.
pub trait ModelAdapter {
    fn request(&self, messages: &[Message]) -> Result<Value, InferenceError>;
    fn stop_sequences(&self) -> &[&str];
}

/// Family-Auswahl ohne dyn-Overhead. Neue Modell-Familien werden hier als
/// zusätzliche Variante angehängt und in [`AdapterKind::from_family`]
/// abgeglichen — die Aufrufer sehen das einheitlich als `ModelAdapter`.
pub enum AdapterKind {
    Gemma4(Gemma4Adapter),
    Llama32(Llama32Adapter),
    Qwen3(Qwen3Adapter),
    Ministral3(Ministral3Adapter),
}

impl AdapterKind {
    /// Baut den passenden Adapter zu einer Family-Kennung (siehe
    /// `<model>.model.toml`, Feld `family`). Fallback: Gemma-4, damit das
    /// Standardmodell nie ohne Adapter dasteht.
    pub fn from_family(family: &str, alias: impl Into<String>) -> Self {
        match family.trim().to_ascii_lowercase().as_str() {
            "llama-3.2" | "llama32" | "llama3.2" => {
                AdapterKind::Llama32(Llama32Adapter::new(alias))
            }
            "qwen3" | "qwen-3" => AdapterKind::Qwen3(Qwen3Adapter::new(alias)),
            "ministral3" | "ministral-3" => AdapterKind::Ministral3(Ministral3Adapter::new(alias)),
            _ => AdapterKind::Gemma4(Gemma4Adapter::new(alias)),
        }
    }
}

impl ModelAdapter for AdapterKind {
    fn request(&self, messages: &[Message]) -> Result<Value, InferenceError> {
        match self {
            AdapterKind::Gemma4(adapter) => adapter.request(messages),
            AdapterKind::Llama32(adapter) => adapter.request(messages),
            AdapterKind::Qwen3(adapter) => adapter.request(messages),
            AdapterKind::Ministral3(adapter) => adapter.request(messages),
        }
    }

    fn stop_sequences(&self) -> &[&str] {
        match self {
            AdapterKind::Gemma4(adapter) => adapter.stop_sequences(),
            AdapterKind::Llama32(adapter) => adapter.stop_sequences(),
            AdapterKind::Qwen3(adapter) => adapter.stop_sequences(),
            AdapterKind::Ministral3(adapter) => adapter.stop_sequences(),
        }
    }
}
