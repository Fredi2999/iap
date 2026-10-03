//! Vorabprüfung eines Workflow-Graphen.
//!
//! Ein Lauf startet nur, wenn diese Prüfung keine Befunde liefert. Die Regeln
//! sind absichtlich streng: Ein Graph, der sich nicht sicher ausführen lässt,
//! wird nicht „irgendwie“ gestartet.

use std::collections::{HashMap, HashSet, VecDeque};

use pa_types::flow::{
    BranchRule, DataKind, GraphProblem, NodeKind, StoreTarget, WorkflowGraph, WorkflowNode,
    WorkflowValidation, WORKFLOW_SCHEMA_VERSION,
};

/// Beschreibung eines Anschlusses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortSpec {
    /// Name, mit dem Kanten den Anschluss ansprechen.
    pub name: &'static str,
    /// Datenart; Kanten verbinden nur gleiche Arten.
    pub kind: DataKind,
    /// Pflichtanschluss (Eingang): muss verbunden sein.
    pub required: bool,
}

const fn port(name: &'static str, kind: DataKind, required: bool) -> PortSpec {
    PortSpec {
        name,
        kind,
        required,
    }
}

static NONE: [PortSpec; 0] = [];
static SIGNAL_OUT: [PortSpec; 1] = [port("out", DataKind::Signal, false)];
static TRIGGER_IN: [PortSpec; 1] = [port("trigger", DataKind::Signal, true)];
static TEXT_OUT: [PortSpec; 1] = [port("out", DataKind::Text, false)];
static QUERY_IN: [PortSpec; 1] = [port("query", DataKind::Text, true)];
static RESULTS_OUT: [PortSpec; 1] = [port("results", DataKind::Results, false)];
static RESULTS_IN: [PortSpec; 1] = [port("results", DataKind::Results, true)];
static MODEL_IN: [PortSpec; 2] = [
    port("text", DataKind::Text, false),
    port("results", DataKind::Results, false),
];
static CHECK_IN: [PortSpec; 2] = [
    port("results", DataKind::Results, true),
    port("answer", DataKind::Text, false),
];
static VERDICT_OUT: [PortSpec; 1] = [port("verdict", DataKind::Verdict, false)];
static CONDITION_IN: [PortSpec; 1] = [port("verdict", DataKind::Verdict, true)];
static CONDITION_OUT: [PortSpec; 2] = [
    port("done", DataKind::Verdict, false),
    port("retry", DataKind::Text, false),
];
static TEXT_IN: [PortSpec; 1] = [port("text", DataKind::Text, true)];
static BRANCH_OUT: [PortSpec; 2] = [
    port("yes", DataKind::Text, false),
    port("no", DataKind::Text, false),
];
static MERGE_IN: [PortSpec; 3] = [
    port("a", DataKind::Text, false),
    port("b", DataKind::Text, false),
    port("c", DataKind::Text, false),
];
static OUTPUT_IN: [PortSpec; 2] = [
    port("answer", DataKind::Text, false),
    port("verdict", DataKind::Verdict, false),
];

/// Eingänge einer Knotenart.
pub fn input_ports(kind: &NodeKind) -> &'static [PortSpec] {
    match kind {
        NodeKind::ManualStart => &NONE,
        NodeKind::Input { .. } => &TRIGGER_IN,
        NodeKind::ExaSearch { .. }
        | NodeKind::WikipediaSearch { .. }
        | NodeKind::BraveSearch { .. }
        | NodeKind::Weather { .. } => &QUERY_IN,
        NodeKind::ExaContents { .. } => &RESULTS_IN,
        NodeKind::LocalModel { .. } => &MODEL_IN,
        NodeKind::Check { .. } => &CHECK_IN,
        NodeKind::Condition { .. } => &CONDITION_IN,
        NodeKind::Output => &OUTPUT_IN,
        NodeKind::RuntimeInput { .. } | NodeKind::Calendar { .. } | NodeKind::MailSearch { .. } => {
            &TRIGGER_IN
        }
        NodeKind::MemorySearch { .. } => &QUERY_IN,
        NodeKind::Skill { .. }
        | NodeKind::Branch { .. }
        | NodeKind::Notify { .. }
        | NodeKind::Store { .. } => &TEXT_IN,
        NodeKind::Join | NodeKind::Merge { .. } => &MERGE_IN,
    }
}

