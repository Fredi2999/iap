//! Session- und Bootstrap-Zustand des Tauri-Backends.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use pa_launcher::{
    bootstrap::BootstrapContext, inference_backend::LlamaServerEngine, runtime::VaultRuntime,
    tool_runtime::ToolRuntime,
};
use pa_memory::MemoryStore;
use pa_types::{
    ipc::{AvailableModel, BootstrapStatus, ManifestSummary},
    model::{HardwareTier, KvQuantization, ResourcePlan},
};

/// Nach dem `bootstrap`-Aufruf zwischengespeicherte Startinformationen.
///
/// Bewusst ohne die nicht-`Clone`-fähigen Handles aus `BootstrapContext`; nur
/// die Felder, die Kommandos später noch lesen müssen.
pub struct StoredBootstrap {
    pub manifest_summary: ManifestSummary,
    pub hardware: pa_types::hardware::HardwareProfile,
    pub plan: ResourcePlan,
    pub descriptor_id: String,
    pub descriptor_display_name: String,
    pub descriptor_family: String,
    pub descriptor_file_bytes: u64,
    pub descriptor_sha256: String,
    pub descriptor_max_context: u32,
    pub host_identifier: String,
    pub cached_model_path: PathBuf,
    pub server_executable: PathBuf,
    pub package_root: PathBuf,
    pub kv_quantization: KvQuantization,
}

impl StoredBootstrap {
    /// Übernimmt die für die Session wichtigen Felder aus einem
    /// `BootstrapContext`. Der Rest (PackageRoot, ModelCache) kann verworfen
    /// werden, weil er nur für den ersten Startpfad gebraucht wird.
    pub fn from_context(context: BootstrapContext) -> Self {
        Self {
            manifest_summary: context.manifest_summary,
            hardware: context.hardware,
            plan: context.plan.clone(),
            descriptor_id: context.descriptor.id.clone(),
            descriptor_display_name: context.descriptor.display_name.clone(),
            descriptor_family: context.descriptor.family.clone(),
            descriptor_file_bytes: context.descriptor.file_bytes,
            descriptor_sha256: context.descriptor.sha256.clone(),
            descriptor_max_context: context.descriptor.max_context_tokens,
            host_identifier: context.host_identifier,
            cached_model_path: context.cached_model_path,
            server_executable: context.server_executable,
            package_root: context.package.as_path().to_path_buf(),
            kv_quantization: context.plan.kv_quantization,
        }
    }

    /// Konvertiert das gespeicherte Bootstrap in den serialisierbaren IPC-Vertrag.
    pub fn to_status(&self) -> BootstrapStatus {
        let vault_initialized = self.default_vault_path().with_extension("meta").exists();
        BootstrapStatus {
            manifest: self.manifest_summary.clone(),
            hardware: self.hardware.clone(),
            plan: self.plan.clone(),
            default_model: self.to_available_model(),
            warnings: self.plan.warnings.clone(),
            vault_initialized,
        }
    }

    /// Beschreibt das installierte Standardmodell für die UI.
    pub fn to_available_model(&self) -> AvailableModel {
        AvailableModel {
            id: self.descriptor_id.clone(),
            display_name: self.descriptor_display_name.clone(),
            family: self.descriptor_family.clone(),
            gguf_bytes: self.descriptor_file_bytes,
            sha256: self.descriptor_sha256.clone(),
            max_context_tokens: self.descriptor_max_context,
            is_default: true,
        }
    }

    /// Konventioneller Vault-Pfad unterhalb des Paketverzeichnisses.
    pub fn default_vault_path(&self) -> PathBuf {
        self.package_root.join("AI").join("data").join("vault.db")
    }
}

/// Fasst alle nach der Vault-Entsperrung nötigen Handles zusammen.
pub struct Session {
    pub engine: Arc<Mutex<LlamaServerEngine>>,
    /// Hält die angezeigte Modellwahl an den tatsächlich laufenden Server gebunden.
    pub model_id: String,
    pub vault_runtime: Arc<VaultRuntime>,
    pub vault_path: PathBuf,
    pub context_tokens: u32,
    pub kv_quantization: KvQuantization,
    pub tier_override: Option<HardwareTier>,
    pub context_override: Option<u32>,
    /// Werkzeug-Runtime (Policy + Registry + Vault-Audit-Sink); wird erst
    /// mit `--tools`- oder UI-Aktivierung tatsächlich benutzt, aber
    /// vorab beim Öffnen der Session initialisiert, damit das Frontend
    /// den Werkzeug-Bereich sofort anzeigen kann.
    pub tool_runtime: Arc<Mutex<ToolRuntime>>,
    /// Wurzel des Nutzer-Workspace (für Dateien-Bereich und Werkzeuge).
    pub workspace_dir: PathBuf,
    /// Memory-System-Fassade (Fakten, hybrides Retrieval, Faktenextraktion).
    pub memory: Arc<Mutex<MemoryStore>>,
}

impl Session {
    /// Legt eine neue Session mit dem soeben gestarteten Server und geöffneten
    /// Vault an.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: Arc<Mutex<LlamaServerEngine>>,
        model_id: String,
        vault_runtime: Arc<VaultRuntime>,
        vault_path: PathBuf,
        context_tokens: u32,
        kv_quantization: KvQuantization,
        tool_runtime: Arc<Mutex<ToolRuntime>>,
        workspace_dir: PathBuf,
        memory: Arc<Mutex<MemoryStore>>,
    ) -> Self {
        Self {
            engine,
            model_id,
            vault_runtime,
            vault_path,
            context_tokens,
            kv_quantization,
            tier_override: None,
            context_override: None,
            tool_runtime,
            workspace_dir,
            memory,
        }
    }

    /// Stoppt den Inferenzprozess und syncht den Vault deterministisch.
    ///
    /// Verzichtet bewusst auf `unwrap()`: bei nicht-eindeutigem Arc kann der
    /// Vault nicht sauber heruntergefahren werden; der Aufrufer entscheidet.
    pub fn shutdown(self) -> std::io::Result<()> {
        {
            let mut engine = self.engine.lock().map_err(|_| {
                std::io::Error::other("Engine-Mutex vergiftet – Server bleibt aktiv")
            })?;
            let _ = engine.stop();
        }
        match Arc::try_unwrap(self.vault_runtime) {
            Ok(runtime) => runtime.shutdown(),
            Err(_) => Err(std::io::Error::other(
                "VaultRuntime hat noch aktive Referenzen; Rücksync konnte nicht ausgeführt werden",
            )),
        }
    }
}
