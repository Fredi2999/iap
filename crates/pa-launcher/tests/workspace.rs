use std::{fs, path::PathBuf};

fn workspace_manifest() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("Cargo.toml");
    fs::read_to_string(path).expect("workspace Cargo.toml must be readable")
}

#[test]
fn workspace_uses_the_portable_release_profile() {
    let manifest = workspace_manifest();

    assert!(manifest.contains("[profile.release]"));
    assert!(manifest.contains("lto = true"));
    assert!(manifest.contains("codegen-units = 1"));
    assert!(manifest.contains("strip = true"));
    assert!(manifest.contains("panic = \"abort\""));
}

#[test]
fn workspace_packages_share_the_required_baseline() {
    let manifest = workspace_manifest();

    assert!(manifest.contains("edition = \"2021\""));
    assert!(manifest.contains("publish = false"));
    assert!(manifest.contains("members = [\"crates/*\"]"));
}
