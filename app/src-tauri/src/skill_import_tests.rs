//! Tests für den Skill-Import per Drag-and-drop (`import_skill_from_path_inner`).
//!
//! Sie laufen gegen einen echten, temporären Tresor und ein echtes, minimales
//! WASM-Modul, damit Policy-Prüfung, Audit und Installation wirklich durchlaufen.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use pa_vault::{
    hot_copy::HotVault,
    key::derive_key,
    meta::{Argon2Parameters, VaultMeta},
};
use sha2::{Digest, Sha256};

use super::{import_skill_from_path_inner, AppError};

/// Minimales Skill-Modul (`pa_alloc`, `pa_skill_invoke` gibt die Eingabe zurück).
const ECHO_WASM: [u8; 149] = [
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0e, 0x02, 0x60, 0x01, 0x7f, 0x01, 0x7f,
    0x60, 0x04, 0x7f, 0x7f, 0x7f, 0x7f, 0x01, 0x7e, 0x03, 0x03, 0x02, 0x00, 0x01, 0x05, 0x03, 0x01,
    0x00, 0x01, 0x06, 0x07, 0x01, 0x7f, 0x01, 0x41, 0x80, 0x08, 0x0b, 0x07, 0x27, 0x03, 0x06, 0x6d,
    0x65, 0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x08, 0x70, 0x61, 0x5f, 0x61, 0x6c, 0x6c, 0x6f, 0x63,
    0x00, 0x00, 0x0f, 0x70, 0x61, 0x5f, 0x73, 0x6b, 0x69, 0x6c, 0x6c, 0x5f, 0x69, 0x6e, 0x76, 0x6f,
    0x6b, 0x65, 0x00, 0x01, 0x0a, 0x20, 0x02, 0x11, 0x01, 0x01, 0x7f, 0x23, 0x00, 0x21, 0x01, 0x23,
    0x00, 0x20, 0x00, 0x6a, 0x24, 0x00, 0x20, 0x01, 0x0b, 0x0c, 0x00, 0x20, 0x02, 0xad, 0x42, 0x20,
    0x86, 0x20, 0x03, 0xad, 0x84, 0x0b, 0x00, 0x1d, 0x04, 0x6e, 0x61, 0x6d, 0x65, 0x02, 0x0d, 0x01,
    0x00, 0x02, 0x00, 0x03, 0x6c, 0x65, 0x6e, 0x01, 0x03, 0x70, 0x74, 0x72, 0x07, 0x07, 0x01, 0x00,
    0x04, 0x68, 0x65, 0x61, 0x70,
];

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    shared: Arc<Mutex<HotVault>>,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("Temp");
    let root = temp.path().join("stick");
    fs::create_dir_all(root.join("AI")).expect("AI");
    let meta = VaultMeta::new(
        [9; 16],
        Argon2Parameters {
            memory_kib: 8 * 1024,
            iterations: 1,
            parallelism: 1,
        },
    );
    let key = derive_key("test", &meta).expect("Schlüssel");
    let vault =
        HotVault::start(&temp.path().join("v.db"), &temp.path().join("hot"), key).expect("Tresor");
    Fixture {
        root,
        shared: Arc::new(Mutex::new(vault)),
        _temp: temp,
    }
}

fn valid_skill(dir: &Path, id: &str) {
    fs::create_dir_all(dir).expect("Skill-Ordner");
    fs::write(dir.join("echo.wasm"), ECHO_WASM).expect("wasm");
    let manifest = format!(
        "[skill]\nid = \"{id}\"\nname = \"Echo\"\nversion = \"1.0.0\"\nruntime = \"wasm\"\nentry = \"echo.wasm\"\nsha256 = \"{}\"\n\n[[tools]]\nname = \"echo\"\ndescription = \"Gibt die Eingabe zurück\"\n",
        hex::encode(Sha256::digest(ECHO_WASM))
    );
    fs::write(dir.join("manifest.toml"), manifest).expect("manifest");
}

fn import(fx: &Fixture, path: &Path) -> Result<crate::InstalledSkillView, AppError> {
    import_skill_from_path_inner(
        path.to_string_lossy().into_owned(),
        fx.root.clone(),
        Arc::clone(&fx.shared),
    )
}

#[test]
fn a_dropped_skill_folder_is_installed() {
    let fx = fixture();
    let source = fx._temp.path().join("quelle").join("echo-skill");
    valid_skill(&source, "echo-test");
    let view = import(&fx, &source).expect("Import");
    assert_eq!(view.id, "echo-test");
    assert!(fx
        .root
        .join("AI/skills/user/echo-test/manifest.toml")
        .is_file());
    assert!(fx.root.join("AI/skills/user/echo-test/echo.wasm").is_file());
}

