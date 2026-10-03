use std::{fs, path::Path};

use pa_launcher::{model_config::load_model_descriptor, paths::PackageRoot};
use pa_types::model::KvQuantization;

#[test]
fn loads_the_measured_default_model_descriptor() {
    let package = PackageRoot::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .as_path(),
    )
    .expect("workspace root");

    let descriptor = load_model_descriptor(
        &package,
        Path::new("AI/models/gemma-4-e2b-q4-k-m.model.toml"),
    )
    .expect("measured descriptor");

    assert_eq!(descriptor.id, "gemma-4-e2b-q4-k-m");
    assert_eq!(descriptor.file_bytes, 3_106_738_272);
    assert_eq!(descriptor.measured_peak_rss_bytes_8k, 2_933_035_008);
    assert_eq!(descriptor.gpu_layer_bytes.len(), 35);
    let f16 = descriptor
        .kv
        .iter()
        .find(|entry| entry.quantization == KvQuantization::F16)
        .expect("f16 profile");
    assert_eq!(f16.fixed_bytes, 12_582_912);
    assert_eq!(f16.bytes_per_token, 6_144);
}

#[test]
fn loads_the_bundled_second_model_descriptor() {
    let package = PackageRoot::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .as_path(),
    )
    .expect("workspace root");

    let descriptor =
        load_model_descriptor(&package, Path::new("AI/models/llama32-3b-abl.model.toml"))
            .expect("bundled second model must be selectable");

    assert_eq!(descriptor.id, "llama32-3b-abl-q4-k-m");
    assert_eq!(descriptor.measured_peak_rss_bytes_8k, 4_401_922_048);
    let f16 = descriptor
        .kv
        .iter()
        .find(|entry| entry.quantization == KvQuantization::F16)
        .expect("f16 profile");
    assert_eq!(f16.fixed_bytes, 0);
    assert_eq!(f16.bytes_per_token, 114_688);
    let q8 = descriptor
        .kv
        .iter()
        .find(|entry| entry.quantization == KvQuantization::Q8_0)
        .expect("q8_0 profile");
    assert_eq!(q8.fixed_bytes, 0);
    assert_eq!(q8.bytes_per_token, 60_928);
}

#[test]
fn loads_qwen3_install_descriptor() {
    let package = PackageRoot::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .as_path(),
    )
    .expect("workspace root");
    let descriptor = load_model_descriptor(
        &package,
        Path::new("AI/models/qwen3-4b-instruct-2507-q4-k-m.model.toml"),
    )
    .expect("Qwen3 descriptor");
    assert_eq!(descriptor.family, "qwen3");
    assert_eq!(descriptor.file_bytes, 2_497_280_736);
    assert_eq!(descriptor.gpu_layer_bytes.len(), 36);
    assert_eq!(descriptor.kv.len(), 2);
}

#[test]
fn loads_qwen25_coder_install_descriptor_with_its_slowness_hint() {
    let package = PackageRoot::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .as_path(),
    )
    .expect("workspace root");
    let descriptor = load_model_descriptor(
        &package,
        Path::new("AI/models/qwen25-coder-7b-abl-q5-k-m.model.toml"),
    )
    .expect("Qwen2.5-Coder descriptor");
    // `qwen3` ist hier der Adapter-Schlüssel für das gemeinsame ChatML-Format.
    assert_eq!(descriptor.family, "qwen3");
    assert_eq!(descriptor.file_bytes, 5_444_832_096);
    assert_eq!(descriptor.max_context_tokens, 32_768);
    // 28 Blöcke plus die getrennte Ausgabeschicht des 7B-Modells.
    assert_eq!(descriptor.gpu_layer_bytes.len(), 29);
    assert_eq!(descriptor.kv.len(), 2);
    // Der Hinweis steht im Namen, damit er in jeder Modellliste erscheint.
    assert!(
        descriptor.display_name.contains("Langsamer"),
        "der Langsamkeits-Hinweis darf nicht aus dem Anzeigenamen verschwinden"
    );
}

#[test]
fn loads_ministral3_install_descriptor() {
    let package = PackageRoot::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .as_path(),
    )
    .expect("workspace root");
    let descriptor = load_model_descriptor(
        &package,
        Path::new("AI/models/ministral3-3b-instruct-2512-q4-k-m.model.toml"),
    )
    .expect("Ministral 3 descriptor");
    assert_eq!(descriptor.family, "ministral3");
    assert_eq!(descriptor.file_bytes, 2_147_023_008);
    assert!(!descriptor.gpu_layer_bytes.is_empty());
    assert_eq!(descriptor.kv.len(), 2);
}

#[test]
fn rejects_a_descriptor_with_duplicate_kv_profiles() {
    let temp = tempfile::tempdir().expect("temp dir");
    fs::write(
        temp.path().join("model.toml"),
        r#"
id = "bad"
display_name = "Bad"
family = "gemma4"
gguf_file = "bad.gguf"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
file_bytes = 1
max_context_tokens = 1
measured_peak_rss_bytes_8k = 1
gpu_layer_bytes = [1]
phase0_source = "test"

[[kv]]
quantization = "f16"
fixed_bytes = 1
bytes_per_token = 1
source = "test"

[[kv]]
quantization = "f16"
fixed_bytes = 1
bytes_per_token = 1
source = "test"
"#,
    )
    .expect("fixture");
    let package = PackageRoot::new(temp.path()).expect("package root");

    let error = load_model_descriptor(&package, Path::new("model.toml"))
        .expect_err("duplicate KV profile must fail");

    assert!(error.to_string().contains("doppelt"));
}