/// Ausgänge einer Knotenart.
pub fn output_ports(kind: &NodeKind) -> &'static [PortSpec] {
    match kind {
        NodeKind::ManualStart => &SIGNAL_OUT,
        NodeKind::Input { .. }
        | NodeKind::LocalModel { .. }
        | NodeKind::RuntimeInput { .. }
        | NodeKind::Calendar { .. }
        | NodeKind::MailSearch { .. }
        | NodeKind::MemorySearch { .. }
        | NodeKind::Skill { .. }
        | NodeKind::Join
        | NodeKind::Weather { .. }
        | NodeKind::Merge { .. } => &TEXT_OUT,
        NodeKind::Branch { .. } => &BRANCH_OUT,
        NodeKind::ExaSearch { .. }
        | NodeKind::ExaContents { .. }
        | NodeKind::WikipediaSearch { .. }
        | NodeKind::BraveSearch { .. } => &RESULTS_OUT,
        NodeKind::Check { .. } => &VERDICT_OUT,
        NodeKind::Condition { .. } => &CONDITION_OUT,
        NodeKind::Output | NodeKind::Notify { .. } | NodeKind::Store { .. } => &NONE,
    }
}

/// Knoten, an denen ein Weg enden darf: Ausgabe sowie Hinweis und Ablegen.
fn is_terminal(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Output | NodeKind::Notify { .. } | NodeKind::Store { .. }
    )
}

/// Ob der Knoten schon startet, wenn nur einer seiner verbundenen Eingänge Daten hat
/// (statt aller). Nur so kann ein Zweig, der nicht genommen wurde, den Lauf nicht blockieren.
pub(crate) fn runs_on_any_input(kind: &NodeKind) -> bool {
    matches!(kind, NodeKind::Join)
}

/// Ob die Kante der Rückweg einer Bedingung ist (einzige erlaubte Schleife).
pub(crate) fn is_retry_edge(graph: &WorkflowGraph, from: &str, from_port: &str) -> bool {
    from_port == "retry"
        && graph
            .nodes
            .iter()
            .any(|n| n.id == from && matches!(n.kind, NodeKind::Condition { .. }))
}

/// Größte Anzahl Ergebnisse je Suche.
pub const MAX_RESULTS_PER_SEARCH: u32 = 10;
/// Größte Zeichenzahl je Seiteninhalt.
pub const MAX_CONTENT_CHARS: u32 = 20_000;
/// Höchstwert für Wiederholungen einer Bedingung.
pub const MAX_CONDITION_ITERATIONS: u32 = 5;
/// Höchstzahl Kriterien einer Prüfung.
pub const MAX_CRITERIA: usize = 8;
/// Größte Zahl Tage im Kalender-Baustein.
pub const MAX_CALENDAR_DAYS: u32 = 31;
/// Höchstzahl Mails, die ein Mail-Knoten liefert.
pub const MAX_MAIL_RESULTS: u32 = 10;
/// Größte Zahl Tage im Wetter-Baustein.
pub const MAX_WEATHER_DAYS: u32 = 7;
/// Größte Trefferzahl der Gedächtnissuche.
pub const MAX_MEMORY_HITS: u32 = 10;

struct Collector {
    problems: Vec<GraphProblem>,
}

impl Collector {
    fn node(&mut self, id: &str, message: impl Into<String>) {
        self.problems.push(GraphProblem {
            node_id: Some(id.to_owned()),
            edge_id: None,
            message: message.into(),
        });
    }
    fn edge(&mut self, id: &str, message: impl Into<String>) {
        self.problems.push(GraphProblem {
            node_id: None,
            edge_id: Some(id.to_owned()),
            message: message.into(),
        });
    }
    fn graph(&mut self, message: impl Into<String>) {
        self.problems.push(GraphProblem {
            node_id: None,
            edge_id: None,
            message: message.into(),
        });
    }
}

