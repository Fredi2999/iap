use pa_types::hardware::{
    ApiAvailability, CpuFeature, GpuAdapter, GraphicsApi, HardwareProfile, InstructionSets,
    Measurement,
};

#[test]
fn hardware_profile_round_trips_failed_measurements_without_inventing_values() {
    let profile = HardwareProfile {
        cpu_model: "Synthetic CPU".to_owned(),
        physical_cores: Measurement::Unavailable {
            reason: "probe denied".to_owned(),
            method: "test probe".to_owned(),
        },
        logical_processors: 4,
        total_ram_bytes: 8 * 1024 * 1024 * 1024,
        available_ram_bytes: 5 * 1024 * 1024 * 1024,
        operating_system: "Synthetic Windows".to_owned(),
        instruction_sets: InstructionSets {
            avx2: CpuFeature::new(true, "runtime test"),
            avx512: CpuFeature::new(false, "runtime test"),
            neon: CpuFeature::new(false, "wrong architecture"),
        },
        gpus: Measurement::Measured {
            value: vec![GpuAdapter {
                model: "Synthetic GPU".to_owned(),
                dedicated_vram_bytes: 0,
                shared_system_memory_bytes: 1024,
                software_adapter: false,
            }],
            method: "synthetic DXGI".to_owned(),
        },
        graphics_apis: vec![ApiAvailability {
            api: GraphicsApi::Vulkan,
            available: false,
            method: "loader absent".to_owned(),
        }],
        process_elevated: Measurement::Unavailable {
            reason: "access denied".to_owned(),
            method: "token query".to_owned(),
        },
    };

    let json = serde_json::to_string(&profile).expect("profile must serialize");
    let decoded: HardwareProfile = serde_json::from_str(&json).expect("profile must deserialize");

    assert_eq!(decoded, profile);
}
