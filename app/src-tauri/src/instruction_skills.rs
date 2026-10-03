//! Anleitungs-Skills (`SKILL.md`): Import, Aktivierung, Entfernen und Einbindung in den Prompt.
//!
//! Ein Anleitungs-Skill ist reiner Text. IAP führt nichts daraus aus; aktivierte
//! Skills werden dem Modell als zusätzliche Anweisung mitgegeben (mit Grenzen für
//! kleine Kontexte). Jeder Dateizugriff beim Import läuft durch pa-policy mit Audit.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use pa_launcher::vault_audit::VaultAuditSink;
use pa_skills::instructions::{self, InstructionRegistry, InstructionSkill};
use tauri::State;

use crate::{
    checked_skill_path, ensure_bootstrap, random_id, require_session, AppError, AppResult,
    AppState, InstalledSkillView,
};

/// Einstellung im Tresor: Kennungen der aktiven Anleitungs-Skills (JSON-Liste).
const SETTING_ACTIVE: &str = "skills.instructions.active";

/// Ordner für Anleitungs-Skills relativ zur Paketwurzel.
fn relative_root() -> PathBuf {
    PathBuf::from("AI").join("skills").join("instructions")
}

/// Registry der Anleitungs-Skills.
pub fn registry(package_root: &Path) -> InstructionRegistry {
    InstructionRegistry::new(package_root.join(relative_root()))
}

fn read_active(repo: &pa_vault::repository::VaultRepository) -> Vec<String> {
    repo.setting(SETTING_ACTIVE)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
        .unwrap_or_default()
}

fn write_active(repo: &mut pa_vault::repository::VaultRepository, ids: &[String]) -> AppResult<()> {
    let text = serde_json::to_string(ids).map_err(|e| AppError::Internal(e.to_string()))?;
    repo.set_setting(SETTING_ACTIVE, &text)?;
    Ok(())
}

/// Sicht für die Oberfläche.
pub fn view(skill: &InstructionSkill, active: bool, skipped: Vec<String>) -> InstalledSkillView {
    InstalledSkillView {
        id: skill.id.clone(),
        name: skill.name.clone(),
        version: String::new(),
        permissions: Vec::new(),
        tools: Vec::new(),
        kind: "instructions".to_owned(),
        description: skill.description.clone(),
        active,
        body_preview: skill.body.chars().take(4_000).collect(),
        files: skill.files.clone(),
        skipped,
    }
}