fn check_node(collector: &mut Collector, node: &WorkflowNode) {
    match &node.kind {
        NodeKind::Input { text } => {
            if text.trim().is_empty() {
                collector.node(&node.id, "Die Suchfrage ist leer.");
            } else if text.chars().count() > pa_policy::egress::MAX_QUERY_CHARS {
                collector.node(&node.id, "Die Suchfrage ist zu lang.");
            }
        }
        NodeKind::ExaSearch { num_results } => {
            if *num_results == 0 || *num_results > MAX_RESULTS_PER_SEARCH {
                collector.node(
                    &node.id,
                    format!("Die Trefferzahl muss zwischen 1 und {MAX_RESULTS_PER_SEARCH} liegen."),
                );
            }
        }
        NodeKind::ExaContents { max_characters } => {
            if *max_characters < 200 || *max_characters > MAX_CONTENT_CHARS {
                collector.node(
                    &node.id,
                    format!("Die Zeichenzahl muss zwischen 200 und {MAX_CONTENT_CHARS} liegen."),
                );
            }
        }
        NodeKind::LocalModel { instruction } => {
            if instruction.trim().is_empty() {
                collector.node(&node.id, "Die Anweisung für das Modell ist leer.");
            } else if instruction.chars().count() > 2_000 {
                collector.node(
                    &node.id,
                    "Die Anweisung ist zu lang (höchstens 2000 Zeichen).",
                );
            }
        }
        NodeKind::Check { criteria } => {
            let filled: Vec<&String> = criteria.iter().filter(|c| !c.trim().is_empty()).collect();
            if filled.is_empty() {
                collector.node(&node.id, "Die Prüfung braucht mindestens ein Kriterium.");
            }
            if criteria.len() > MAX_CRITERIA {
                collector.node(
                    &node.id,
                    format!("Höchstens {MAX_CRITERIA} Kriterien sind möglich."),
                );
            }
            if criteria.iter().any(|c| c.chars().count() > 300) {
                collector.node(
                    &node.id,
                    "Ein Kriterium ist zu lang (höchstens 300 Zeichen).",
                );
            }
        }
        NodeKind::Condition { max_iterations } => {
            if *max_iterations == 0 || *max_iterations > MAX_CONDITION_ITERATIONS {
                collector.node(
                    &node.id,
                    format!("Die Wiederholungen müssen zwischen 1 und {MAX_CONDITION_ITERATIONS} liegen."),
                );
            }
        }
        NodeKind::RuntimeInput { label, .. } => {
            if label.trim().is_empty() {
                collector.node(&node.id, "Die Beschriftung der Eingabe ist leer.");
            } else if label.chars().count() > 80 {
                collector.node(
                    &node.id,
                    "Die Beschriftung ist zu lang (höchstens 80 Zeichen).",
                );
            }
        }
        NodeKind::Calendar { days_ahead, .. } => {
            if *days_ahead == 0 || *days_ahead > MAX_CALENDAR_DAYS {
                collector.node(
                    &node.id,
                    format!("Die Tage müssen zwischen 1 und {MAX_CALENDAR_DAYS} liegen."),
                );
            }
        }
        NodeKind::MailSearch {
            from,
            subject,
            limit,
            ..
        } => {
            if *limit == 0 || *limit > MAX_MAIL_RESULTS {
                collector.node(
                    &node.id,
                    format!("Die Trefferzahl muss zwischen 1 und {MAX_MAIL_RESULTS} liegen."),
                );
            }
            for (name, value) in [("Absender", from), ("Betreff", subject)] {
                if value.chars().count() > 200 || value.chars().any(char::is_control) {
                    collector.node(
                        &node.id,
                        format!("Der Filter „{name}“ ist zu lang oder enthält Steuerzeichen."),
                    );
                }
            }
        }
        NodeKind::MemorySearch { max_hits } => {
            if *max_hits == 0 || *max_hits > MAX_MEMORY_HITS {
                collector.node(
                    &node.id,
                    format!("Die Trefferzahl muss zwischen 1 und {MAX_MEMORY_HITS} liegen."),
                );
            }
        }
        NodeKind::Skill { skill_id } => {
            let valid = !skill_id.is_empty()
                && skill_id.len() <= 64
                && skill_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if !valid {
                collector.node(&node.id, "Bitte einen Skill auswählen.");
            }
        }
        NodeKind::Branch { rule } => match rule {
            BranchRule::Contains { text } => {
                if text.trim().is_empty() {
                    collector.node(&node.id, "Der gesuchte Text ist leer.");
                } else if text.chars().count() > 200 {
                    collector.node(
                        &node.id,
                        "Der gesuchte Text ist zu lang (höchstens 200 Zeichen).",
                    );
                }
            }
            BranchRule::ModelYesNo { question } => {
                if question.trim().is_empty() {
                    collector.node(&node.id, "Die Frage für das Modell ist leer.");
                } else if question.chars().count() > 300 {
                    collector.node(&node.id, "Die Frage ist zu lang (höchstens 300 Zeichen).");
                }
            }
        },
        NodeKind::Merge { template } => {
            if template.trim().is_empty() {
                collector.node(&node.id, "Die Vorlage ist leer.");
            } else if template.chars().count() > 2_000 {
                collector.node(
                    &node.id,
                    "Die Vorlage ist zu lang (höchstens 2000 Zeichen).",
                );
            } else if unknown_placeholder(template) {
                collector.node(
                    &node.id,
                    "Die Vorlage darf nur {{a}}, {{b}} und {{c}} enthalten.",
                );
            }
        }
        NodeKind::Notify { title } => {
            if title.trim().is_empty() {
                collector.node(&node.id, "Der Titel des Hinweises ist leer.");
            } else if title.chars().count() > 80 {
                collector.node(&node.id, "Der Titel ist zu lang (höchstens 80 Zeichen).");
            }
        }
        NodeKind::Store { target } => {
            if let StoreTarget::File { relative_path } = target {
                if !plausible_relative_path(relative_path) {
                    collector.node(
                        &node.id,
                        "Bitte einen Dateipfad im Arbeitsordner angeben (ohne Laufwerk, ohne „..“).",
                    );
                }
            }
        }
        NodeKind::WikipediaSearch { num_results, lang } => {
            if *num_results == 0 || *num_results > MAX_RESULTS_PER_SEARCH {
                collector.node(
                    &node.id,
                    format!("Die Trefferzahl muss zwischen 1 und {MAX_RESULTS_PER_SEARCH} liegen."),
                );
            }
            if lang != "de" && lang != "en" {
                collector.node(&node.id, "Die Sprache muss „de“ oder „en“ sein.");
            }
        }
        NodeKind::BraveSearch { num_results } => {
            if *num_results == 0 || *num_results > MAX_RESULTS_PER_SEARCH {
                collector.node(
                    &node.id,
                    format!("Die Trefferzahl muss zwischen 1 und {MAX_RESULTS_PER_SEARCH} liegen."),
                );
            }
        }
        NodeKind::Weather { days } => {
            if *days == 0 || *days > MAX_WEATHER_DAYS {
                collector.node(
                    &node.id,
                    format!("Die Tage müssen zwischen 1 und {MAX_WEATHER_DAYS} liegen."),
                );
            }
        }
        NodeKind::ManualStart | NodeKind::Output | NodeKind::Join => {}
    }
}