#[test]
fn dropping_the_manifest_file_itself_works_too() {
    let fx = fixture();
    let source = fx._temp.path().join("quelle").join("ordner");
    valid_skill(&source, "manifest-drop");
    let view = import(&fx, &source.join("manifest.toml")).expect("Import");
    assert_eq!(view.id, "manifest-drop");
}

#[test]
fn a_folder_with_a_spaces_and_umlauts_in_its_path_works() {
    let fx = fixture();
    let source = fx._temp.path().join("Büro Skills").join("mein skill");
    valid_skill(&source, "umlaut-test");
    assert!(import(&fx, &source).is_ok());
}

#[test]
fn a_skill_md_folder_is_installed_as_an_instruction_skill() {
    let fx = fixture();
    let source = fx._temp.path().join("quelle").join("mein-skill");
    fs::create_dir_all(source.join("scripts")).expect("Ordner");
    fs::write(
        source.join("SKILL.md"),
        "---
name: Briefe schreiben
description: Hilft bei Elternbriefen
---

# Briefe
Schreibe freundlich und kurz.
",
    )
    .expect("SKILL.md");
    fs::write(source.join("beispiele.md"), "Beispiel").expect("Beilage");
    fs::write(source.join("scripts").join("run.py"), "print(1)").expect("Skript");
    let view = import(&fx, &source).expect("Import");
    assert_eq!(view.kind, "instructions");
    assert_eq!(view.id, "briefe-schreiben");
    assert_eq!(view.description, "Hilft bei Elternbriefen");
    assert!(view.files.contains(&"beispiele.md".to_owned()));
    // Skripte werden nie übernommen, sondern gemeldet.
    assert!(
        view.skipped.iter().any(|s| s.starts_with("scripts/run.py")),
        "{:?}",
        view.skipped
    );
    let installed = fx.root.join("AI/skills/instructions/briefe-schreiben");
    assert!(installed.join("SKILL.md").is_file());
    assert!(installed.join("beispiele.md").is_file());
    assert!(!installed.join("scripts").exists());
    // Ein zweiter Import mit demselben Namen wird abgewiesen.
    let error = import(&fx, &source).expect_err("doppelt").to_string();
    assert!(error.contains("bereits installiert"), "{error}");
}

#[test]
fn a_skill_md_without_a_body_or_name_is_refused_with_a_reason() {
    let fx = fixture();
    let empty = fx._temp.path().join("leer-skill");
    fs::create_dir_all(&empty).expect("Ordner");
    fs::write(
        empty.join("SKILL.md"),
        "---
name: Nur Kopf
---
",
    )
    .expect("SKILL.md");
    let error = import(&fx, &empty).expect_err("leer").to_string();
    assert!(error.contains("keine Anleitung"), "{error}");
}

#[test]
fn the_parent_folder_of_a_skill_md_skill_is_found_too() {
    let fx = fixture();
    let parent = fx._temp.path().join("sammlung");
    let inner = parent.join("ordnername");
    fs::create_dir_all(&inner).expect("Ordner");
    fs::write(
        inner.join("SKILL.md"),
        "# Tabellen

Arbeite genau.",
    )
    .expect("SKILL.md");
    let view = import(&fx, &parent).expect("Import");
    assert_eq!(view.id, "tabellen");
}

#[test]
fn a_folder_without_any_skill_files_names_what_is_missing() {
    let fx = fixture();
    let source = fx._temp.path().join("leer");
    fs::create_dir_all(&source).expect("Ordner");
    let error = import(&fx, &source)
        .expect_err("muss scheitern")
        .to_string();
    assert!(error.contains("manifest.toml"), "{error}");
    assert!(!error.contains("os error"), "{error}");
}

#[test]
fn a_missing_wasm_file_is_reported_by_name() {
    let fx = fixture();
    let source = fx._temp.path().join("ohne-wasm");
    valid_skill(&source, "ohne-wasm");
    fs::remove_file(source.join("echo.wasm")).expect("löschen");
    let error = import(&fx, &source)
        .expect_err("muss scheitern")
        .to_string();
    assert!(error.contains("echo.wasm"), "{error}");
    assert!(!error.contains("os error"), "{error}");
}

#[test]
fn a_wrong_checksum_and_a_second_install_are_clear_errors() {
    let fx = fixture();
    let source = fx._temp.path().join("pruefsumme");
    valid_skill(&source, "pruefsumme");
    let manifest = fs::read_to_string(source.join("manifest.toml")).expect("lesen");
    let broken = manifest.replace(&hex::encode(Sha256::digest(ECHO_WASM)), &"0".repeat(64));
    fs::write(source.join("manifest.toml"), broken).expect("schreiben");
    let error = import(&fx, &source)
        .expect_err("muss scheitern")
        .to_string();
    assert!(error.contains("SHA-256"), "{error}");

    let good = fx._temp.path().join("gut");
    valid_skill(&good, "doppelt");
    import(&fx, &good).expect("erster Import");
    let error = import(&fx, &good).expect_err("zweiter Import").to_string();
    assert!(error.contains("bereits installiert"), "{error}");
}

