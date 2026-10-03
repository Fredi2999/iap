use std::{fs::File, io::Write, path::Path};

use pa_launcher::gguf::inspect_gguf_layers;
use tempfile::TempDir;

fn string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn tensor(bytes: &mut Vec<u8>, name: &str, offset: u64) {
    string(bytes, name);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&offset.to_le_bytes());
}

fn write_synthetic_gguf(path: &Path) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"GGUF");
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&3_u64.to_le_bytes());
    bytes.extend_from_slice(&1_u64.to_le_bytes());
    string(&mut bytes, "general.alignment");
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&32_u32.to_le_bytes());
    tensor(&mut bytes, "blk.0.weight", 0);
    tensor(&mut bytes, "blk.1.weight", 32);
    tensor(&mut bytes, "output.weight", 96);
    while bytes.len() % 32 != 0 {
        bytes.push(0);
    }
    bytes.resize(bytes.len() + 224, 0xA5);

    let mut file = File::create(path).expect("GGUF fixture must be created");
    file.write_all(&bytes)
        .expect("GGUF fixture must be written");
}

#[test]
fn derives_exact_file_spans_in_llama_gpu_offload_order() {
    let temp = TempDir::new().expect("temp directory must be created");
    let path = temp.path().join("tiny.gguf");
    write_synthetic_gguf(&path);

    let layout = inspect_gguf_layers(&path).expect("synthetic GGUF must parse");

    assert_eq!(layout.block_layer_bytes, vec![32, 64]);
    assert_eq!(layout.output_layer_bytes, 128);
    assert_eq!(layout.gpu_offload_order_bytes, vec![64, 32, 128]);
}

#[test]
fn rejects_non_gguf_input_without_allocating_from_untrusted_lengths() {
    let temp = TempDir::new().expect("temp directory must be created");
    let path = temp.path().join("bad.gguf");
    std::fs::write(&path, b"NOPE").expect("bad fixture must be written");

    let error = inspect_gguf_layers(&path).expect_err("bad magic must fail");

    assert!(error.to_string().contains("GGUF"));
}

#[test]
#[ignore = "requires the 3 GB phase-zero model"]
fn inspects_the_real_phase_zero_model_when_requested() {
    let path = std::env::var_os("PA_REAL_GGUF").expect("PA_REAL_GGUF must be set");
    let layout = inspect_gguf_layers(Path::new(&path)).expect("real GGUF must parse");
    println!("{layout:#?}");
    assert_eq!(layout.block_layer_bytes.len(), 35);
}