/// Grobe Prüfung des Pfads beim Entwerfen. Maßgeblich bleibt pa-policy beim Schreiben.
fn plausible_relative_path(path: &str) -> bool {
    let path = path.trim();
    !path.is_empty()
        && path.chars().count() <= 200
        && !path.starts_with(['/', '\\', '~'])
        && !path.contains(':')
        && !path.chars().any(char::is_control)
        && !path
            .replace('\\', "/")
            .split('/')
            .any(|part| part == ".." || part.is_empty())
}

/// Ob die Vorlage einen anderen Platzhalter als `{{a}}`, `{{b}}`, `{{c}}` enthält.
fn unknown_placeholder(template: &str) -> bool {
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return true;
        };
        if !matches!(after[..end].trim(), "a" | "b" | "c") {
            return true;
        }
        rest = &after[end + 2..];
    }
    false
}

/// Prüft den Graphen.
pub fn validate(graph: &WorkflowGraph) -> WorkflowValidation {
    let mut collector = Collector {
        problems: Vec::new(),
    };
    if graph.version != WORKFLOW_SCHEMA_VERSION {
        collector.graph(format!(
            "Das Format (Version {}) wird nicht unterstützt.",
            graph.version
        ));
    }
    if graph.nodes.is_empty() {
        collector.graph("Der Ablauf ist leer.");
    }

    let mut ids = HashSet::new();
    for node in &graph.nodes {
        if !ids.insert(node.id.as_str()) {
            collector.node(&node.id, "Diese Knoten-ID kommt mehrfach vor.");
        }
        check_node(&mut collector, node);
    }
    let starts = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::ManualStart))
        .count();
    if starts != 1 && !graph.nodes.is_empty() {
        collector.graph(if starts == 0 {
            "Es fehlt der manuelle Start.".to_owned()
        } else {
            "Es darf nur einen manuellen Start geben.".to_owned()
        });
    }
    if !graph
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::Output))
        && !graph.nodes.is_empty()
    {
        collector.graph("Es fehlt eine Ausgabe.");
    }

    let by_id: HashMap<&str, &WorkflowNode> =
        graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut edge_ids = HashSet::new();
    let mut incoming: HashMap<(&str, &str), usize> = HashMap::new();
    let mut connected_inputs: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut valid_edges = Vec::new();

    for edge in &graph.edges {
        if !edge_ids.insert(edge.id.as_str()) {
            collector.edge(&edge.id, "Diese Kanten-ID kommt mehrfach vor.");
            continue;
        }
        let (Some(from), Some(to)) = (by_id.get(edge.from.as_str()), by_id.get(edge.to.as_str()))
        else {
            collector.edge(
                &edge.id,
                "Die Verbindung führt zu einem unbekannten Knoten.",
            );
            continue;
        };
        if edge.from == edge.to {
            collector.edge(
                &edge.id,
                "Ein Knoten darf nicht mit sich selbst verbunden sein.",
            );
            continue;
        }
        let Some(out) = output_ports(&from.kind)
            .iter()
            .find(|p| p.name == edge.from_port)
        else {
            collector.edge(&edge.id, "Der Ausgang existiert an diesem Knoten nicht.");
            continue;
        };
        let Some(inp) = input_ports(&to.kind)
            .iter()
            .find(|p| p.name == edge.to_port)
        else {
            collector.edge(&edge.id, "Der Eingang existiert an diesem Knoten nicht.");
            continue;
        };
        if out.kind != inp.kind {
            collector.edge(&edge.id, "Die Datenarten passen nicht zusammen.");
            continue;
        }
        let retry = is_retry_edge(graph, &edge.from, &edge.from_port);
        if retry && !matches!(to.kind, NodeKind::ExaSearch { .. }) {
            collector.edge(
                &edge.id,
                "Der Rückweg einer Bedingung muss zu einer Exa-Suche führen.",
            );
            continue;
        }
        if !retry {
            let count = incoming
                .entry((edge.to.as_str(), edge.to_port.as_str()))
                .or_insert(0);
            *count += 1;
            if *count > 1 {
                collector.edge(
                    &edge.id,
                    "In diesen Eingang darf nur eine Verbindung führen.",
                );
                continue;
            }
        }
        connected_inputs
            .entry(edge.to.as_str())
            .or_default()
            .insert(edge.to_port.as_str());
        valid_edges.push((edge, retry));
    }

    for node in &graph.nodes {
        let connected = connected_inputs.get(node.id.as_str());
        for spec in input_ports(&node.kind) {
            if spec.required && !connected.is_some_and(|c| c.contains(spec.name)) {
                collector.node(
                    &node.id,
                    format!("Der Eingang „{}“ ist nicht verbunden.", spec.name),
                );
            }
        }
        if matches!(
            node.kind,
            NodeKind::LocalModel { .. }
                | NodeKind::Output
                | NodeKind::Join
                | NodeKind::Merge { .. }
        ) && connected.is_none_or(HashSet::is_empty)
        {
            collector.node(&node.id, "Dieser Knoten hat keine eingehende Verbindung.");
        }
        if matches!(node.kind, NodeKind::Condition { .. })
            && !valid_edges
                .iter()
                .any(|(e, retry)| *retry && e.from == node.id)
        {
            collector.node(
                &node.id,
                "Die Bedingung braucht einen Rückweg („retry“) zur Exa-Suche.",
            );
        }
    }

    // Ohne Rückwege muss der Graph kreisfrei sein.
    if execution_order_of(graph, &valid_edges).is_none() {
        collector.graph(
            "Der Ablauf enthält einen Kreis. Wiederholungen sind nur über den Rückweg einer Bedingung erlaubt.",
        );
    }

    // Erreichbarkeit: alles muss vom Start erreichbar sein und zu einer Ausgabe führen.
    if let Some(start) = graph
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::ManualStart))
    {
        let forward = reach(&start.id, valid_edges.iter().map(|(e, _)| (&e.from, &e.to)));
        for node in &graph.nodes {
            if !forward.contains(node.id.as_str()) {
                collector.node(
                    &node.id,
                    "Dieser Knoten ist vom Start aus nicht erreichbar.",
                );
            }
        }
    }
    let outputs: Vec<&str> = graph
        .nodes
        .iter()
        .filter(|n| is_terminal(&n.kind))
        .map(|n| n.id.as_str())
        .collect();
    let mut backward: HashSet<&str> = outputs.iter().copied().collect();
    let mut queue: VecDeque<&str> = outputs.iter().copied().collect();
    while let Some(current) = queue.pop_front() {
        for (edge, _) in &valid_edges {
            if edge.to == current && backward.insert(edge.from.as_str()) {
                queue.push_back(edge.from.as_str());
            }
        }
    }
    for node in &graph.nodes {
        if !is_terminal(&node.kind) && !backward.contains(node.id.as_str()) {
            collector.node(
                &node.id,
                "Von diesem Knoten führt kein Weg zu einer Ausgabe.",
            );
        }
    }

    WorkflowValidation {
        ok: collector.problems.is_empty(),
        problems: collector.problems,
    }
}

