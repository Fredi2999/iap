//! WASM-Skill-System (Konzept 8.2).
//!
//! Enthält:
//! - [`Manifest`] — Skill-Manifest aus dem Konzept 8.2 (Berechtigungen +
//!   Werkzeugdefinitionen), inklusive Berechnung der Berechtigungsanzeige.
//! - [`SkillRegistry`] — Skill-Ordner-Scanner mit SHA-256-Verifikation
//!   der referenzierten `.wasm`-Datei.
//! - [`dry_run`] — Trockenlauf: prüft Manifest, Datei-Hash und Argumente,
//!   ohne WASM auszuführen. So sieht man vor dem ersten echten Aufruf, was geschähe.
//! - [`run`] — echte Ausführung in der Wasmtime-Sandbox ([`sandbox`]):
//!   ohne Importe, mit Zeit- und Speichergrenze aus dem Manifest.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub mod instructions;
pub mod sandbox;

/// Obergrenze für den Speicher eines Skills, egal was das Manifest verlangt:
/// Die Zielhardware hat 8 GB, und das Modell braucht den Großteil davon.
const MAX_SKILL_MEMORY_MB: u32 = 512;
/// Obergrenze der Laufzeit, damit ein Skill die Oberfläche nie lange blockiert.
const MAX_SKILL_RUNTIME_MS: u32 = 60_000;
/// Größte Antwort eines Skills; mehr passt ohnehin nicht sinnvoll in den Kontext.
const MAX_SKILL_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum SkillError {
    #[error("Skill-Manifest ungültig: {0}")]
    Manifest(String),
    #[error("Skill-Datei nicht gefunden: {0}")]
    Missing(PathBuf),
    #[error("Skill `{id}`: SHA-256 stimmt nicht (erwartet {expected}, gefunden {actual})")]
    HashMismatch {
        id: String,
        expected: String,
        actual: String,
    },
    #[error("Skill `{id}`: {reason}")]
    Argument { id: String, reason: String },
    #[error("Skill `{id}` wird nicht ausgeführt: {reason}")]
    Denied { id: String, reason: String },
    #[error("Skill `{id}`: {source}")]
    Sandbox {
        id: String,
        #[source]
        source: sandbox::SandboxError,
    },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Skill-Manifest nach Konzept 8.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub skill: SkillHeader,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(rename = "tools", default)]
    pub tools: Vec<ToolDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillHeader {
    pub id: String,
    pub name: String,
    pub version: String,
    pub runtime: String,
    pub entry: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub fs_read: Vec<String>,
    #[serde(default)]
    pub fs_write: Vec<String>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub exec: bool,
    #[serde(default = "default_memory_mb")]
    pub max_memory_mb: u32,
    #[serde(default = "default_runtime_ms")]
    pub max_runtime_ms: u32,
}

fn default_memory_mb() -> u32 {
    64
}
fn default_runtime_ms() -> u32 {
    5_000
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub parameters: Value,
}

impl Manifest {
    /// Parst ein Manifest im TOML-Format.
    pub fn parse(text: &str) -> Result<Self, SkillError> {
        toml::from_str::<Manifest>(text).map_err(|error| SkillError::Manifest(error.to_string()))
    }

    /// Verdichtet die Berechtigungen zu einem klaren Aufzählungssatz für
    /// die Installations-UI. Kein Marketing-Text, keine Verschleierung.
    pub fn permission_summary(&self) -> Vec<PermissionEntry> {
        let mut result = Vec::new();
        if !self.capabilities.fs_read.is_empty() {
            result.push(PermissionEntry {
                topic: "Dateien lesen".to_owned(),
                detail: format!("Pfade: {}", self.capabilities.fs_read.join(", ")),
                is_sensitive: true,
            });
        }
        if !self.capabilities.fs_write.is_empty() {
            result.push(PermissionEntry {
                topic: "Dateien schreiben".to_owned(),
                detail: format!("Pfade: {}", self.capabilities.fs_write.join(", ")),
                is_sensitive: true,
            });
        }
        if self.capabilities.network {
            result.push(PermissionEntry {
                topic: "Netzwerk".to_owned(),
                detail: "Skill fordert Netzwerkzugriff — Portable-AI verweigert das immer."
                    .to_owned(),
                is_sensitive: true,
            });
        }
        if self.capabilities.exec {
            result.push(PermissionEntry {
                topic: "Prozess-Start".to_owned(),
                detail: "Skill fordert Prozess-Start — im MVP nicht zulässig.".to_owned(),
                is_sensitive: true,
            });
        }
        result.push(PermissionEntry {
            topic: "Ressourcen".to_owned(),
            detail: format!(
                "≤ {} MB Speicher, ≤ {} ms Laufzeit",
                self.capabilities.max_memory_mb, self.capabilities.max_runtime_ms
            ),
            is_sensitive: false,
        });
        result
    }
}

