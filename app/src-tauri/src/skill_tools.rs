//! Installierte WASM-Skills als Werkzeuge für das Modell und für die Oberfläche.
//!
//! Jedes Werkzeug eines Skills wird zu einem eigenen [`pa_tools::Tool`]. Damit
//! läuft ein Skill-Aufruf durch denselben Pfad wie die Kern-Werkzeuge: erst
//! pa-policy mit Audit-Eintrag, dann die Wasmtime-Sandbox aus `pa-skills`.

use std::{path::Path, sync::Arc};

use pa_policy::{CapabilityAction, CapabilityRequest};
use pa_skills::{InstalledSkill, SkillRegistry, ToolDef};
use pa_tools::{
    evaluate_and_audit, Tool, ToolContext, ToolError, ToolInvocation, ToolOutput, ToolSpec,
};
use serde_json::json;

/// Größte Skill-Antwort, die ins Modell geht; der Rest wird gekürzt.
const MAX_TOOL_OUTPUT_CHARS: usize = 8_000;

/// Ein Werkzeug eines installierten Skills.
pub struct SkillTool {
    skill: InstalledSkill,
    tool: ToolDef,
    name: String,
}

impl SkillTool {
    /// Name, unter dem das Modell das Werkzeug aufruft. Nur `a-z0-9_`, damit er
    /// in der Werkzeug-Grammatik und in Prompts eindeutig bleibt.
    pub fn tool_name(skill_id: &str, tool: &str) -> String {
        let clean = |text: &str| -> String {
            text.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect()
        };
        format!("skill_{}_{}", clean(skill_id), clean(tool))
    }

    /// Ein Werkzeug je Eintrag im Manifest.
    pub fn from_skill(skill: &InstalledSkill) -> Vec<SkillTool> {
        skill
            .manifest
            .tools
            .iter()
            .map(|tool| SkillTool {
                skill: skill.clone(),
                tool: tool.clone(),
                name: Self::tool_name(&skill.manifest.skill.id, &tool.name),
            })
            .collect()
    }
}

/// Liest alle installierten Skills; defekte Skills werden übersprungen,
/// damit ein einzelner kaputter Ordner nicht alle Werkzeuge verhindert.
pub fn installed_skill_tools(package_root: &Path) -> Vec<Arc<dyn Tool>> {
    let registry = SkillRegistry::new(package_root.join("AI").join("skills").join("user"));
    registry
        .scan()
        .unwrap_or_default()
        .iter()
        .flat_map(SkillTool::from_skill)
        .map(|tool| Arc::new(tool) as Arc<dyn Tool>)
        .collect()
}

impl Tool for SkillTool {
    fn spec(&self) -> ToolSpec {
        let parameters_schema = if self.tool.parameters.is_object() {
            self.tool.parameters.clone()
        } else {
            json!({ "type": "object", "properties": {} })
        };
        ToolSpec {
            name: self.name.clone(),
            description: format!(
                "Skill „{}“: {}",
                self.skill.manifest.skill.name, self.tool.description
            ),
            parameters_schema,
            category: "skill".to_owned(),
        }
    }

    fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        // Skills ohne Importe rechnen nur; die Policy bekommt das als reine
        // Berechnung zur Entscheidung, und jeder Aufruf steht im Audit-Log.
        let request = CapabilityRequest {
            action: CapabilityAction::Pure,
            relative_path: None,
            source: invocation.source,
            reason: format!(
                "Skill `{}` Werkzeug `{}`",
                self.skill.manifest.skill.id, self.tool.name
            ),
        };
        evaluate_and_audit(&invocation.name, &request, context)?;
        let value = pa_skills::run(&self.skill, &self.tool.name, &invocation.arguments).map_err(
            |error| ToolError::Invalid {
                tool: invocation.name.clone(),
                reason: error.to_string(),
            },
        )?;
        // Skill-Code stammt von Dritten; seine Ausgabe gilt daher als fremder Inhalt
        // und darf keine automatischen Freigaben auslösen (Konzept 10.3).
        Ok(ToolOutput {
            tool: invocation.name.clone(),
            content: value.to_string(),
            is_untrusted: true,
            truncated_from_bytes: None,
        }
        .truncate(MAX_TOOL_OUTPUT_CHARS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_simple_identifiers() {
        assert_eq!(
            SkillTool::tool_name("csv-analyse", "Summary.v2"),
            "skill_csv_analyse_summary_v2"
        );
    }

    #[test]
    fn missing_skill_folder_yields_no_tools() {
        assert!(installed_skill_tools(Path::new("gibt-es-nicht-iap")).is_empty());
    }
}
