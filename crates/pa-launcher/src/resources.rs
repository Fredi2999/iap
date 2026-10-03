use pa_types::{
    hardware::{GraphicsApi, HardwareProfile, Measurement},
    model::{HardwareTier, KvQuantization, ModelDescriptor, ResourcePlan},
};

use crate::LauncherError;

const GIB: u64 = 1024 * 1024 * 1024;
const OS_RESERVE_BYTES: u64 = GIB + GIB / 2;
const SUPPORTED_CONTEXTS: [u32; 7] = [2048, 4096, 8192, 16_384, 32_768, 65_536, 131_072];

/// Beschreibt ausschließlich Nutzerentscheidungen; alle Sicherheitsgrenzen werden neu berechnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanRequest {
    pub tier_override: Option<HardwareTier>,
    pub context_override: Option<u32>,
    pub kv_quantization: KvQuantization,
}

impl PlanRequest {
    /// Erzeugt den üblichen automatischen Plan ohne versteckte Overrides.
    pub fn for_quantization(kv_quantization: KvQuantization) -> Self {
        Self {
            tier_override: None,
            context_override: None,
            kv_quantization,
        }
    }
}

/// Wendet zuerst die gemessenen RAM-Grenzen und danach die unvalidierten GPU-Regeln an.
pub fn classify_tier(profile: &HardwareProfile) -> HardwareTier {
    if profile.total_ram_bytes < 8 * GIB {
        return HardwareTier::Unsupported;
    }

    let vulkan = api_available(profile, GraphicsApi::Vulkan);
    let cuda = api_available(profile, GraphicsApi::Cuda);
    let usable_gpu = vulkan || cuda;
    let maximum_vram = maximum_dedicated_vram(profile);
    if usable_gpu && maximum_vram >= 8 * GIB {
        HardwareTier::T3
    } else if profile.total_ram_bytes >= 16 * GIB && vulkan && has_hardware_gpu(profile) {
        HardwareTier::T2
    } else if profile.total_ram_bytes >= 16 * GIB {
        HardwareTier::T1
    } else {
        HardwareTier::T0
    }
}

/// Berechnet alle llama.cpp-Parameter mit Integerarithmetik und sichtbaren Begrenzungen.
pub fn compute_resource_plan(
    profile: &HardwareProfile,
    model: &ModelDescriptor,
    request: PlanRequest,
) -> Result<ResourcePlan, LauncherError> {
    let detected_tier = classify_tier(profile);
    let tier = request.tier_override.unwrap_or(detected_tier);
    if tier == HardwareTier::Unsupported {
        return Err(LauncherError::ResourcePlan(
            "weniger als 8 GiB Gesamt-RAM werden nicht unterstützt".to_owned(),
        ));
    }

    let available_bytes = profile.available_ram_bytes.saturating_sub(OS_RESERVE_BYTES);
    let model_budget_bytes = percentage(available_bytes, 60);
    let kv_budget_bytes = percentage(available_bytes, 25);
    let kv = model
        .kv
        .iter()
        .find(|entry| entry.quantization == request.kv_quantization)
        .ok_or_else(|| {
            LauncherError::ResourcePlan(format!(
                "kein gemessenes KV-Profil für {:?}",
                request.kv_quantization
            ))
        })?;
    if kv.bytes_per_token == 0 || kv_budget_bytes <= kv.fixed_bytes {
        return Err(LauncherError::ResourcePlan(
            "KV budget reicht nach der festen Cache-Allokation für keinen Kontext".to_owned(),
        ));
    }
    let raw_context = (kv_budget_bytes - kv.fixed_bytes) / kv.bytes_per_token;
    let safe_context = SUPPORTED_CONTEXTS
        .iter()
        .copied()
        .filter(|candidate| u64::from(*candidate) <= raw_context)
        .max()
        .ok_or_else(|| {
            LauncherError::ResourcePlan(
                "KV budget reicht nicht für den kleinsten Kontext von 2048 Token".to_owned(),
            )
        })?;
    let (default_context, tier_maximum) = tier_context_limits(tier);
    let requested_context = request.context_override.unwrap_or(default_context);
    let context_tokens = requested_context
        .min(safe_context)
        .min(tier_maximum)
        .min(model.max_context_tokens);

    let mut warnings = Vec::new();
    let threads = match profile.physical_cores {
        Measurement::Measured { value, .. } => value.saturating_sub(1).max(1),
        Measurement::Unavailable { .. } => {
            warnings.push(
                "physical cores unavailable; using one conservative inference thread".to_owned(),
            );
            1
        }
    };
    if model.measured_peak_rss_bytes_8k > model_budget_bytes {
        warnings.push(format!(
            "measured 8K peak RSS {} exceeds the model budget {}",
            model.measured_peak_rss_bytes_8k, model_budget_bytes
        ));
    }
    if matches!(tier, HardwareTier::T2 | HardwareTier::T3) {
        warnings.push("T2/T3 boundary is formula-based and not hardware-validated".to_owned());
    }

    let gpu_budget_bytes = if api_available(profile, GraphicsApi::Vulkan)
        || api_available(profile, GraphicsApi::Cuda)
    {
        percentage(maximum_dedicated_vram(profile), 80)
    } else {
        0
    };
    let gpu_layers = strict_prefix_count(&model.gpu_layer_bytes, gpu_budget_bytes);

    Ok(ResourcePlan {
        tier,
        tier_hardware_validated: matches!(tier, HardwareTier::T0 | HardwareTier::T1),
        available_bytes,
        model_budget_bytes,
        kv_budget_bytes,
        gpu_budget_bytes,
        context_tokens,
        context_was_clamped: context_tokens != requested_context,
        threads,
        gpu_layers,
        kv_quantization: request.kv_quantization,
        warnings,
    })
}

fn percentage(value: u64, percent: u64) -> u64 {
    ((u128::from(value) * u128::from(percent)) / 100) as u64
}

fn tier_context_limits(tier: HardwareTier) -> (u32, u32) {
    match tier {
        HardwareTier::Unsupported => (0, 0),
        HardwareTier::T0 => (8192, 8192),
        HardwareTier::T1 => (16_384, 32_768),
        HardwareTier::T2 => (32_768, 32_768),
        HardwareTier::T3 => (32_768, 65_536),
    }
}

fn api_available(profile: &HardwareProfile, api: GraphicsApi) -> bool {
    profile
        .graphics_apis
        .iter()
        .any(|entry| entry.api == api && entry.available)
}

fn has_hardware_gpu(profile: &HardwareProfile) -> bool {
    matches!(
        &profile.gpus,
        Measurement::Measured { value, .. }
            if value.iter().any(|adapter| !adapter.software_adapter)
    )
}

fn maximum_dedicated_vram(profile: &HardwareProfile) -> u64 {
    match &profile.gpus {
        Measurement::Measured { value, .. } => value
            .iter()
            .filter(|adapter| !adapter.software_adapter)
            .map(|adapter| adapter.dedicated_vram_bytes)
            .max()
            .unwrap_or(0),
        Measurement::Unavailable { .. } => 0,
    }
}

fn strict_prefix_count(layer_bytes: &[u64], budget: u64) -> usize {
    let mut used = 0_u64;
    let mut count = 0_usize;
    for layer in layer_bytes {
        let Some(candidate) = used.checked_add(*layer) else {
            break;
        };
        if candidate >= budget {
            break;
        }
        used = candidate;
        count += 1;
    }
    count
}
