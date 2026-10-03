use pa_launcher::resources::{classify_tier, compute_resource_plan, PlanRequest};
use pa_types::{
    hardware::{
        ApiAvailability, CpuFeature, GpuAdapter, GraphicsApi, HardwareProfile, InstructionSets,
        Measurement,
    },
    model::{HardwareTier, KvCacheProfile, KvQuantization, ModelDescriptor},
};

const GIB: u64 = 1024 * 1024 * 1024;

fn profile(total_gib: u64, free_gib: u64, vulkan: bool, cuda: bool, vram: u64) -> HardwareProfile {
    HardwareProfile {
        cpu_model: "Synthetic CPU".to_owned(),
        physical_cores: Measurement::Measured {
            value: 4,
            method: "fixture".to_owned(),
        },
        logical_processors: 8,
        total_ram_bytes: total_gib * GIB,
        available_ram_bytes: free_gib * GIB,
        operating_system: "Synthetic Windows".to_owned(),
        instruction_sets: InstructionSets {
            avx2: CpuFeature::new(true, "fixture"),
            avx512: CpuFeature::new(false, "fixture"),
            neon: CpuFeature::new(false, "fixture"),
        },
        gpus: Measurement::Measured {
            value: if vulkan || cuda || vram > 0 {
                vec![GpuAdapter {
                    model: "Synthetic GPU".to_owned(),
                    dedicated_vram_bytes: vram,
                    shared_system_memory_bytes: 2 * GIB,
                    software_adapter: false,
                }]
            } else {
                Vec::new()
            },
            method: "fixture".to_owned(),
        },
        graphics_apis: vec![
            ApiAvailability {
                api: GraphicsApi::Vulkan,
                available: vulkan,
                method: "fixture".to_owned(),
            },
            ApiAvailability {
                api: GraphicsApi::Cuda,
                available: cuda,
                method: "fixture".to_owned(),
            },
        ],
        process_elevated: Measurement::Measured {
            value: false,
            method: "fixture".to_owned(),
        },
    }
}

fn model() -> ModelDescriptor {
    ModelDescriptor {
        id: "gemma-4-e2b-q4-k-m".to_owned(),
        display_name: "Gemma 4 E2B Q4_K_M".to_owned(),
        family: "gemma4".to_owned(),
        gguf_file: "gemma.gguf".to_owned(),
        sha256: "0".repeat(64),
        file_bytes: 3_106_738_272,
        max_context_tokens: 131_072,
        measured_peak_rss_bytes_8k: 2_933_035_008,
        kv: vec![KvCacheProfile {
            quantization: KvQuantization::F16,
            fixed_bytes: 12 * 1024 * 1024,
            bytes_per_token: 6 * 1024,
            source: "llama-server b10930 measured".to_owned(),
        }],
        gpu_layer_bytes: vec![100, 200, 300],
        phase0_source: "fixture".to_owned(),
    }
}

#[test]
fn measured_ram_boundaries_classify_t0_and_t1() {
    assert_eq!(
        classify_tier(&profile(7, 5, false, false, 0)),
        HardwareTier::Unsupported
    );
    assert_eq!(
        classify_tier(&profile(8, 5, false, false, 0)),
        HardwareTier::T0
    );
    assert_eq!(
        classify_tier(&profile(15, 8, false, false, 0)),
        HardwareTier::T0
    );
    assert_eq!(
        classify_tier(&profile(16, 9, false, false, 0)),
        HardwareTier::T1
    );
}

#[test]
fn unvalidated_gpu_tiers_require_a_usable_llama_backend() {
    assert_eq!(
        classify_tier(&profile(16, 9, true, false, 0)),
        HardwareTier::T2
    );
    assert_eq!(
        classify_tier(&profile(16, 9, false, false, 8 * GIB)),
        HardwareTier::T1
    );
    assert_eq!(
        classify_tier(&profile(16, 9, false, true, 8 * GIB)),
        HardwareTier::T3
    );
}

#[test]
fn plan_implements_the_concept_formula_and_rounds_context_down() {
    let mut host = profile(8, 3, false, false, 0);
    host.available_ram_bytes = (3 * GIB) + 64 * 1024 * 1024;
    let descriptor = model();

    let plan = compute_resource_plan(
        &host,
        &descriptor,
        PlanRequest {
            tier_override: None,
            context_override: None,
            kv_quantization: KvQuantization::F16,
        },
    )
    .expect("synthetic plan must be computable");

    assert_eq!(plan.tier, HardwareTier::T0);
    assert_eq!(
        plan.available_bytes,
        host.available_ram_bytes - (GIB + GIB / 2)
    );
    assert_eq!(plan.model_budget_bytes, plan.available_bytes * 60 / 100);
    assert_eq!(plan.kv_budget_bytes, plan.available_bytes * 25 / 100);
    assert_eq!(plan.threads, 3);
    assert_eq!(plan.context_tokens, 8192);
}

#[test]
fn low_free_ram_saturates_and_never_claims_a_context() {
    let mut host = profile(8, 1, false, false, 0);
    host.available_ram_bytes = GIB;

    let error = compute_resource_plan(
        &host,
        &model(),
        PlanRequest::for_quantization(KvQuantization::F16),
    )
    .expect_err("reserve larger than free RAM must not produce a fake plan");

    assert!(error.to_string().contains("KV budget"));
}

#[test]
fn context_override_is_clamped_to_safe_model_and_tier_limits() {
    let descriptor = model();
    let plan = compute_resource_plan(
        &profile(16, 12, false, false, 0),
        &descriptor,
        PlanRequest {
            tier_override: None,
            context_override: Some(65_536),
            kv_quantization: KvQuantization::F16,
        },
    )
    .expect("T1 plan must be computable");

    assert_eq!(plan.context_tokens, 32_768);
    assert!(plan.context_was_clamped);
}

#[test]
fn gpu_layer_count_uses_a_strict_eighty_percent_prefix_budget() {
    let mut host = profile(16, 12, false, true, 750);
    if let Measurement::Measured { value, .. } = &mut host.gpus {
        value[0].dedicated_vram_bytes = 750;
    }

    let plan = compute_resource_plan(
        &host,
        &model(),
        PlanRequest::for_quantization(KvQuantization::F16),
    )
    .expect("GPU plan must be computable");

    assert_eq!(plan.gpu_budget_bytes, 600);
    assert_eq!(
        plan.gpu_layers, 2,
        "100 + 200 fits, adding 300 equals the strict limit"
    );
}

#[test]
fn missing_physical_core_measurement_uses_one_safe_thread() {
    let mut host = profile(8, 5, false, false, 0);
    host.physical_cores = Measurement::Unavailable {
        reason: "denied".to_owned(),
        method: "fixture".to_owned(),
    };

    let plan = compute_resource_plan(
        &host,
        &model(),
        PlanRequest::for_quantization(KvQuantization::F16),
    )
    .expect("fallback plan must be computable");

    assert_eq!(plan.threads, 1);
    assert!(plan
        .warnings
        .iter()
        .any(|warning| warning.contains("physical cores")));
}
