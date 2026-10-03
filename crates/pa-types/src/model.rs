use serde::{Deserialize, Serialize};

/// Trennt gemessene RAM-Tiers von den noch unvalidierten GPU-Tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareTier {
    Unsupported,
    T0,
    T1,
    T2,
    T3,
}

/// Entspricht exakt den von der gepinnten llama.cpp-Version akzeptierten MVP-Typen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KvQuantization {
    F16,
    Q8_0,
}

/// Hält gemessene lineare und fixe KV-Kosten getrennt, weil Gemma SWA verwendet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KvCacheProfile {
    pub quantization: KvQuantization,
    pub fixed_bytes: u64,
    pub bytes_per_token: u64,
    pub source: String,
}

/// Ist die versionierte Brücke zwischen einer konkreten GGUF-Datei und Ressourcenplanung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDescriptor {
    pub id: String,
    pub display_name: String,
    pub family: String,
    pub gguf_file: String,
    pub sha256: String,
    pub file_bytes: u64,
    pub max_context_tokens: u32,
    pub measured_peak_rss_bytes_8k: u64,
    pub kv: Vec<KvCacheProfile>,
    /// Reihenfolge entspricht der Reihenfolge, in der `--gpu-layers` Speicher belegt.
    pub gpu_layer_bytes: Vec<u64>,
    pub phase0_source: String,
}

/// Legt die tatsächlich gestarteten Werte samt Herkunft für UI und Diagnose offen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourcePlan {
    pub tier: HardwareTier,
    pub tier_hardware_validated: bool,
    pub available_bytes: u64,
    pub model_budget_bytes: u64,
    pub kv_budget_bytes: u64,
    pub gpu_budget_bytes: u64,
    pub context_tokens: u32,
    pub context_was_clamped: bool,
    pub threads: usize,
    pub gpu_layers: usize,
    pub kv_quantization: KvQuantization,
    pub warnings: Vec<String>,
}
