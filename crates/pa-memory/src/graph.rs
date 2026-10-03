//! Gedächtnis als Graph (wie in Obsidian): Fakten sind Knoten, Verknüpfungen entstehen aus dem
//! Text selbst.
//!
//! Warum ohne eigenes Schema: Alles lässt sich aus den gespeicherten Fakten ableiten. Es gibt drei
//! Arten von Kanten:
//! - **Kategorie**: jeder Fakt hängt an einem Sammelknoten seiner Kategorie.
//! - **Ersetzt**: ein alter Fakt zeigt auf den neuen, der ihn abgelöst hat (`superseded_by`).
//! - **Link**: `[[Titel]]` im Text verweist auf einen anderen Fakt. Der Titel eines Fakts ist seine
//!   erste Zeile. Gibt es keinen passenden Fakt, entsteht ein Geisterknoten, den man später mit
//!   einem Fakt dieses Titels füllen kann.
//!
//! Semantische Nachbarn (Ähnlichkeit der Einbettungen) fehlen mit Absicht: Der aktuelle
//! Einbettungsbaustein ist ein Hash-Verfahren, dessen Ähnlichkeit nichts bedeutet. Solche Kanten
//! wären zufällig und würden Verknüpfungen vortäuschen.

use std::collections::{HashMap, HashSet};

use pa_types::memory::{
    Fact, FactCategory, GraphEdge, GraphEdgeKind, GraphNode, GraphNodeKind, MemoryGraph,
};

/// Längster Titel eines Knotens in Zeichen.
const MAX_TITLE: usize = 80;
/// Mehr `[[Links]]` je Fakt werden ignoriert (Schutz vor riesigen Graphen).
const MAX_LINKS_PER_FACT: usize = 20;

/// Titel eines Fakts: erste nicht leere Zeile, gekürzt.
pub fn title_of(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let mut title: String = line.chars().take(MAX_TITLE).collect();
    if line.chars().count() > MAX_TITLE {
        title.push('…');
    }
    title
}

/// Alle `[[Ziel]]` im Text, ohne Wiederholung und in Reihenfolge des Auftretens.
///
/// Ein Link darf `|Anzeigetext` enthalten (`[[Ziel|Text]]`); es zählt nur das Ziel. Leere oder zu
/// lange Ziele und Zeilenumbrüche im Link werden verworfen.
pub fn extract_links(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let inner = &after[..end];
        rest = &after[end + 2..];
        if inner.contains('\n') || inner.contains("[[") {
            continue;
        }
        let target = inner.split('|').next().unwrap_or("").trim();
        if target.is_empty() || target.chars().count() > MAX_TITLE {
            continue;
        }
        if !found.iter().any(|known| known.eq_ignore_ascii_case(target)) {
            found.push(target.to_owned());
        }
        if found.len() >= MAX_LINKS_PER_FACT {
            break;
        }
    }
    found
}

fn category_label(category: FactCategory) -> &'static str {
    match category {
        FactCategory::Preference => "Vorlieben",
        FactCategory::Project => "Projekte",
        FactCategory::Person => "Personen",
        FactCategory::Skill => "Fähigkeiten",
        FactCategory::Constraint => "Einschränkungen",
        FactCategory::Other => "Sonstiges",
    }
}