#[test]
fn a_skill_that_asks_for_the_network_is_refused_with_a_reason() {
    let fx = fixture();
    let source = fx._temp.path().join("netz");
    valid_skill(&source, "netz");
    let manifest = fs::read_to_string(source.join("manifest.toml")).expect("lesen");
    fs::write(
        source.join("manifest.toml"),
        format!("{manifest}\n[capabilities]\nnetwork = true\n"),
    )
    .expect("schreiben");
    let error = import(&fx, &source)
        .expect_err("muss scheitern")
        .to_string();
    assert!(error.contains("Netzwerk"), "{error}");
}

#[test]
fn dropping_the_parent_folder_finds_the_single_skill_inside() {
    let fx = fixture();
    let parent = fx._temp.path().join("skills-sammlung");
    valid_skill(&parent.join("echo-skill"), "verschachtelt");
    let view = import(&fx, &parent).expect("Import");
    assert_eq!(view.id, "verschachtelt");
}

#[test]
fn several_skills_in_one_dropped_folder_are_not_guessed() {
    let fx = fixture();
    let parent = fx._temp.path().join("mehrere");
    valid_skill(&parent.join("a"), "skill-a");
    valid_skill(&parent.join("b"), "skill-b");
    let error = import(&fx, &parent)
        .expect_err("muss scheitern")
        .to_string();
    assert!(error.contains("mehrere Skills"), "{error}");
}

#[test]
fn only_active_instruction_skills_reach_the_prompt() {
    let fx = fixture();
    let make = |name: &str, text: &str| {
        let dir = fx._temp.path().join(format!("q-{name}"));
        fs::create_dir_all(&dir).expect("Ordner");
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\n---\n{text}"),
        )
        .expect("SKILL.md");
        import(&fx, &dir).expect("Import");
    };
    make("Alpha", "Antworte immer mit Hallo Alpha.");
    make("Beta", "Antworte immer mit Hallo Beta.");
    let mut vault = fx.shared.lock().expect("Tresor");
    // Nichts aktiv: kein Zusatztext.
    assert!(crate::instruction_skills::prompt(vault.repository(), &fx.root, 8192).is_none());
    vault
        .repository_mut()
        .set_setting("skills.instructions.active", r#"["alpha"]"#)
        .expect("setzen");
    let text = crate::instruction_skills::prompt(vault.repository(), &fx.root, 8192).expect("Text");
    assert!(
        text.contains("Hallo Alpha") && !text.contains("Hallo Beta"),
        "{text}"
    );
    assert!(text.contains("Sie ändern keine Rechte"));
    // Ein aktiver, aber gelöschter Skill verschwindet still aus dem Prompt.
    std::fs::remove_dir_all(fx.root.join("AI/skills/instructions/alpha")).expect("löschen");
    let gone = crate::instruction_skills::prompt(vault.repository(), &fx.root, 8192);
    assert!(gone.is_none_or(|t| !t.contains("Hallo Alpha")));
}

#[test]
fn a_skill_chosen_with_slash_applies_to_one_turn_even_when_not_active() {
    let fx = fixture();
    let dir = fx._temp.path().join("q-slash");
    fs::create_dir_all(&dir).expect("Ordner");
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: Alpha\ndescription: Begrüßt\n---\nAntworte immer mit Hallo Alpha.",
    )
    .expect("SKILL.md");
    import(&fx, &dir).expect("Import");

    let mut turn = crate::library_cmds::TurnContext::default();
    let applied = crate::instruction_skills::attach_one(&mut turn, &fx.root, "alpha", 8192)
        .expect("gültige Kennung");
    assert!(applied);
    let prompt = turn.project_prompt.expect("Prompt");
    assert!(
        prompt.contains("Hallo Alpha") && prompt.contains("ausdrücklich"),
        "{prompt}"
    );

    // Unbekannte Kennung ist kein Anleitungs-Skill (etwa ein WASM-Skill).
    let mut other = crate::library_cmds::TurnContext::default();
    assert!(
        !crate::instruction_skills::attach_one(&mut other, &fx.root, "gibtsnicht", 8192)
            .expect("gültige Kennung")
    );
    assert!(other.project_prompt.is_none());

    // Pfadtricks in der Kennung werden abgelehnt.
    let mut bad = crate::library_cmds::TurnContext::default();
    assert!(crate::instruction_skills::attach_one(&mut bad, &fx.root, "../x", 8192).is_err());
}