/// Einzelner Zeileneintrag der Berechtigungsanzeige.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionEntry {
    pub topic: String,
    pub detail: String,
    pub is_sensitive: bool,
}

/// Trägt einen geladenen und verifizierten Skill.
#[derive(Debug, Clone)]
pub struct InstalledSkill {
    pub manifest: Manifest,
    pub manifest_path: PathBuf,
    pub wasm_path: PathBuf,
}

/// Scanner über einen Skill-Wurzelordner (typisch: `AI/skills/user`).
pub struct SkillRegistry {
    root: PathBuf,
}

impl SkillRegistry {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Liest alle Manifeste unter `<root>/<id>/manifest.toml`.
    pub fn scan(&self) -> Result<Vec<InstalledSkill>, SkillError> {
        let mut result = Vec::new();
        if !self.root.exists() {
            return Ok(result);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let dir = entry.path();
            let manifest_path = dir.join("manifest.toml");
            if !manifest_path.exists() {
                continue;
            }
            let text = fs::read_to_string(&manifest_path)?;
            let manifest = Manifest::parse(&text)?;
            let wasm_path = dir.join(&manifest.skill.entry);
            if !wasm_path.exists() {
                return Err(SkillError::Missing(wasm_path));
            }
            verify_hash(&wasm_path, &manifest.skill.id, &manifest.skill.sha256)?;
            result.push(InstalledSkill {
                manifest,
                manifest_path,
                wasm_path,
            });
        }
        result.sort_by(|a, b| a.manifest.skill.id.cmp(&b.manifest.skill.id));
        Ok(result)
    }
}

