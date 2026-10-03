//! Wiederverwendbarer Startpfad: Manifestprüfung → Hardwareprofil →
//! Ressourcenplan → Modell-Cache → Hostprofil.
//!
//! Sowohl der CLI-Launcher als auch die Tauri-App rufen [`prepare`] auf, damit
//! die Reihenfolge und die dokumentierten Werte aus Schritt 3
//! ([docs/mvp-schritt3-cli.md](../../docs/mvp-schritt3-cli.md)) an genau einer
//! Stelle leben.

use std::path::{Path, PathBuf};

use pa_types::{
    hardware::HardwareProfile,
    ipc::ManifestSummary,
    model::{HardwareTier, KvQuantization, ModelDescriptor, ResourcePlan},
};

use crate::{
    cache::{CacheResult, CopyObserver, ModelCache},
    hardware::probe_hardware,
    host_profile::{host_id, save_host_profile},
    manifest::verify_manifest,
    model_config::load_model_descriptor,
    paths::PackageRoot,
    resources::{compute_resource_plan, PlanRequest},
    LauncherError,
};

/// Pfad des installierten Standardmodells relativ zum Paketroot.
pub const DEFAULT_MODEL_DESCRIPTOR_PATH: &str = "AI/models/gemma-4-e2b-q4-k-m.model.toml";
/// Pfad des SHA-256-Manifests relativ zum Paketroot.
pub const DEFAULT_MANIFEST_PATH: &str = "AI/bin/manifest.json";
/// Windows-spezifischer Pfad des llama.cpp-Serverbinaries relativ zum Paketroot.
pub const DEFAULT_SERVER_PATH: &str = "AI/bin/win-x64/llama-server.exe";

/// Alle Startoptionen, die vom Nutzer/UI vor dem `prepare`-Aufruf kommen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapOptions {
    pub tier_override: Option<HardwareTier>,
    pub context_override: Option<u32>,
    pub kv_quantization: KvQuantization,
}

impl Default for BootstrapOptions {
    fn default() -> Self {
        Self {
            tier_override: None,
            context_override: None,
            kv_quantization: KvQuantization::F16,
        }
    }
}

/// Ergebnis eines vollständig ausgeführten Startpfades.
pub struct BootstrapContext {
    pub package: PackageRoot,
    pub manifest_summary: ManifestSummary,
    pub hardware: HardwareProfile,
    pub descriptor: ModelDescriptor,
    pub plan: ResourcePlan,
    pub cache: ModelCache,
    pub host_identifier: String,
    pub cache_result: CacheResult,
    pub cached_model_path: PathBuf,
    pub server_executable: PathBuf,
}

/// Führt den in Schritt 3 dokumentierten Startpfad einmal aus.
///
/// `progress` sieht Modellkopierfortschritt beim ersten Start eines neuen
/// Hosts; danach wird die geprüfte SSD-Kopie wiederverwendet.
pub fn prepare(
    root: &Path,
    options: BootstrapOptions,
    progress: &mut dyn CopyObserver,
) -> Result<BootstrapContext, LauncherError> {
    let package = PackageRoot::new(root)?;
    let manifest_path = Path::new(DEFAULT_MANIFEST_PATH);
    let verification = verify_manifest(&package, manifest_path)?;
    let manifest_summary = ManifestSummary {
        version: verification.version,
        verified_files: u32::try_from(verification.verified_files).unwrap_or(u32::MAX),
        verified_bytes: verification.verified_bytes,
        embedding_model_id: verification.embedding_model_id,
    };

    let hardware = probe_hardware();
    let descriptor = load_model_descriptor(&package, Path::new(DEFAULT_MODEL_DESCRIPTOR_PATH))?;
    let plan = compute_resource_plan(
        &hardware,
        &descriptor,
        PlanRequest {
            tier_override: options.tier_override,
            context_override: options.context_override,
            kv_quantization: options.kv_quantization,
        },
    )?;

    let cache = ModelCache::for_current_host()?;
    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unavailable".to_owned());
    let host_identifier = host_id(&hostname, &hardware);
    save_host_profile(cache.root(), &host_identifier, &hardware)?;

    let source_model = package
        .as_path()
        .join("AI")
        .join("models")
        .join(&descriptor.gguf_file);
    let source_model = source_model
        .canonicalize()
        .map_err(|source| LauncherError::Io {
            action: "canonicalize",
            path: source_model,
            source,
        })?;
    let cache_result = cache.ensure_cached(
        &source_model,
        &descriptor.sha256,
        descriptor.file_bytes,
        progress,
    )?;
    let cached_model_path = cache_result.path().to_path_buf();
    let server_executable = package.resolve_existing(Path::new(DEFAULT_SERVER_PATH))?;

    Ok(BootstrapContext {
        package,
        manifest_summary,
        hardware,
        descriptor,
        plan,
        cache,
        host_identifier,
        cache_result,
        cached_model_path,
        server_executable,
    })
}
