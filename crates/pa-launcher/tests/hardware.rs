use pa_launcher::hardware::probe_hardware;
use pa_types::hardware::{GraphicsApi, Measurement};

#[test]
fn real_windows_profile_reports_measured_cpu_and_memory() {
    let profile = probe_hardware();

    assert!(!profile.cpu_model.trim().is_empty());
    assert!(profile.logical_processors > 0);
    assert!(profile.total_ram_bytes > 0);
    assert!(profile.available_ram_bytes > 0);
    assert!(profile.available_ram_bytes <= profile.total_ram_bytes);
    assert!(!profile.operating_system.trim().is_empty());
    assert!(matches!(
        profile.physical_cores,
        Measurement::Measured { value, .. } if value > 0
    ));
}

#[test]
fn profile_reports_every_relevant_graphics_api_without_guessing() {
    let profile = probe_hardware();

    for expected in [
        GraphicsApi::Vulkan,
        GraphicsApi::Cuda,
        GraphicsApi::DirectMl,
        GraphicsApi::Metal,
    ] {
        assert!(
            profile
                .graphics_apis
                .iter()
                .any(|entry| entry.api == expected),
            "missing API result for {expected:?}"
        );
    }
}

#[test]
fn windows_profile_never_claims_neon_or_metal() {
    #[cfg_attr(not(windows), allow(unused_variables))]
    let profile = probe_hardware();

    #[cfg(windows)]
    {
        assert!(!profile.instruction_sets.neon.available);
        let metal = profile
            .graphics_apis
            .iter()
            .find(|entry| entry.api == GraphicsApi::Metal)
            .expect("Metal result must exist");
        assert!(!metal.available);
        assert!(metal.method.contains("not a Windows"));
    }
}