fn verify_hash(path: &Path, id: &str, expected: &str) -> Result<(), SkillError> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = hex::encode(hasher.finalize());
    if !expected.eq_ignore_ascii_case(&actual) {
        return Err(SkillError::HashMismatch {
            id: id.to_owned(),
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok(())
}

/// Ergebnis eines Trockenlaufs.
#[derive(Debug, Clone, Serialize)]
pub struct DryRunOutcome {
    pub skill_id: String,
    pub tool: String,
    pub arguments: BTreeMap<String, Value>,
    pub permissions: Vec<PermissionEntry>,
    pub warnings: Vec<String>,
}

/// Prüft, ob der Aufruf plausibel ist: Werkzeug existiert, Parameter
/// stimmen strukturell, Berechtigungen sind bekannt. **Führt kein WASM
/// aus.** Die UI zeigt den Nutzern damit vor jeder echten Ausführung, was
/// geschehen würde.
pub fn dry_run(
    skill: &InstalledSkill,
    tool: &str,
    arguments: BTreeMap<String, Value>,
) -> Result<DryRunOutcome, SkillError> {
    let tool_def = skill
        .manifest
        .tools
        .iter()
        .find(|t| t.name == tool)
        .ok_or_else(|| SkillError::Argument {
            id: skill.manifest.skill.id.clone(),
            reason: format!("Werkzeug `{tool}` ist nicht Teil des Skills"),
        })?;
    validate_arguments(&skill.manifest.skill.id, tool_def, &arguments)?;
    let mut warnings = Vec::new();
    if skill.manifest.capabilities.network {
        warnings
            .push("Skill fordert Netzwerk; die Policy blockt das immer (Konzept 10.1)".to_owned());
    }
    if skill.manifest.capabilities.exec {
        warnings.push("Skill fordert Prozess-Start; im MVP nicht zulässig".to_owned());
    }
    if let Some(reason) = unsupported_capability(&skill.manifest.capabilities) {
        warnings.push(reason.to_owned());
    }
    Ok(DryRunOutcome {
        skill_id: skill.manifest.skill.id.clone(),
        tool: tool.to_owned(),
        arguments,
        permissions: skill.manifest.permission_summary(),
        warnings,
    })
}

fn validate_arguments(
    skill_id: &str,
    tool: &ToolDef,
    arguments: &BTreeMap<String, Value>,
) -> Result<(), SkillError> {
    let Some(schema) = tool.parameters.as_object() else {
        return Ok(());
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for req in required {
            if let Some(name) = req.as_str() {
                if !arguments.contains_key(name) {
                    return Err(SkillError::Argument {
                        id: skill_id.to_owned(),
                        reason: format!("Pflichtparameter `{name}` fehlt"),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Grund, warum ein Skill mit diesen Rechten nicht ausgeführt wird.
///
/// Netzwerk und Prozessstart sind immer gesperrt (Invariante 1 und 2). Datei-
/// rechte bräuchten Host-Funktionen, die jeden Zugriff über pa-policy leiten;
/// die gibt es noch nicht, darum werden solche Skills ehrlich abgelehnt statt
/// ohne die angeforderten Rechte scheinbar zu laufen.
fn unsupported_capability(capabilities: &Capabilities) -> Option<&'static str> {
    if capabilities.network || capabilities.exec {
        return Some("Netzwerk und Prozessstart sind für Skills immer gesperrt");
    }
    if !capabilities.fs_read.is_empty() || !capabilities.fs_write.is_empty() {
        return Some("Dateirechte für Skills sind in dieser Version noch nicht freigeschaltet");
    }
    None
}

/// Führt ein Werkzeug eines Skills in der Sandbox aus.
///
/// Der Aufrufer muss den Aufruf vorher über pa-policy geprüft und protokolliert
/// haben; diese Funktion prüft Werkzeug, Argumente, Rechte und erneut den Hash,
/// damit eine nach der Installation veränderte Datei nie ausgeführt wird.
pub fn run(
    skill: &InstalledSkill,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
) -> Result<Value, SkillError> {
    let id = skill.manifest.skill.id.clone();
    let tool_def = skill
        .manifest
        .tools
        .iter()
        .find(|t| t.name == tool)
        .ok_or_else(|| SkillError::Argument {
            id: id.clone(),
            reason: format!("Werkzeug `{tool}` ist nicht Teil des Skills"),
        })?;
    validate_arguments(&id, tool_def, arguments)?;
    if let Some(reason) = unsupported_capability(&skill.manifest.capabilities) {
        return Err(SkillError::Denied {
            id,
            reason: reason.to_owned(),
        });
    }
    let wasm = fs::read(&skill.wasm_path)?;
    let actual = hex::encode(Sha256::digest(&wasm));
    if !skill.manifest.skill.sha256.eq_ignore_ascii_case(&actual) {
        return Err(SkillError::HashMismatch {
            id,
            expected: skill.manifest.skill.sha256.clone(),
            actual,
        });
    }
    let arguments_json =
        serde_json::to_string(arguments).map_err(|error| SkillError::Argument {
            id: id.clone(),
            reason: error.to_string(),
        })?;
    let capabilities = &skill.manifest.capabilities;
    let limits = sandbox::SandboxLimits {
        max_memory_bytes: capabilities.max_memory_mb.clamp(1, MAX_SKILL_MEMORY_MB) as usize
            * 1024
            * 1024,
        max_runtime: std::time::Duration::from_millis(u64::from(
            capabilities.max_runtime_ms.clamp(1, MAX_SKILL_RUNTIME_MS),
        )),
        max_output_bytes: MAX_SKILL_OUTPUT_BYTES,
    };
    sandbox::execute(&wasm, tool, &arguments_json, &limits)
        .map_err(|source| SkillError::Sandbox { id, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[skill]
id = "csv-analyse"
name = "CSV analysieren"
version = "1.2.0"
runtime = "wasm"
entry = "csv_analyse.wasm"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"

[capabilities]
fs_read = ["$WORKSPACE/**"]
fs_write = []
network = false
exec = false
max_memory_mb = 128
max_runtime_ms = 10000

[[tools]]
name = "csv_summary"
description = "Spaltentypen und Statistik."

[tools.parameters]
type = "object"
required = ["path"]

[tools.parameters.properties.path]
type = "string"
description = "Pfad relativ zum Workspace"
"#;

    #[test]
    fn parses_manifest_and_permission_summary() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        assert_eq!(manifest.skill.id, "csv-analyse");
        let perms = manifest.permission_summary();
        assert!(perms.iter().any(|p| p.topic == "Dateien lesen"));
        assert!(perms.iter().any(|p| p.topic == "Ressourcen"));
    }

    #[test]
    fn dry_run_flags_missing_required_argument() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        let skill = InstalledSkill {
            manifest,
            manifest_path: PathBuf::new(),
            wasm_path: PathBuf::new(),
        };
        let error = dry_run(&skill, "csv_summary", BTreeMap::new()).unwrap_err();
        assert!(matches!(error, SkillError::Argument { .. }));
    }

    #[test]
    fn dry_run_warns_about_file_permissions() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        let skill = InstalledSkill {
            manifest,
            manifest_path: PathBuf::new(),
            wasm_path: PathBuf::new(),
        };
        let mut args = BTreeMap::new();
        args.insert("path".to_owned(), Value::String("x.csv".into()));
        let outcome = dry_run(&skill, "csv_summary", args).unwrap();
        assert!(outcome.warnings.iter().any(|w| w.contains("Dateirechte")));
    }

    #[test]
    fn run_refuses_skills_with_file_permissions() {
        let manifest = Manifest::parse(SAMPLE).unwrap();
        let skill = InstalledSkill {
            manifest,
            manifest_path: PathBuf::new(),
            wasm_path: PathBuf::new(),
        };
        let mut args = BTreeMap::new();
        args.insert("path".to_owned(), Value::String("x.csv".into()));
        let error = run(&skill, "csv_summary", &args).unwrap_err();
        assert!(matches!(error, SkillError::Denied { .. }));
    }

    #[test]
    fn run_executes_pure_skill_and_checks_hash() {
        // Mit dem Test-Feature `wat` nimmt Wasmtime das Textformat direkt an.
        let wasm = br#"(module
          (memory (export "memory") 1)
          (data (i32.const 16) "{\"ok\":true}")
          (func (export "pa_alloc") (param i32) (result i32) i32.const 1024)
          (func (export "pa_skill_invoke") (param i32 i32 i32 i32) (result i64) i64.const 68719476747))"#;
        let dir = std::env::temp_dir().join(format!("pa-skills-run-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let wasm_path = dir.join("skill.wasm");
        fs::write(&wasm_path, wasm).unwrap();
        let hash = hex::encode(Sha256::digest(wasm));
        let text = format!(
            "[skill]\nid = \"rein\"\nname = \"Rein\"\nversion = \"1\"\nruntime = \"wasm\"\nentry = \"skill.wasm\"\nsha256 = \"{hash}\"\n\n[[tools]]\nname = \"los\"\ndescription = \"Test\"\n"
        );
        let skill = InstalledSkill {
            manifest: Manifest::parse(&text).unwrap(),
            manifest_path: PathBuf::new(),
            wasm_path: wasm_path.clone(),
        };
        assert_eq!(
            run(&skill, "los", &BTreeMap::new()).unwrap(),
            serde_json::json!({ "ok": true })
        );

        fs::write(&wasm_path, b"(module)").unwrap();
        assert!(matches!(
            run(&skill, "los", &BTreeMap::new()),
            Err(SkillError::HashMismatch { .. })
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