/// Installiert einen Anleitungs-Skill aus `source_dir` (enthält eine `SKILL.md`).
///
/// # Errors
/// Verständliche Meldung, wenn Datei, Name oder Ziel nicht passen.
pub fn import(
    source_dir: &Path,
    package_root: &Path,
    audit: &mut VaultAuditSink,
) -> AppResult<(InstalledSkillView, String)> {
    use pa_policy::{CapabilityAction as Action, Mode, PathScope};
    let invalid = |text: String| AppError::Invalid(text);
    let source_scope = PathScope::new(source_dir).map_err(|e| invalid(e.to_string()))?;
    let survey = instructions::survey(source_dir)
        .map_err(|e| invalid(format!("Der Ordner ist nicht lesbar: {e}")))?;
    let Some(skill_md) = survey.keep.first().cloned() else {
        return Err(invalid(
            "In diesem Ordner fehlt die Datei SKILL.md.".to_owned(),
        ));
    };
    let md_path = checked_skill_path(
        &source_scope,
        Path::new(&skill_md),
        Action::FileRead,
        Mode::M0Observe,
        audit,
    )?;
    let text = std::fs::read_to_string(md_path)
        .map_err(|_| invalid("Die SKILL.md ist kein lesbarer Text (UTF-8).".to_owned()))?;
    let folder_name = source_dir
        .file_name()
        .map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let parsed = instructions::parse_skill_md(&text, &folder_name).map_err(|e| {
        invalid(format!(
            "{e}. Trage in der SKILL.md einen Namen ein (Zeile „name: …“)."
        ))
    })?;
    if parsed.body.trim().is_empty() {
        return Err(invalid("Die SKILL.md enthält keine Anleitung.".to_owned()));
    }

    let package_scope =
        PathScope::new(package_root).map_err(|e| AppError::Internal(e.to_string()))?;
    for folder in [
        PathBuf::from("AI"),
        PathBuf::from("AI").join("skills"),
        relative_root(),
    ] {
        let path = checked_skill_path(
            &package_scope,
            &folder,
            Action::FileWrite,
            Mode::M1Workspace,
            audit,
        )?;
        std::fs::create_dir_all(path)?;
    }
    let target_relative = relative_root().join(&parsed.id);
    let target = checked_skill_path(
        &package_scope,
        &target_relative,
        Action::FileWrite,
        Mode::M1Workspace,
        audit,
    )?;
    if target.exists() {
        return Err(invalid(format!(
            "Der Skill „{}“ ist bereits installiert.",
            parsed.name
        )));
    }
    let stage_relative = PathBuf::from("AI")
        .join("skills")
        .join(format!(".import-{}", random_id("skill")));
    let stage = checked_skill_path(
        &package_scope,
        &stage_relative,
        Action::FileWrite,
        Mode::M1Workspace,
        audit,
    )?;
    std::fs::create_dir(&stage)?;
    let copy = (|| -> AppResult<()> {
        for relative in &survey.keep {
            let from = checked_skill_path(
                &source_scope,
                Path::new(relative),
                Action::FileRead,
                Mode::M0Observe,
                audit,
            )?;
            let bytes = std::fs::read(from)?;
            // SKILL.md wird unter festem Namen abgelegt, damit die Suche nicht von der Schreibweise abhängt.
            let name = if relative.eq_ignore_ascii_case("SKILL.md") {
                "SKILL.md"
            } else {
                relative.as_str()
            };
            let to_relative = stage_relative.join(name);
            if let Some(parent) = to_relative.parent() {
                let dir = checked_skill_path(
                    &package_scope,
                    parent,
                    Action::FileWrite,
                    Mode::M1Workspace,
                    audit,
                )?;
                std::fs::create_dir_all(dir)?;
            }
            let to = checked_skill_path(
                &package_scope,
                &to_relative,
                Action::FileWrite,
                Mode::M1Workspace,
                audit,
            )?;
            std::fs::write(to, bytes)?;
        }
        std::fs::rename(&stage, &target)?;
        Ok(())
    })();
    if copy.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    copy?;
    let skill = InstructionSkill {
        id: parsed.id.clone(),
        name: parsed.name,
        description: parsed.description,
        body: parsed.body,
        files: survey
            .keep
            .into_iter()
            .filter(|f| !f.eq_ignore_ascii_case("SKILL.md"))
            .collect(),
    };
    Ok((view(&skill, true, survey.skipped), parsed.id))
}

/// Merkt den Skill als aktiv (nach dem Import).
pub fn activate_after_import(state: &AppState, id: &str) -> AppResult<()> {
    let session = require_session(state)?;
    let mut vault = session.vault_runtime.lock()?;
    let mut ids = read_active(vault.repository());
    if !ids.iter().any(|i| i == id) {
        ids.push(id.to_owned());
        write_active(vault.repository_mut(), &ids)?;
    }
    Ok(())
}

/// Alle Anleitungs-Skills mit Aktiv-Zustand.
pub fn list(state: &AppState) -> AppResult<Vec<InstalledSkillView>> {
    let root = ensure_bootstrap(state)?.package_root.clone();
    let session = require_session(state)?;
    let active = {
        let vault = session.vault_runtime.lock()?;
        read_active(vault.repository())
    };
    Ok(registry(&root)
        .scan()
        .iter()
        .map(|skill| view(skill, active.contains(&skill.id), Vec::new()))
        .collect())
}