fn reach<'a>(
    start: &'a str,
    edges: impl Iterator<Item = (&'a String, &'a String)> + Clone,
) -> HashSet<&'a str> {
    let mut seen: HashSet<&str> = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(current) = queue.pop_front() {
        for (from, to) in edges.clone() {
            if from == current && seen.insert(to.as_str()) {
                queue.push_back(to.as_str());
            }
        }
    }
    seen
}

fn execution_order_of(
    graph: &WorkflowGraph,
    edges: &[(&pa_types::flow::WorkflowEdge, bool)],
) -> Option<Vec<usize>> {
    let index: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    let mut indegree = vec![0_usize; graph.nodes.len()];
    let mut forward: Vec<Vec<usize>> = vec![Vec::new(); graph.nodes.len()];
    for (edge, retry) in edges {
        if *retry {
            continue;
        }
        if let (Some(&from), Some(&to)) =
            (index.get(edge.from.as_str()), index.get(edge.to.as_str()))
        {
            forward[from].push(to);
            indegree[to] += 1;
        }
    }
    let mut ready: VecDeque<usize> = (0..graph.nodes.len())
        .filter(|&i| indegree[i] == 0)
        .collect();
    let mut order = Vec::with_capacity(graph.nodes.len());
    while let Some(current) = ready.pop_front() {
        order.push(current);
        for &next in &forward[current] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                ready.push_back(next);
            }
        }
    }
    (order.len() == graph.nodes.len()).then_some(order)
}

