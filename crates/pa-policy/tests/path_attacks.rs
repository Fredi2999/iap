//! Angriffstests für die zentrale Pfadnormalisierung.
//!
//! Diese Tests wurden VOR der Implementierung geschrieben und beschreiben
//! die Bedrohungen aus Konzept 10.2. Wenn hier auch nur ein Test fällt, ist
//! ein Verzeichnisausbruch möglich.

use std::{fs, path::PathBuf};

use pa_policy::{normalize, safe_join, PathScope, PolicyError};

fn workspace_with_file(prefix: &str) -> (tempfile::TempDir, PathScope, PathBuf) {
    let temp = tempfile::TempDir::with_prefix(prefix).expect("tempdir");
    let inside = temp.path().join("notes.txt");
    fs::write(&inside, "harmlos").expect("seed file");
    let scope = PathScope::new(temp.path()).expect("scope");
    (temp, scope, inside)
}

#[test]
fn parent_dir_traversal_is_blocked() {
    let (_temp, scope, _) = workspace_with_file("pa-policy-parent");
    // Der Backslash trennt nur unter Windows Pfadteile; unter Linux ist das ein Dateiname.
    #[cfg(windows)]
    {
        let attempt = PathBuf::from("..\\etc\\passwd");
        let err = safe_join(&scope, &attempt).expect_err("parent traversal must fail");
        assert!(matches!(err, PolicyError::PathTraversal { .. }));
    }

    let mixed = PathBuf::from("subdir/../../..");
    let err = safe_join(&scope, &mixed).expect_err("mixed separators must fail");
    assert!(matches!(err, PolicyError::PathTraversal { .. }));
}

#[test]
fn absolute_paths_are_rejected() {
    let (_temp, scope, _) = workspace_with_file("pa-policy-abs");
    let attempt = PathBuf::from(if cfg!(windows) {
        "C:\\Windows\\System32"
    } else {
        "/etc/passwd"
    });
    let err = safe_join(&scope, &attempt).expect_err("absolute path must fail");
    assert!(matches!(err, PolicyError::PathTraversal { .. }));
}

#[test]
fn nul_bytes_and_control_chars_are_rejected() {
    let (_temp, scope, _) = workspace_with_file("pa-policy-nul");
    let with_nul = PathBuf::from("no\0break.txt");
    let err = safe_join(&scope, &with_nul).expect_err("NUL byte must fail");
    assert!(matches!(err, PolicyError::PathTraversal { .. }));

    let with_ctrl = PathBuf::from("bell\u{0007}.txt");
    let err = safe_join(&scope, &with_ctrl).expect_err("control char must fail");
    assert!(matches!(err, PolicyError::PathTraversal { .. }));
}

#[test]
fn trailing_dot_or_space_is_rejected() {
    assert!(matches!(
        normalize(&PathBuf::from("harmlos.txt.")),
        Err(PolicyError::PathTraversal { .. })
    ));
    assert!(matches!(
        normalize(&PathBuf::from("harmlos ")),
        Err(PolicyError::PathTraversal { .. })
    ));
}

#[test]
fn windows_reserved_names_are_rejected_even_with_extension() {
    for name in [
        "CON",
        "nul",
        "COM1.txt",
        "lpT9.log",
        "AUX",
        "PRN.dat",
        "workspace/CON.md",
    ] {
        let err = normalize(&PathBuf::from(name)).expect_err(name);
        assert!(
            matches!(err, PolicyError::ReservedName { .. }),
            "erwartete ReservedName für `{name}`, bekam {err:?}"
        );
    }
}

#[test]
fn empty_relative_path_is_rejected() {
    let (_temp, scope, _) = workspace_with_file("pa-policy-empty");
    let err = safe_join(&scope, &PathBuf::from("")).expect_err("empty must fail");
    assert!(matches!(err, PolicyError::PathTraversal { .. }));
}

#[test]
fn safe_join_returns_canonical_within_scope() {
    let (_temp, scope, seeded) = workspace_with_file("pa-policy-ok");
    let joined = safe_join(&scope, &PathBuf::from("notes.txt")).expect("valid");
    let canonical_expected = std::fs::canonicalize(&seeded).unwrap();
    assert_eq!(joined, canonical_expected);
}

#[test]
fn safe_join_allows_new_files_via_lenient_canonicalize() {
    let (_temp, scope, _) = workspace_with_file("pa-policy-new");
    let joined = safe_join(&scope, &PathBuf::from("neu.txt")).expect("new file allowed");
    assert!(joined.ends_with("neu.txt"));
    assert!(joined.starts_with(scope.root()));
}