/// Baut den Graphen. `include_superseded` blendet abgelöste Fakten mit ein.
pub fn build_graph(facts: &[Fact], include_superseded: bool) -> MemoryGraph {
    let shown: Vec<&Fact> = facts
        .iter()
        .filter(|fact| include_superseded || fact.superseded_by.is_none())
        .collect();
    let shown_ids: HashSet<&str> = shown.iter().map(|fact| fact.id.as_str()).collect();

    let mut graph = MemoryGraph::default();
    let mut categories_used: Vec<FactCategory> = Vec::new();
    // Titel (kleingeschrieben) -> Fakt-ID; bei gleichem Titel gewinnt der aktive, dann der erste.
    let mut by_title: HashMap<String, &str> = HashMap::new();
    for fact in &shown {
        let key = title_of(&fact.text).to_lowercase();
        match by_title.get(key.as_str()) {
            Some(_) if fact.superseded_by.is_some() => {}
            _ => {
                by_title.insert(key, fact.id.as_str());
            }
        }
    }

    for fact in &shown {
        graph.nodes.push(GraphNode {
            id: format!("fact:{}", fact.id),
            kind: GraphNodeKind::Fact,
            label: title_of(&fact.text),
            category: Some(fact.category),
            verified: fact.user_verified,
            superseded: fact.superseded_by.is_some(),
            confidence: fact.confidence,
            access_count: fact.access_count,
            created_unix_ms: fact.valid_from_unix_ms.max(0),
            learned: fact.source_message_id.is_some(),
        });
        if !categories_used.contains(&fact.category) {
            categories_used.push(fact.category);
        }
        graph.edges.push(GraphEdge {
            from: format!("category:{}", fact.category.as_str()),
            to: format!("fact:{}", fact.id),
            kind: GraphEdgeKind::Category,
        });
    }

    for category in categories_used {
        graph.nodes.push(GraphNode {
            id: format!("category:{}", category.as_str()),
            kind: GraphNodeKind::Category,
            label: category_label(category).to_owned(),
            category: Some(category),
            verified: false,
            superseded: false,
            confidence: 1.0,
            access_count: 0,
            created_unix_ms: 0,
            learned: false,
        });
    }

    let mut ghosts: Vec<String> = Vec::new();
    for fact in &shown {
        if let Some(successor) = fact.superseded_by.as_deref() {
            if shown_ids.contains(successor) {
                graph.edges.push(GraphEdge {
                    from: format!("fact:{}", fact.id),
                    to: format!("fact:{successor}"),
                    kind: GraphEdgeKind::Supersedes,
                });
            }
        }
        for target in extract_links(&fact.text) {
            let key = target.to_lowercase();
            let to = match by_title.get(key.as_str()) {
                Some(id) if *id == fact.id.as_str() => continue,
                Some(id) => format!("fact:{id}"),
                None => {
                    let ghost_id = format!("ghost:{key}");
                    if !ghosts.contains(&ghost_id) {
                        ghosts.push(ghost_id.clone());
                        graph.nodes.push(GraphNode {
                            id: ghost_id.clone(),
                            kind: GraphNodeKind::Ghost,
                            label: target.clone(),
                            category: None,
                            verified: false,
                            superseded: false,
                            confidence: 0.0,
                            access_count: 0,
                            created_unix_ms: 0,
                            learned: false,
                        });
                    }
                    ghost_id
                }
            };
            let from = format!("fact:{}", fact.id);
            let duplicate = graph
                .edges
                .iter()
                .any(|edge| edge.kind == GraphEdgeKind::Link && edge.from == from && edge.to == to);
            if !duplicate {
                graph.edges.push(GraphEdge {
                    from,
                    to,
                    kind: GraphEdgeKind::Link,
                });
            }
        }
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(id: &str, text: &str, category: FactCategory, superseded_by: Option<&str>) -> Fact {
        Fact {
            id: id.to_owned(),
            text: text.to_owned(),
            category,
            confidence: 0.9,
            source_message_id: None,
            valid_from_unix_ms: 10,
            valid_until_unix_ms: None,
            superseded_by: superseded_by.map(str::to_owned),
            user_verified: true,
            access_count: 0,
            last_accessed_unix_ms: None,
        }
    }

    #[test]
    fn titles_are_the_first_non_empty_line_and_are_shortened() {
        assert_eq!(title_of("\n  Rust lernen  \nmehr Text"), "Rust lernen");
        assert_eq!(title_of(""), "");
        let long = "a".repeat(200);
        let title = title_of(&long);
        assert_eq!(title.chars().count(), MAX_TITLE + 1);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn links_are_extracted_once_with_aliases_and_without_junk() {
        let text =
            "Siehe [[Rust]] und [[rust]], auch [[Svelte|das Frontend]] und [[ ]] [[a\nb]] [[offen";
        assert_eq!(extract_links(text), vec!["Rust", "Svelte"]);
        assert!(extract_links("kein Link [ein] [[]]").is_empty());
        let many: String = (0..50).map(|i| format!("[[L{i}]]")).collect();
        assert_eq!(extract_links(&many).len(), MAX_LINKS_PER_FACT);
        assert!(extract_links(&format!("[[{}]]", "x".repeat(MAX_TITLE + 1))).is_empty());
    }

    #[test]
    fn every_fact_hangs_on_a_category_hub() {
        let facts = vec![
            fact("1", "Mag Kaffee", FactCategory::Preference, None),
            fact("2", "Baut IAP", FactCategory::Project, None),
            fact("3", "Mag Tee", FactCategory::Preference, None),
        ];
        let graph = build_graph(&facts, false);
        let hubs: Vec<&GraphNode> = graph
            .nodes
            .iter()
            .filter(|n| n.kind == GraphNodeKind::Category)
            .collect();
        assert_eq!(hubs.len(), 2, "nur benutzte Kategorien");
        let category_edges = graph
            .edges
            .iter()
            .filter(|e| e.kind == GraphEdgeKind::Category)
            .count();
        assert_eq!(category_edges, 3);
        assert!(graph.edges.contains(&GraphEdge {
            from: "category:preference".to_owned(),
            to: "fact:3".to_owned(),
            kind: GraphEdgeKind::Category,
        }));
    }

    #[test]
    fn a_link_connects_to_the_fact_with_that_title_or_creates_a_ghost() {
        let facts = vec![
            fact(
                "1",
                "IAP\nEin portabler Agent, nutzt [[Rust]] und [[Tauri]]",
                FactCategory::Project,
                None,
            ),
            fact("2", "Rust\nSystemsprache", FactCategory::Skill, None),
            fact(
                "3",
                "Notiz mit [[iap]] und [[Notiz mit [[iap]] und [[x]]",
                FactCategory::Other,
                None,
            ),
        ];
        let graph = build_graph(&facts, false);
        let links: Vec<(&str, &str)> = graph
            .edges
            .iter()
            .filter(|e| e.kind == GraphEdgeKind::Link)
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();
        assert!(links.contains(&("fact:1", "fact:2")), "{links:?}");
        assert!(links.contains(&("fact:1", "ghost:tauri")), "{links:?}");
        assert!(
            links.contains(&("fact:3", "fact:1")),
            "Groß-/Kleinschreibung egal: {links:?}"
        );
        let ghosts: Vec<&GraphNode> = graph
            .nodes
            .iter()
            .filter(|n| n.kind == GraphNodeKind::Ghost)
            .collect();
        assert_eq!(ghosts.iter().filter(|g| g.label == "Tauri").count(), 1);
    }

    #[test]
    fn a_link_to_oneself_is_ignored_and_duplicate_links_collapse() {
        let facts = vec![fact(
            "1",
            "Eigener Titel\n[[Eigener Titel]] [[Eigener Titel]]",
            FactCategory::Other,
            None,
        )];
        let graph = build_graph(&facts, false);
        assert!(graph.edges.iter().all(|e| e.kind != GraphEdgeKind::Link));
        assert!(graph.nodes.iter().all(|n| n.kind != GraphNodeKind::Ghost));
    }

    #[test]
    fn superseded_facts_show_up_only_on_request_with_a_directed_edge() {
        let facts = vec![
            fact("old", "Mag Kaffee", FactCategory::Preference, Some("new")),
            fact(
                "new",
                "Mag Kaffee ohne Zucker",
                FactCategory::Preference,
                None,
            ),
        ];
        let active = build_graph(&facts, false);
        assert_eq!(
            active
                .nodes
                .iter()
                .filter(|n| n.kind == GraphNodeKind::Fact)
                .count(),
            1
        );
        assert!(active
            .edges
            .iter()
            .all(|e| e.kind != GraphEdgeKind::Supersedes));

        let all = build_graph(&facts, true);
        assert_eq!(
            all.nodes
                .iter()
                .filter(|n| n.kind == GraphNodeKind::Fact)
                .count(),
            2
        );
        assert!(all.edges.contains(&GraphEdge {
            from: "fact:old".to_owned(),
            to: "fact:new".to_owned(),
            kind: GraphEdgeKind::Supersedes,
        }));
        let old = all.nodes.iter().find(|n| n.id == "fact:old").unwrap();
        assert!(old.superseded);
    }

    #[test]
    fn a_link_prefers_the_active_fact_when_titles_clash() {
        let facts = vec![
            fact("old", "Ziel\nalt", FactCategory::Other, Some("new")),
            fact("new", "Ziel\nneu", FactCategory::Other, None),
            fact("src", "[[Ziel]]", FactCategory::Other, None),
        ];
        let graph = build_graph(&facts, true);
        assert!(graph.edges.contains(&GraphEdge {
            from: "fact:src".to_owned(),
            to: "fact:new".to_owned(),
            kind: GraphEdgeKind::Link,
        }));
    }

    #[test]
    fn the_graph_is_deterministic_and_every_edge_points_at_a_node() {
        let facts = vec![
            fact("a", "A [[B]] [[Fehlt]]", FactCategory::Other, None),
            fact("b", "B", FactCategory::Project, Some("c")),
            fact("c", "C [[A]]", FactCategory::Person, None),
        ];
        let first = build_graph(&facts, true);
        assert_eq!(first, build_graph(&facts, true));
        let ids: HashSet<&str> = first.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids.len(), first.nodes.len(), "keine doppelten Knoten");
        for edge in &first.edges {
            assert!(
                ids.contains(edge.from.as_str()) && ids.contains(edge.to.as_str()),
                "{edge:?}"
            );
        }
    }
}
