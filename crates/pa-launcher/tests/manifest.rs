use std::{fs, path::Path};

use pa_launcher::{manifest::verify_manifest, paths::PackageRoot, LauncherError};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn write_manifest(root: &Path, path: &str, bytes: u64, hash: &str) {
    let body = json!({
        "version": "test-1",
        "files": [{"path": path, "bytes": bytes, "sha256": hash}],
    });
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&body).expect("fixture JSON must serialize"),
    )
    .expect("fixture manifest must be written");
}

#[test]
fn verifies_the_static_fixture() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let root = PackageRoot::new(&fixture).expect("fixture root must resolve");

    let report = verify_manifest(&root, Path::new("manifest-valid.json"))
        .expect("valid fixture must verify");

    assert_eq!(report.version, "test-1");
    assert_eq!(report.verified_files, 1);
    assert_eq!(report.verified_bytes, 11);
}

#[test]
fn reports_size_and_hash_tampering_separately() {
    let temp = TempDir::new().expect("temp directory must be created");
    fs::write(temp.path().join("payload.bin"), b"correct")
        .expect("fixture payload must be written");
    let root = PackageRoot::new(temp.path()).expect("temp root must resolve");

    write_manifest(temp.path(), "payload.bin", 99, &"0".repeat(64));
    let size_error = verify_manifest(&root, Path::new("manifest.json"))
        .expect_err("wrong size must fail before hashing");
    assert!(matches!(size_error, LauncherError::SizeMismatch { .. }));

    write_manifest(temp.path(), "payload.bin", 7, &"0".repeat(64));
    let hash_error =
        verify_manifest(&root, Path::new("manifest.json")).expect_err("wrong hash must fail");
    assert!(matches!(hash_error, LauncherError::HashMismatch { .. }));
}

#[test]
fn rejects_paths_that_can_leave_the_package_root() {
    let temp = TempDir::new().expect("temp directory must be created");
    let root = PackageRoot::new(temp.path()).expect("temp root must resolve");

    // Laufwerks- und UNC-Schreibweisen sind nur unter Windows Pfade mit Sonderbedeutung.
    let mut unsafe_paths = vec!["../outside.bin", "/absolute.bin"];
    if cfg!(windows) {
        unsafe_paths.extend([r"C:\absolute.bin", r"\\server\share\payload.bin"]);
    }
    for unsafe_path in unsafe_paths {
        write_manifest(temp.path(), unsafe_path, 0, &"0".repeat(64));
        let error =
            verify_manifest(&root, Path::new("manifest.json")).expect_err("unsafe path must fail");
        assert!(
            matches!(error, LauncherError::UnsafePath { .. }),
            "unexpected error for {unsafe_path}: {error}"
        );
    }
}

#[cfg(windows)]
#[test]
fn rejects_a_junction_whose_target_is_outside_the_package_root() {
    let package = TempDir::new().expect("package temp directory must be created");
    let outside = TempDir::new().expect("outside temp directory must be created");
    fs::write(outside.path().join("secret.bin"), b"secret")
        .expect("outside fixture must be written");
    junction::create(outside.path(), package.path().join("link"))
        .expect("NTFS junction must be created without administrator rights");
    write_manifest(package.path(), "link/secret.bin", 6, &"0".repeat(64));
    let root = PackageRoot::new(package.path()).expect("package root must resolve");

    let error = verify_manifest(&root, Path::new("manifest.json"))
        .expect_err("external symlink target must fail");

    assert!(matches!(error, LauncherError::UnsafePath { .. }));
}

#[test]
fn hashes_a_large_file_without_changing_the_contract() {
    let temp = TempDir::new().expect("temp directory must be created");
    let payload = vec![0xA5; 16 * 1024 * 1024];
    fs::write(temp.path().join("large.bin"), &payload).expect("large fixture must be written");
    let expected = hex::encode(Sha256::digest(&payload));
    write_manifest(temp.path(), "large.bin", payload.len() as u64, &expected);
    let root = PackageRoot::new(temp.path()).expect("temp root must resolve");

    let report =
        verify_manifest(&root, Path::new("manifest.json")).expect("large fixture must verify");

    assert_eq!(report.verified_bytes, payload.len() as u64);
}
