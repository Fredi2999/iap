use std::{fs, time::SystemTime};

use pa_launcher::{
    cache::{CacheResult, CopyObserver, ModelCache},
    host_profile::{host_id, load_current_host_profile, save_host_profile},
    LauncherError,
};
use pa_types::hardware::{CpuFeature, HardwareProfile, InstructionSets, Measurement};
use sha2::{Digest, Sha256};

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

struct NeverAbort;

impl CopyObserver for NeverAbort {
    fn copied(&mut self, _bytes: u64, _total: u64) -> Result<(), LauncherError> {
        Ok(())
    }
}

struct AbortAfter(u64);

impl CopyObserver for AbortAfter {
    fn copied(&mut self, bytes: u64, _total: u64) -> Result<(), LauncherError> {
        if bytes >= self.0 {
            Err(LauncherError::CopyAborted)
        } else {
            Ok(())
        }
    }
}

#[test]
fn copies_once_and_reuses_only_a_valid_cache_entry() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source = temp.path().join("model.gguf");
    let bytes = vec![7_u8; 2 * 1024 * 1024];
    fs::write(&source, &bytes).expect("source");
    let expected_hash = hash(&bytes);
    let cache = ModelCache::new(temp.path().join("cache"));

    let first = cache
        .ensure_cached(&source, &expected_hash, bytes.len() as u64, &mut NeverAbort)
        .expect("first copy");
    assert!(matches!(first, CacheResult::Copied(_)));
    let path = first.path().to_owned();
    let modified = path
        .metadata()
        .and_then(|metadata| metadata.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let second = cache
        .ensure_cached(&source, &expected_hash, bytes.len() as u64, &mut NeverAbort)
        .expect("cache hit");
    assert!(matches!(second, CacheResult::Reused(_)));
    assert_eq!(second.path(), path);
    assert_eq!(
        path.metadata().and_then(|m| m.modified()).ok(),
        Some(modified)
    );

    fs::write(&path, b"manipulated").expect("damage cache");
    let repaired = cache
        .ensure_cached(&source, &expected_hash, bytes.len() as u64, &mut NeverAbort)
        .expect("repair");
    assert!(matches!(repaired, CacheResult::Copied(_)));
    assert_eq!(fs::read(repaired.path()).expect("cached bytes"), bytes);
}

#[test]
fn interrupted_partial_copy_is_never_published() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source = temp.path().join("model.gguf");
    let bytes = vec![3_u8; 3 * 1024 * 1024];
    fs::write(&source, &bytes).expect("source");
    let expected_hash = hash(&bytes);
    let cache = ModelCache::new(temp.path().join("cache"));

    let error = cache
        .ensure_cached(
            &source,
            &expected_hash,
            bytes.len() as u64,
            &mut AbortAfter(1024 * 1024),
        )
        .expect_err("copy must abort");
    assert!(matches!(error, LauncherError::CopyAborted));
    assert!(!cache.entry_path(&expected_hash).exists());

    let completed = cache
        .ensure_cached(&source, &expected_hash, bytes.len() as u64, &mut NeverAbort)
        .expect("retry");
    assert!(matches!(completed, CacheResult::Copied(_)));
}

#[test]
fn cache_identity_depends_on_content_not_source_path() {
    let temp = tempfile::tempdir().expect("temp dir");
    let first_source = temp.path().join("drive-d.gguf");
    let second_source = temp.path().join("drive-e.gguf");
    let bytes = b"same model bytes";
    fs::write(&first_source, bytes).expect("first");
    fs::write(&second_source, bytes).expect("second");
    let expected_hash = hash(bytes);
    let cache = ModelCache::new(temp.path().join("cache"));

    cache
        .ensure_cached(
            &first_source,
            &expected_hash,
            bytes.len() as u64,
            &mut NeverAbort,
        )
        .expect("copy");
    let second = cache
        .ensure_cached(
            &second_source,
            &expected_hash,
            bytes.len() as u64,
            &mut NeverAbort,
        )
        .expect("reuse");

    assert!(matches!(second, CacheResult::Reused(_)));
}

#[test]
fn host_profile_load_uses_current_free_ram() {
    let temp = tempfile::tempdir().expect("temp dir");
    let saved = profile(16, 4);
    save_host_profile(temp.path(), "test-host", &saved).expect("save");
    let current = profile(16, 11);

    let loaded = load_current_host_profile(temp.path(), "test-host", &current)
        .expect("load")
        .expect("profile");

    assert_eq!(loaded.available_ram_bytes, 11 * 1024 * 1024 * 1024);
    assert_eq!(loaded.total_ram_bytes, saved.total_ram_bytes);
    let id_a = host_id("host", &saved);
    let id_b = host_id("host", &saved);
    assert_eq!(id_a, id_b);
    assert_ne!(id_a, host_id("other-host", &saved));
    assert!(temp.path().join("hosts/test-host.json").exists());
    assert!(!temp.path().join("hosts/test-host.json.partial").exists());
}

fn profile(total_gib: u64, available_gib: u64) -> HardwareProfile {
    HardwareProfile {
        cpu_model: "Test CPU".to_owned(),
        physical_cores: Measurement::Measured {
            value: 4,
            method: "test".to_owned(),
        },
        logical_processors: 8,
        total_ram_bytes: total_gib * 1024 * 1024 * 1024,
        available_ram_bytes: available_gib * 1024 * 1024 * 1024,
        operating_system: "Windows Test".to_owned(),
        instruction_sets: InstructionSets {
            avx2: CpuFeature::new(true, "test"),
            avx512: CpuFeature::new(false, "test"),
            neon: CpuFeature::new(false, "test"),
        },
        gpus: Measurement::Measured {
            value: Vec::new(),
            method: "test".to_owned(),
        },
        graphics_apis: Vec::new(),
        process_elevated: Measurement::Measured {
            value: false,
            method: "test".to_owned(),
        },
    }
}