/// Reihenfolge, in der der Runner Knoten prüft (ohne Rückwege topologisch sortiert).
/// Ein ungültiger Graph liefert die Knoten in gegebener Reihenfolge.
pub(crate) fn execution_order(graph: &WorkflowGraph) -> Vec<usize> {
    let edges: Vec<(&pa_types::flow::WorkflowEdge, bool)> = graph
        .edges
        .iter()
        .map(|e| (e, is_retry_edge(graph, &e.from, &e.from_port)))
        .collect();
    execution_order_of(graph, &edges).unwrap_or_else(|| (0..graph.nodes.len()).collect())
}

#[cfg(test)]
pub(crate) mod fixtures {
    use pa_types::flow::{WorkflowEdge, WorkflowGraph, WorkflowNode, WORKFLOW_SCHEMA_VERSION};

    use super::*;

    pub fn node(id: &str, kind: NodeKind) -> WorkflowNode {
        WorkflowNode {
            id: id.to_owned(),
            kind,
            x: 0.0,
            y: 0.0,
        }
    }

    pub fn edge(id: &str, from: &str, from_port: &str, to: &str, to_port: &str) -> WorkflowEdge {
        WorkflowEdge {
            id: id.to_owned(),
            from: from.to_owned(),
            from_port: from_port.to_owned(),
            to: to.to_owned(),
            to_port: to_port.to_owned(),
        }
    }

