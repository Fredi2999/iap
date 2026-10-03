use serde::{Deserialize, Serialize};

/// Trennt echte Messwerte von nicht verfügbaren Proben, damit die UI nichts schätzt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Measurement<T> {
    Measured { value: T, method: String },
    Unavailable { reason: String, method: String },
}

/// Hält neben dem CPU-Feature auch fest, wie es auf diesem Host erkannt wurde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuFeature {
    pub available: bool,
    pub method: String,
}

impl CpuFeature {
    /// Verhindert, dass ein nackter Bool ohne überprüfbare Erkennungsmethode entsteht.
    pub fn new(available: bool, method: impl Into<String>) -> Self {
        Self {
            available,
            method: method.into(),
        }
    }
}

/// Gruppiert die für die Wahl eines llama.cpp-Binaries relevanten CPU-Features.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionSets {
    pub avx2: CpuFeature,
    pub avx512: CpuFeature,
    pub neon: CpuFeature,
}

/// Enthält nur Speicherangaben, die DXGI selbst für einen Adapter meldet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuAdapter {
    pub model: String,
    pub dedicated_vram_bytes: u64,
    pub shared_system_memory_bytes: u64,
    pub software_adapter: bool,
}

/// Beschränkt API-Namen auf die für das Ressourcenmodell relevanten Backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsApi {
    Vulkan,
    Cuda,
    DirectMl,
    Metal,
}

/// Erklärt einen API-Befund, ohne eine vorhandene DLL mit nutzbarer Leistung gleichzusetzen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiAvailability {
    pub api: GraphicsApi,
    pub available: bool,
    pub method: String,
}

/// Ist der serialisierbare Vertrag zwischen Launcher, Core und späterer UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub cpu_model: String,
    pub physical_cores: Measurement<usize>,
    pub logical_processors: usize,
    pub total_ram_bytes: u64,
    pub available_ram_bytes: u64,
    pub operating_system: String,
    pub instruction_sets: InstructionSets,
    pub gpus: Measurement<Vec<GpuAdapter>>,
    pub graphics_apis: Vec<ApiAvailability>,
    pub process_elevated: Measurement<bool>,
}
