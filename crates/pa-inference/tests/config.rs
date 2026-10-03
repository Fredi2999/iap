use std::path::PathBuf;

use pa_inference::config::ServerConfig;
use pa_types::model::KvQuantization;

#[test]
fn pins_offline_loopback_single_slot_and_disables_extra_surfaces() {
    let config = ServerConfig {
        executable: PathBuf::from("llama-server.exe"),
        model: PathBuf::from("model.gguf"),
        model_alias: "gemma-4-e2b-q4-k-m".to_owned(),
        port: 49152,
        api_key: "secret-token".to_owned(),
        context_tokens: 8192,
        threads: 3,
        gpu_layers: 0,
        kv_quantization: KvQuantization::F16,
        mmproj: None,
    };
    let args = config.arguments();
    let text = args
        .iter()
        .map(|a| a.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");

    for required in [
        "--offline",
        "--host 127.0.0.1",
        "--port 49152",
        "--api-key secret-token",
        "--ctx-size 8192",
        "--threads 3",
        "--gpu-layers 0",
        "--cache-type-k f16",
        "--cache-type-v f16",
        "--no-ui",
        "--no-agent",
        "--no-ui-mcp-proxy",
        "--parallel 1",
        "--reasoning auto",
    ] {
        assert!(text.contains(required), "missing {required}: {text}");
    }
    assert!(text.contains("--cache-ram 64"));
    assert!(!text.contains("download"));
    assert!(!text.contains("router"));
}

#[test]
fn generates_a_random_port_and_256_bit_token() {
    let (port, token) = ServerConfig::random_endpoint().expect("endpoint");
    assert_ne!(port, 0);
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn mmproj_flags_only_appear_when_a_projector_is_configured() {
    let mut config = ServerConfig {
        executable: PathBuf::from("llama-server.exe"),
        model: PathBuf::from("model.gguf"),
        model_alias: "gemma".to_owned(),
        port: 49152,
        api_key: "t".to_owned(),
        context_tokens: 8192,
        threads: 3,
        gpu_layers: 0,
        kv_quantization: KvQuantization::F16,
        mmproj: None,
    };
    let joined = |config: &ServerConfig| {
        config
            .arguments()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    };
    assert!(!joined(&config).contains("--mmproj"));
    config.mmproj = Some(PathBuf::from("mmproj.gguf"));
    let text = joined(&config);
    assert!(text.contains("--mmproj mmproj.gguf"));
    assert!(text.contains("--no-mmproj-offload"));
}