    /// Die Recherche mit Quellenprüfung: Start → Frage → Suche → Inhalte →
    /// Modell → Prüfung → Bedingung → Ausgabe, Rückweg zur Suche.
    pub fn research_graph(criteria: &[&str], max_iterations: u32) -> WorkflowGraph {
        WorkflowGraph {
            version: WORKFLOW_SCHEMA_VERSION,
            nodes: vec![
                node("start", NodeKind::ManualStart),
                node(
                    "input",
                    NodeKind::Input {
                        text: "Wie hoch ist der Mount Everest?".to_owned(),
                    },
                ),
                node("search", NodeKind::ExaSearch { num_results: 3 }),
                node(
                    "contents",
                    NodeKind::ExaContents {
                        max_characters: 2000,
                    },
                ),
                node(
                    "model",
                    NodeKind::LocalModel {
                        instruction: "Fasse die Quellen kurz zusammen.".to_owned(),
                    },
                ),
                node(
                    "check",
                    NodeKind::Check {
                        criteria: criteria.iter().map(|c| (*c).to_owned()).collect(),
                    },
                ),
                node("cond", NodeKind::Condition { max_iterations }),
                node("out", NodeKind::Output),
            ],
            edges: vec![
                edge("e1", "start", "out", "input", "trigger"),
                edge("e2", "input", "out", "search", "query"),
                edge("e3", "search", "results", "contents", "results"),
                edge("e4", "contents", "results", "model", "results"),
                edge("e5", "contents", "results", "check", "results"),
                edge("e6", "model", "out", "check", "answer"),
                edge("e7", "check", "verdict", "cond", "verdict"),
                edge("e8", "cond", "done", "out", "verdict"),
                edge("e9", "cond", "retry", "search", "query"),
                edge("e10", "model", "out", "out", "answer"),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    fn problems(graph: &WorkflowGraph) -> Vec<String> {
        validate(graph)
            .problems
            .into_iter()
            .map(|p| p.message)
            .collect()
    }

    #[test]
    fn the_research_pattern_is_valid() {
        let graph = research_graph(&["Höhe ist belegt"], 2);
        assert_eq!(problems(&graph), Vec::<String>::new());
        assert!(validate(&graph).ok);
    }

    #[test]
    fn mismatched_data_kinds_are_rejected() {
        let mut graph = research_graph(&["x"], 2);
        graph
            .edges
            .push(edge("bad", "search", "results", "search", "query"));
        // Selbstverbindung UND falsche Art: erste Regel greift.
        assert!(problems(&graph).iter().any(|m| m.contains("sich selbst")));
        graph.edges.pop();
        graph
            .edges
            .push(edge("bad2", "input", "out", "contents", "results"));
        assert!(problems(&graph).iter().any(|m| m.contains("Datenarten")));
    }

    #[test]
    fn two_connections_into_one_input_are_rejected_except_the_retry_edge() {
        let mut graph = research_graph(&["x"], 2);
        graph.nodes.push(node(
            "second",
            NodeKind::Input {
                text: "zweite Frage".to_owned(),
            },
        ));
        graph
            .edges
            .push(edge("e11", "start", "out", "second", "trigger"));
        graph
            .edges
            .push(edge("e12", "second", "out", "search", "query"));
        assert!(problems(&graph)
            .iter()
            .any(|m| m.contains("nur eine Verbindung")));
    }

    #[test]
    fn cycles_without_a_condition_are_rejected() {
        let mut graph = research_graph(&["x"], 2);
        // model.out -> model.text schließt einen Kreis ohne Bedingung.
        graph
            .edges
            .push(edge("loop", "model", "out", "model", "text"));
        let messages = problems(&graph);
        assert!(messages.iter().any(|m| m.contains("sich selbst")));
        // Ein echter Kreis über zwei Knoten: check.verdict kann nirgends zurück, daher über Modell.
        let mut graph = research_graph(&["x"], 2);
        graph.nodes.push(node(
            "m2",
            NodeKind::LocalModel {
                instruction: "Umformulieren".to_owned(),
            },
        ));
        graph.edges.push(edge("a", "model", "out", "m2", "text"));
        graph.edges.push(edge("b", "m2", "out", "model", "text"));
        assert!(problems(&graph).iter().any(|m| m.contains("Kreis")));
    }

    #[test]
    fn the_retry_edge_must_lead_to_a_search() {
        let mut graph = research_graph(&["x"], 2);
        graph.edges.retain(|e| e.id != "e9");
        graph
            .edges
            .push(edge("e9", "cond", "retry", "model", "text"));
        assert!(problems(&graph)
            .iter()
            .any(|m| m.contains("Rückweg einer Bedingung muss zu einer Exa-Suche")));
    }

    #[test]
    fn missing_start_output_and_unreachable_nodes_are_reported() {
        let mut graph = research_graph(&["x"], 2);
        graph.nodes.retain(|n| n.id != "start");
        graph.edges.retain(|e| e.from != "start");
        let messages = problems(&graph);
        assert!(messages.iter().any(|m| m.contains("manuelle Start")));
        assert!(messages.iter().any(|m| m.contains("nicht verbunden")));

        let mut graph = research_graph(&["x"], 2);
        graph.nodes.push(node("island", NodeKind::Output));
        assert!(problems(&graph)
            .iter()
            .any(|m| m.contains("nicht erreichbar") || m.contains("keine eingehende")));
    }

    #[test]
    fn parameters_are_bounded() {
        let mut graph = research_graph(&["x"], 9);
        assert!(problems(&graph)
            .iter()
            .any(|m| m.contains("Wiederholungen")));
        graph = research_graph(&[], 2);
        assert!(problems(&graph).iter().any(|m| m.contains("Kriterium")));
        graph = research_graph(&["x"], 2);
        if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == "search") {
            node.kind = NodeKind::ExaSearch { num_results: 50 };
        }
        assert!(problems(&graph).iter().any(|m| m.contains("Trefferzahl")));
        if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == "input") {
            node.kind = NodeKind::Input {
                text: "  ".to_owned(),
            };
        }
        assert!(problems(&graph).iter().any(|m| m.contains("leer")));
    }

    #[test]
    fn execution_order_ignores_retry_edges() {
        let graph = research_graph(&["x"], 2);
        let order = execution_order(&graph);
        let position = |id: &str| {
            let index = graph.nodes.iter().position(|n| n.id == id).unwrap();
            order.iter().position(|&i| i == index).unwrap()
        };
        assert!(position("search") < position("contents"));
        assert!(position("check") < position("cond"));
        assert!(position("cond") < position("out"));
    }

    #[test]
    fn new_block_parameters_are_bounded() {
        // Nur die Parameterprüfung des Knotens, ohne Anschlüsse und Erreichbarkeit.
        let bad = |kind: NodeKind| {
            let mut collector = Collector {
                problems: Vec::new(),
            };
            check_node(&mut collector, &node("extra", kind));
            !collector.problems.is_empty()
        };
        assert!(bad(NodeKind::RuntimeInput {
            label: " ".into(),
            public: false
        }));
        assert!(bad(NodeKind::Calendar {
            days_ahead: 0,
            include_tasks: true
        }));
        assert!(bad(NodeKind::Calendar {
            days_ahead: 99,
            include_tasks: true
        }));
        let mail = |from: &str, limit: u32| NodeKind::MailSearch {
            from: from.into(),
            subject: String::new(),
            unread_only: true,
            limit,
        };
        assert!(bad(mail("", 0)));
        assert!(bad(mail("", MAX_MAIL_RESULTS + 1)));
        assert!(bad(mail("a\nb", 3)));
        assert!(bad(mail(&"x".repeat(201), 3)));
        assert!(!bad(mail("anna@gmail.com", MAX_MAIL_RESULTS)));
        assert!(bad(NodeKind::MemorySearch { max_hits: 0 }));
        assert!(bad(NodeKind::Skill {
            skill_id: "../x".into()
        }));
        assert!(bad(NodeKind::Skill {
            skill_id: String::new()
        }));
        assert!(bad(NodeKind::Branch {
            rule: BranchRule::Contains { text: " ".into() }
        }));
        assert!(bad(NodeKind::Branch {
            rule: BranchRule::ModelYesNo {
                question: String::new()
            }
        }));
        assert!(bad(NodeKind::Merge {
            template: "{{x}}".into()
        }));
        assert!(bad(NodeKind::Merge {
            template: "{{a".into()
        }));
        assert!(bad(NodeKind::Notify {
            title: String::new()
        }));
        for path in [
            "",
            "/etc/passwd",
            "C:\\x.txt",
            "../x.txt",
            "a//b.txt",
            "~/x",
            "a/../b",
        ] {
            assert!(
                bad(NodeKind::Store {
                    target: StoreTarget::File {
                        relative_path: path.into()
                    }
                }),
                "{path:?} muss abgelehnt werden"
            );
        }
        assert!(!bad(NodeKind::Merge {
            template: "A {{a}} B {{ b }}".into()
        }));
        assert!(!bad(NodeKind::Store {
            target: StoreTarget::File {
                relative_path: "notizen/idee.md".into()
            }
        }));
    }

    #[test]
    fn notify_and_store_may_end_a_path_but_a_join_needs_an_input() {
        let mut graph = research_graph(&["x"], 2);
        // Ein Hinweis am Ende eines Zweigs ist ein gültiges Ende, auch ohne Weg zur Ausgabe.
        graph.nodes.push(node(
            "note",
            NodeKind::Notify {
                title: "Fertig".into(),
            },
        ));
        graph.edges.push(edge("en", "model", "out", "note", "text"));
        let report = validate(&graph);
        assert!(report.ok, "{:?}", report.problems);

        graph.nodes.push(node("join", NodeKind::Join));
        let report = validate(&graph);
        assert!(!report.ok);
        assert!(report
            .problems
            .iter()
            .any(|p| p.node_id.as_deref() == Some("join")));
    }

    #[test]
    fn branch_outputs_are_typed_text_and_each_goes_to_one_place_per_input() {
        assert_eq!(
            output_ports(&NodeKind::Branch {
                rule: BranchRule::Contains { text: "x".into() }
            })
            .len(),
            2
        );
        assert_eq!(input_ports(&NodeKind::Join).len(), 3);
        assert!(runs_on_any_input(&NodeKind::Join));
        assert!(!runs_on_any_input(&NodeKind::Merge {
            template: "{{a}}".into()
        }));
    }

    #[test]
    fn the_new_search_blocks_are_bounded_and_typed() {
        let bad = |kind: NodeKind| {
            let mut collector = Collector {
                problems: Vec::new(),
            };
            check_node(&mut collector, &node("extra", kind));
            !collector.problems.is_empty()
        };
        assert!(bad(NodeKind::WikipediaSearch {
            num_results: 0,
            lang: "de".into()
        }));
        assert!(bad(NodeKind::WikipediaSearch {
            num_results: 3,
            lang: "fr".into()
        }));
        assert!(!bad(NodeKind::WikipediaSearch {
            num_results: 3,
            lang: "en".into()
        }));
        assert!(bad(NodeKind::BraveSearch { num_results: 11 }));
        assert!(bad(NodeKind::Weather { days: 0 }));
        assert!(bad(NodeKind::Weather { days: 8 }));
        assert!(!bad(NodeKind::Weather { days: 7 }));
        assert_eq!(
            input_ports(&NodeKind::BraveSearch { num_results: 3 })[0].name,
            "query"
        );
        assert_eq!(
            output_ports(&NodeKind::Weather { days: 1 })[0].kind,
            DataKind::Text
        );
        assert_eq!(
            output_ports(&NodeKind::WikipediaSearch {
                num_results: 1,
                lang: "de".into()
            })[0]
                .kind,
            DataKind::Results
        );
    }
}