#[cfg(windows)]
#[test]
fn junction_pointing_outside_workspace_is_rejected() {
    // Ein Angreifer legt innerhalb des Workspaces eine Junction an, die auf
    // C:\Windows zeigt. Die Policy muss beim `safe_join` merken, dass der
    // kanonische Pfad außerhalb der Wurzel liegt.
    let temp = tempfile::TempDir::with_prefix("pa-policy-jct").expect("tempdir");
    let scope = PathScope::new(temp.path()).expect("scope");
    let junction_dir = temp.path().join("shortcut");
    let target = std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .expect("WINDIR env");
    junction::create(&target, &junction_dir).expect("junction");

    let err = safe_join(&scope, &PathBuf::from("shortcut\\notepad.exe"))
        .expect_err("junction outside must fail");
    assert!(
        matches!(err, PolicyError::PathOutOfScope { .. }),
        "erwartete PathOutOfScope, bekam {err:?}"
    );
}

#[cfg(unix)]
#[test]
fn symlink_pointing_outside_workspace_is_rejected() {
    let temp = tempfile::TempDir::with_prefix("pa-policy-sym").expect("tempdir");
    let scope = PathScope::new(temp.path()).expect("scope");
    let link = temp.path().join("shortcut");
    std::os::unix::fs::symlink("/etc", &link).expect("symlink");
    let err =
        safe_join(&scope, &PathBuf::from("shortcut/passwd")).expect_err("symlink escape must fail");
    assert!(matches!(err, PolicyError::PathOutOfScope { .. }));
}

#[cfg(windows)]
#[test]
fn case_insensitive_root_matching_holds() {
    // Auf Windows ist Groß-/Kleinschreibung im Dateisystem meist irrelevant.
    // Nach `canonicalize` sollte die Schreibweise übereinstimmen; als Fallback
    // vergleicht die Policy zusätzlich case-insensitiv. Wir simulieren einen
    // absichtlich anders geschriebenen Root und stellen sicher, dass ein
    // valider innerhalb liegender Pfad akzeptiert wird.
    let temp = tempfile::TempDir::with_prefix("pa-policy-case").expect("tempdir");
    fs::write(temp.path().join("ok.txt"), "").unwrap();
    // Kanonisch normalisierter Root
    let canonical = std::fs::canonicalize(temp.path()).unwrap();
    // Ändere zusätzlich die Schreibweise, um den Vergleich zu strapazieren:
    let messed_up = PathBuf::from(canonical.to_string_lossy().to_uppercase());
    let scope = PathScope::from_canonical(messed_up);
    let joined = safe_join(&scope, &PathBuf::from("ok.txt")).expect("mixed case root allowed");
    assert!(joined.exists());
}

#[test]
fn absolute_resolution_allows_deep_new_targets_inside_scope() {
    let (temp, scope, _) = workspace_with_file("pa-policy-abs-ok");
    let target = temp.path().join("wt").join("task").join("k1");
    let resolved = pa_policy::resolve_absolute_in_scope(&scope, &target).expect("inside");
    assert!(resolved.starts_with(scope.root()));
    assert!(resolved.ends_with("k1"));
}

#[test]
fn absolute_resolution_rejects_outside_relative_and_traversal() {
    let (_temp, scope, _) = workspace_with_file("pa-policy-abs-bad");
    let other = tempfile::TempDir::with_prefix("pa-policy-abs-other").expect("other");
    assert!(matches!(
        pa_policy::resolve_absolute_in_scope(&scope, &other.path().join("x")),
        Err(PolicyError::PathOutOfScope { .. })
    ));
    assert!(
        pa_policy::resolve_absolute_in_scope(&scope, std::path::Path::new("notes.txt")).is_err()
    );
    let traversal = scope.root().join("a").join("..").join("..").join("x");
    assert!(pa_policy::resolve_absolute_in_scope(&scope, &traversal).is_err());
}

#[cfg(windows)]
#[test]
fn absolute_resolution_rejects_junction_escape() {
    let temp = tempfile::TempDir::with_prefix("pa-policy-abs-junction").expect("temp");
    let outside = tempfile::TempDir::with_prefix("pa-policy-abs-target").expect("outside");
    let scope = PathScope::new(temp.path()).expect("scope");
    let link = temp.path().join("shortcut");
    junction::create(outside.path(), &link).expect("junction");
    let err = pa_policy::resolve_absolute_in_scope(&scope, &link.join("neu").join("datei"))
        .expect_err("escape must fail");
    assert!(matches!(err, PolicyError::PathOutOfScope { .. }));
}
