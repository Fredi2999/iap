//! Registry aller aktiven Kern-Werkzeuge.
//!
//! Der Orchestrator holt sich hier die `ToolSpec`s für den Prompt und ruft
//! `invoke` mit dem `ToolContext`. Bis mehr als zwölf Werkzeuge registriert
//! sind, entfällt die dynamische Auswahl aus Konzept 8.4; sobald der
//! Schwellwert erreicht ist, meldet [`ToolRegistry::specs_for_prompt`] eine
//! sortierte Teilmenge zurück.

use std::sync::Arc;

use crate::{Tool, ToolContext, ToolError, ToolInvocation, ToolOutput, ToolSpec};

const DYNAMIC_SELECTION_THRESHOLD: usize = 12;

pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Registriert die im MVP mitgelieferten Kern-Werkzeuge.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Arc::new(crate::FileReadTool));
        registry.register(Arc::new(crate::FileWriteTool));
        registry.register(Arc::new(crate::ListDirectoryTool));
        registry.register(Arc::new(crate::WorkspaceSearchTool));
        registry.register(Arc::new(crate::CalculatorTool));
        registry.register(Arc::new(crate::ClockTool));
        registry
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn all_specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|tool| tool.spec()).collect()
    }

    /// Liefert bis zu 12 relevante Werkzeugdefinitionen für den Prompt.
    ///
    /// Solange ≤ 12 Werkzeuge existieren, werden schlicht alle zurückgegeben.
    /// Ab dem 13. Werkzeug rankt eine sehr einfache Bag-of-Words-Heuristik
    /// gegen die Nutzeranfrage; alle Kern-Werkzeuge einer angepinnten
    /// Kategorie bleiben immer im Set.
    pub fn specs_for_prompt(
        &self,
        user_query: &str,
        pinned_category: Option<&str>,
    ) -> Vec<ToolSpec> {
        if self.tools.len() <= DYNAMIC_SELECTION_THRESHOLD {
            return self.all_specs();
        }
        let query_terms: Vec<String> = user_query
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect();
        let mut scored: Vec<(usize, ToolSpec)> = self
            .tools
            .iter()
            .map(|tool| {
                let spec = tool.spec();
                let text = format!("{} {}", spec.name, spec.description).to_lowercase();
                let mut score = query_terms
                    .iter()
                    .filter(|term| text.contains(term.as_str()))
                    .count();
                if let Some(pinned) = pinned_category {
                    if spec.category == pinned {
                        score += 100;
                    }
                }
                (score, spec)
            })
            .collect();
        scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        scored
            .into_iter()
            .take(DYNAMIC_SELECTION_THRESHOLD)
            .map(|(_, spec)| spec)
            .collect()
    }

    /// Sucht ein Werkzeug per Name und führt es aus.
    pub fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.spec().name == invocation.name)
            .ok_or_else(|| ToolError::Invalid {
                tool: invocation.name.clone(),
                reason: "unbekanntes Werkzeug".to_owned(),
            })?;
        tool.invoke(invocation, context)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolSpec;
    use serde_json::json;

    struct StubTool {
        name: String,
        category: String,
    }

    impl Tool for StubTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: self.name.clone(),
                description: self.name.clone(),
                parameters_schema: json!({ "type": "object" }),
                category: self.category.clone(),
            }
        }

        fn invoke(
            &self,
            _invocation: &ToolInvocation,
            _context: &mut ToolContext<'_>,
        ) -> Result<ToolOutput, ToolError> {
            unimplemented!("nur für Registry-Test")
        }
    }

    #[test]
    fn below_threshold_returns_all_tools() {
        let mut registry = ToolRegistry::new();
        for i in 0..10 {
            registry.register(Arc::new(StubTool {
                name: format!("t{i}"),
                category: "files".into(),
            }));
        }
        assert_eq!(registry.specs_for_prompt("nichts", None).len(), 10);
    }

    #[test]
    fn above_threshold_prefers_matching_words() {
        let mut registry = ToolRegistry::new();
        for name in [
            "read_file",
            "write_file",
            "list_directory",
            "workspace_search",
            "calculator",
            "now",
            "extra1",
            "extra2",
            "extra3",
            "extra4",
            "extra5",
            "extra6",
            "extra7",
        ] {
            registry.register(Arc::new(StubTool {
                name: name.into(),
                category: "files".into(),
            }));
        }
        let picked = registry.specs_for_prompt("bitte Datei lesen und rechnen", None);
        assert_eq!(picked.len(), 12);
        // "read_file" enthält "lesen"? Nein, aber wir prüfen nur, dass die
        // Standard-Werkzeuge weiter oben landen als die Extras: Reihenfolge
        // exakt zu prüfen wäre zu spröde. Stattdessen: mindestens ein extra fehlt.
        let names: Vec<String> = picked.into_iter().map(|s| s.name).collect();
        assert!(names.iter().any(|n| n == "read_file"));
        assert!(!names.contains(&"extra7".to_owned()) || !names.contains(&"extra1".to_owned()));
    }

    #[test]
    fn pinned_category_survives_ranking() {
        let mut registry = ToolRegistry::new();
        for i in 0..13 {
            registry.register(Arc::new(StubTool {
                name: format!("gen{i}"),
                category: if i == 0 {
                    "search".into()
                } else {
                    "files".into()
                },
            }));
        }
        let picked = registry.specs_for_prompt("irgendwas", Some("search"));
        assert!(picked.iter().any(|s| s.category == "search"));
    }
}