/// Schaltet einen Anleitungs-Skill ein oder aus.
#[tauri::command]
pub fn set_instruction_skill_active(
    state: State<'_, AppState>,
    id: String,
    active: bool,
) -> AppResult<()> {
    let session = require_session(&state)?;
    let mut vault = session.vault_runtime.lock()?;
    let mut ids = read_active(vault.repository());
    ids.retain(|i| *i != id);
    if active {
        ids.push(id);
    }
    write_active(vault.repository_mut(), &ids)?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

/// Entfernt einen installierten Skill (Anleitung oder WASM). Werkzeuge eines WASM-Skills
/// verschwinden beim nächsten Start.
#[tauri::command]
pub fn uninstall_skill(state: State<'_, AppState>, id: String, kind: String) -> AppResult<()> {
    use pa_policy::{CapabilityAction as Action, Mode, PathScope};
    let root = ensure_bootstrap(&state)?.package_root.clone();
    let session = require_session(&state)?;
    let shared = session
        .vault_runtime
        .shared()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let clean = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !clean {
        return Err(AppError::Invalid("Ungültige Skill-Kennung.".to_owned()));
    }
    let relative = match kind.as_str() {
        "instructions" => relative_root().join(&id),
        "wasm" => PathBuf::from("AI").join("skills").join("user").join(&id),
        _ => return Err(AppError::Invalid("Unbekannte Skill-Art.".to_owned())),
    };
    let scope = PathScope::new(&root).map_err(|e| AppError::Internal(e.to_string()))?;
    let mut audit = VaultAuditSink::new(Arc::clone(&shared));
    let path = checked_skill_path(
        &scope,
        &relative,
        Action::FileWrite,
        Mode::M1Workspace,
        &mut audit,
    )?;
    if !path.is_dir() {
        return Err(AppError::Invalid(
            "Der Skill ist nicht (mehr) installiert.".to_owned(),
        ));
    }
    std::fs::remove_dir_all(path)?;
    let mut vault = session.vault_runtime.lock()?;
    let mut ids = read_active(vault.repository());
    ids.retain(|i| *i != id);
    write_active(vault.repository_mut(), &ids)?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

/// Zusatztext der aktiven Skills für den Systemprompt; `None`, wenn keiner aktiv ist.
pub fn prompt(
    repository: &pa_vault::repository::VaultRepository,
    package_root: &Path,
    context_tokens: u32,
) -> Option<String> {
    let active = read_active(repository);
    if active.is_empty() {
        return None;
    }
    let reg = registry(package_root);
    let skills: Vec<InstructionSkill> = active.iter().filter_map(|id| reg.read(id)).collect();
    // Ein Viertel des Kontexts in Zeichen (je Token etwa vier Zeichen), mit Ober- und Untergrenze.
    let total = (context_tokens as usize).clamp(2_000, 12_000);
    instructions::prompt_for(&skills, total / 2, total)
}

/// Hängt die aktiven Skills an die Grundanweisung des Zuges.
pub fn attach(
    turn: &mut crate::library_cmds::TurnContext,
    repository: &pa_vault::repository::VaultRepository,
    package_root: &Path,
    context_tokens: u32,
) {
    let Some(extra) = prompt(repository, package_root, context_tokens) else {
        return;
    };
    turn.project_prompt = Some(match turn.project_prompt.take() {
        Some(existing) if !existing.trim().is_empty() => format!("{existing}\n\n{extra}"),
        _ => extra,
    });
}

/// Wendet einen vom Nutzer mit „/name“ gewählten Skill nur auf diesen Zug an.
///
/// Warum getrennt von `attach`: Die globale Aktivierung gilt für jede Frage. Wer
/// „/skill Aufgabe“ tippt, will den Skill genau jetzt, auch ohne ihn dauerhaft
/// einzuschalten. Gibt `Ok(true)` zurück, wenn ein Anleitungs-Skill angehängt
/// wurde, und `Ok(false)` für andere Arten (WASM-Skills laufen über Werkzeuge).
///
/// # Errors
/// Wenn die Kennung kein gültiger Skill-Name ist.
pub fn attach_one(
    turn: &mut crate::library_cmds::TurnContext,
    package_root: &Path,
    id: &str,
    context_tokens: u32,
) -> Result<bool, String> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("Ungültige Skill-Kennung: {id}"));
    }
    let Some(skill) = registry(package_root).read(id) else {
        return Ok(false);
    };
    let total = (context_tokens as usize).clamp(2_000, 12_000);
    let Some(extra) = instructions::prompt_for(std::slice::from_ref(&skill), total / 2, total)
    else {
        return Ok(false);
    };
    let extra = format!(
        "Der Nutzer hat für diese Anfrage ausdrücklich den Skill „{}“ gewählt. Wende ihn an.\n\n{extra}",
        skill.name
    );
    turn.project_prompt = Some(match turn.project_prompt.take() {
        Some(existing) if !existing.trim().is_empty() => format!("{existing}\n\n{extra}"),
        _ => extra,
    });
    Ok(true)
}

/// Hinweis für den Werkzeugpfad, wenn der gewählte Skill ein WASM-Skill ist.
pub fn wasm_hint(turn: &mut crate::library_cmds::TurnContext, id: &str) {
    let extra = format!(
        "Der Nutzer hat für diese Anfrage den Skill „{id}“ gewählt. Nutze dafür dessen Werkzeuge (Namen beginnen mit skill_{id}_)."
    );
    turn.project_prompt = Some(match turn.project_prompt.take() {
        Some(existing) if !existing.trim().is_empty() => format!("{existing}\n\n{extra}"),
        _ => extra,
    });
}
