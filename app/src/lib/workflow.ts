// Feste Knotenarten des Workflow-Builders: Anschlüsse, Vorlagen und Verbindungsregeln.
// Spiegel von crates/pa-core/src/workflow/validate.rs. Maßgeblich ist immer die
// Prüfung im Backend (workflowValidate); die Regeln hier verhindern nur offensichtlich
// falsche Verbindungen beim Zeichnen.

import { tk } from "./i18n/index.svelte";
import type { DataKind, NodeKind, WorkflowGraph, WorkflowNode } from "./types";

export type NodeType = NodeKind["type"];

export interface PortDef {
  name: string;
  kind: DataKind;
  /** Deutscher Anzeigetext (wird mit t() übersetzt). */
  label: string;
}

export const NODE_WIDTH = 224;
export const HEADER_HEIGHT = 40;
export const ROW_HEIGHT = 24;
export const BODY_PADDING = 8;

export type NodeGroup = "input" | "web" | "process" | "flow" | "result";

export const NODE_TYPES: { type: NodeType; title: string; hint: string; group: NodeGroup }[] = [
  { type: "manual_start", group: "input", title: tk("Manueller Start"), hint: tk("Der Ablauf beginnt, wenn du ihn startest.") },
  { type: "input", group: "input", title: tk("Eingabe"), hint: tk("Fester Text im Ablauf. Geht er an Exa, ist er eine öffentliche Suchfrage.") },
  { type: "runtime_input", group: "input", title: tk("Eingabe beim Start"), hint: tk("Du tippst den Text erst beim Start des Laufs ein.") },
  { type: "calendar", group: "input", title: tk("Kalender"), hint: tk("Termine und offene Aufgaben der nächsten Tage als Text. Bleibt privat.") },
  { type: "mail_search", group: "input", title: tk("Mails"), hint: tk("Sucht im eigenen Gmail-Postfach (nur lesend). Der Text bleibt privat.") },
  { type: "memory_search", group: "input", title: tk("Gedächtnissuche"), hint: tk("Sucht passende Erinnerungen. Bleibt privat.") },
  { type: "exa_search", group: "web", title: tk("Exa-Suche"), hint: tk("Websuche über Exa (nur mit Freigabe je Lauf).") },
  { type: "wikipedia_search", group: "web", title: tk("Wikipedia-Suche"), hint: tk("Sucht Artikel bei Wikipedia (kostenlos, ohne Schlüssel).") },
  { type: "brave_search", group: "web", title: tk("Brave-Suche"), hint: tk("Websuche über Brave (eigener Schlüssel, nur mit Freigabe je Lauf).") },
  { type: "weather", group: "web", title: tk("Wetter"), hint: tk("Wetter für einen öffentlich eingegebenen Ort (Open-Meteo, kostenlos).") },
  { type: "exa_contents", group: "web", title: tk("Exa-Seiteninhalte"), hint: tk("Holt den Text der gefundenen Seiten.") },
  { type: "check", group: "web", title: tk("Prüfung"), hint: tk("Prüft, ob die Quellen deine Kriterien belegen.") },
  { type: "condition", group: "web", title: tk("Bedingung"), hint: tk("Sucht bei Lücken erneut, begrenzt.") },
  { type: "local_model", group: "process", title: tk("Lokales Modell"), hint: tk("Fasst zusammen, in getrenntem Kontext ohne Chat und Dateien.") },
  { type: "skill", group: "process", title: tk("Skill"), hint: tk("Wendet eine Anleitung (SKILL.md) mit dem lokalen Modell auf den Text an.") },
  { type: "merge", group: "process", title: tk("Vorlage"), hint: tk("Setzt bis zu drei Texte in einen Mustertext ein.") },
  { type: "branch", group: "flow", title: tk("Verzweigung"), hint: tk("Geht je nach Text oder Modellurteil nach „ja“ oder „nein“ weiter.") },
  { type: "join", group: "flow", title: tk("Zusammenführen"), hint: tk("Nimmt den Text, der aus einem der Zweige ankommt.") },
  { type: "output", group: "result", title: tk("Ausgabe"), hint: tk("Ergebnis mit Quellen und offenen Punkten.") },
  { type: "notify", group: "result", title: tk("Hinweis"), hint: tk("Zeigt am Ende des Laufs einen Hinweis.") },
  { type: "store", group: "result", title: tk("Ablegen"), hint: tk("Schlägt vor, den Text zu speichern. Geschrieben wird erst nach deiner Bestätigung.") },
];

export const NODE_GROUPS: { id: NodeGroup; title: string }[] = [
  { id: "input", title: tk("Quellen") },
  { id: "web", title: tk("Web") },
  { id: "process", title: tk("Verarbeiten") },
  { id: "flow", title: tk("Ablauf") },
  { id: "result", title: tk("Ergebnis") },
];

export function nodeTitle(kind: NodeKind): string {
  return NODE_TYPES.find((n) => n.type === kind.type)?.title ?? kind.type;
}

const IN: Record<NodeType, PortDef[]> = {
  manual_start: [],
  input: [{ name: "trigger", kind: "signal", label: tk("Start") }],
  exa_search: [{ name: "query", kind: "text", label: tk("Suchfrage") }],
  exa_contents: [{ name: "results", kind: "results", label: tk("Treffer") }],
  local_model: [
    { name: "text", kind: "text", label: tk("Text") },
    { name: "results", kind: "results", label: tk("Treffer") },
  ],
  check: [
    { name: "results", kind: "results", label: tk("Treffer") },
    { name: "answer", kind: "text", label: tk("Antwort (optional)") },
  ],
  condition: [{ name: "verdict", kind: "verdict", label: tk("Prüfung") }],
  output: [
    { name: "answer", kind: "text", label: tk("Antwort") },
    { name: "verdict", kind: "verdict", label: tk("Prüfung") },
  ],
  runtime_input: [{ name: "trigger", kind: "signal", label: tk("Start") }],
  calendar: [{ name: "trigger", kind: "signal", label: tk("Start") }],
  mail_search: [{ name: "trigger", kind: "signal", label: tk("Start") }],
  memory_search: [{ name: "query", kind: "text", label: tk("Suchtext") }],
  skill: [{ name: "text", kind: "text", label: tk("Text") }],
  branch: [{ name: "text", kind: "text", label: tk("Text") }],
  join: [
    { name: "a", kind: "text", label: tk("Zweig A") },
    { name: "b", kind: "text", label: tk("Zweig B") },
    { name: "c", kind: "text", label: tk("Zweig C") },
  ],
  merge: [
    { name: "a", kind: "text", label: tk("Text A") },
    { name: "b", kind: "text", label: tk("Text B") },
    { name: "c", kind: "text", label: tk("Text C") },
  ],
  notify: [{ name: "text", kind: "text", label: tk("Text") }],
  store: [{ name: "text", kind: "text", label: tk("Text") }],
  wikipedia_search: [{ name: "query", kind: "text", label: tk("Suchfrage") }],
  brave_search: [{ name: "query", kind: "text", label: tk("Suchfrage") }],
  weather: [{ name: "query", kind: "text", label: tk("Ort") }],
};

const OUT: Record<NodeType, PortDef[]> = {
  manual_start: [{ name: "out", kind: "signal", label: tk("Start") }],
  input: [{ name: "out", kind: "text", label: tk("Text") }],
  exa_search: [{ name: "results", kind: "results", label: tk("Treffer") }],
  exa_contents: [{ name: "results", kind: "results", label: tk("Treffer + Inhalte") }],
  local_model: [{ name: "out", kind: "text", label: tk("Antwort") }],
  check: [{ name: "verdict", kind: "verdict", label: tk("Ergebnis") }],
  condition: [
    { name: "done", kind: "verdict", label: tk("Erledigt") },
    { name: "retry", kind: "text", label: tk("Nochmal suchen") },
  ],
  output: [],
  runtime_input: [{ name: "out", kind: "text", label: tk("Text") }],
  calendar: [{ name: "out", kind: "text", label: tk("Text") }],
  mail_search: [{ name: "out", kind: "text", label: tk("Text") }],
  memory_search: [{ name: "out", kind: "text", label: tk("Treffer als Text") }],
  skill: [{ name: "out", kind: "text", label: tk("Ergebnis") }],
  branch: [
    { name: "yes", kind: "text", label: tk("Ja") },
    { name: "no", kind: "text", label: tk("Nein") },
  ],
  join: [{ name: "out", kind: "text", label: tk("Text") }],
  merge: [{ name: "out", kind: "text", label: tk("Text") }],
  notify: [],
  store: [],
  wikipedia_search: [{ name: "results", kind: "results", label: tk("Treffer") }],
  brave_search: [{ name: "results", kind: "results", label: tk("Treffer") }],
  weather: [{ name: "out", kind: "text", label: tk("Wetter als Text") }],
};

export const inputPorts = (kind: NodeKind): PortDef[] => IN[kind.type];
export const outputPorts = (kind: NodeKind): PortDef[] => OUT[kind.type];

export function portY(node: WorkflowNode, side: "in" | "out", name: string): number {
  const ports = side === "in" ? inputPorts(node.kind) : outputPorts(node.kind);
  const index = Math.max(0, ports.findIndex((p) => p.name === name));
  return node.y + HEADER_HEIGHT + BODY_PADDING + index * ROW_HEIGHT + ROW_HEIGHT / 2;
}

export function nodeHeight(node: WorkflowNode): number {
  const rows = Math.max(inputPorts(node.kind).length, outputPorts(node.kind).length, 1);
  return HEADER_HEIGHT + BODY_PADDING * 2 + rows * ROW_HEIGHT + 22;
}

export function newId(prefix: string): string {
  const bytes = new Uint8Array(6);
  crypto.getRandomValues(bytes);
  return `${prefix}-${[...bytes].map((b) => b.toString(16).padStart(2, "0")).join("")}`;
}

export function defaultKind(type: NodeType): NodeKind {
  switch (type) {
    case "manual_start": return { type };
    case "input": return { type, text: "" };
    case "exa_search": return { type, num_results: 5 };
    case "exa_contents": return { type, max_characters: 3000 };
    case "local_model": return { type, instruction: "Fasse die Quellen sachlich zusammen und nenne nur, was die Quellen belegen." };
    case "check": return { type, criteria: [""] };
    case "condition": return { type, max_iterations: 2 };
    case "output": return { type };
    case "runtime_input": return { type, label: "Thema", public: false };
    case "calendar": return { type, days_ahead: 2, include_tasks: true };
    case "mail_search": return { type, from: "", subject: "", unread_only: true, limit: 5 };
    case "memory_search": return { type, max_hits: 5 };
    case "skill": return { type, skill_id: "" };
    case "branch": return { type, rule: { kind: "contains", text: "" } };
    case "join": return { type };
    case "merge": return { type, template: "{{a}}\n\n{{b}}" };
    case "notify": return { type, title: "Ablauf fertig" };
    case "store": return { type, target: { kind: "memory" } };
    case "wikipedia_search": return { type, num_results: 3, lang: "de" };
    case "brave_search": return { type, num_results: 5 };
    case "weather": return { type, days: 3 };
  }
}

export function newNode(type: NodeType, x: number, y: number): WorkflowNode {
  return { id: newId("n"), kind: defaultKind(type), x: Math.max(0, Math.round(x)), y: Math.max(0, Math.round(y)) };
}

function isRetry(graph: WorkflowGraph, from: string, fromPort: string): boolean {
  return fromPort === "retry" && graph.nodes.find((n) => n.id === from)?.kind.type === "condition";
}

/** Prüft eine geplante Verbindung. `null` = erlaubt, sonst der Grund (deutsch). */
export function connectProblem(
  graph: WorkflowGraph,
  from: string,
  fromPort: string,
  to: string,
  toPort: string,
): string | null {
  if (from === to) return tk("Ein Knoten kann nicht mit sich selbst verbunden werden.");
  const a = graph.nodes.find((n) => n.id === from);
  const b = graph.nodes.find((n) => n.id === to);
  if (!a || !b) return tk("Unbekannter Knoten.");
  const out = outputPorts(a.kind).find((p) => p.name === fromPort);
  const inp = inputPorts(b.kind).find((p) => p.name === toPort);
  if (!out || !inp) return tk("Dieser Anschluss existiert nicht.");
  if (out.kind !== inp.kind) return tk("Die Datenarten passen nicht zusammen.");
  const retry = isRetry(graph, from, fromPort);
  if (retry && b.kind.type !== "exa_search") return tk("Der Rückweg einer Bedingung muss zu einer Exa-Suche führen.");
  if (!retry && graph.edges.some((e) => e.to === to && e.to_port === toPort && !isRetry(graph, e.from, e.from_port))) {
    return tk("In diesen Eingang führt schon eine Verbindung.");
  }
  if (graph.edges.some((e) => e.from === from && e.from_port === fromPort && e.to === to && e.to_port === toPort)) {
    return tk("Diese Verbindung gibt es schon.");
  }
  return null;
}

/** Vorlage „Recherche mit Quellenprüfung“. Suchfrage und Kriterien schreibt der Nutzer selbst. */
export function researchTemplate(): WorkflowGraph {
  const n = (id: string, kind: NodeKind, x: number, y: number): WorkflowNode => ({ id, kind, x, y });
  return {
    version: 1,
    nodes: [
      n("start", { type: "manual_start" }, 20, 70),
      n("input", { type: "input", text: "" }, 290, 70),
      n("search", { type: "exa_search", num_results: 5 }, 560, 70),
      n("contents", { type: "exa_contents", max_characters: 3000 }, 830, 70),
      n("model", defaultKind("local_model"), 1100, 10),
      n("check", { type: "check", criteria: [""] }, 1100, 190),
      n("cond", { type: "condition", max_iterations: 2 }, 1370, 190),
      n("out", { type: "output" }, 1370, 10),
    ],
    edges: [
      { id: "e1", from: "start", from_port: "out", to: "input", to_port: "trigger" },
      { id: "e2", from: "input", from_port: "out", to: "search", to_port: "query" },
      { id: "e3", from: "search", from_port: "results", to: "contents", to_port: "results" },
      { id: "e4", from: "contents", from_port: "results", to: "model", to_port: "results" },
      { id: "e5", from: "contents", from_port: "results", to: "check", to_port: "results" },
      { id: "e6", from: "model", from_port: "out", to: "check", to_port: "answer" },
      { id: "e7", from: "check", from_port: "verdict", to: "cond", to_port: "verdict" },
      { id: "e8", from: "cond", from_port: "done", to: "out", to_port: "verdict" },
      { id: "e9", from: "cond", from_port: "retry", to: "search", to_port: "query" },
      { id: "e10", from: "model", from_port: "out", to: "out", to_port: "answer" },
    ],
  };
}

/** Vorlage „Text lokal bearbeiten“: ohne Internet, nur das lokale Modell. */
export function localTemplate(): WorkflowGraph {
  const n = (id: string, kind: NodeKind, x: number, y: number): WorkflowNode => ({ id, kind, x, y });
  return {
    version: 1,
    nodes: [
      n("start", { type: "manual_start" }, 20, 40),
      n("input", { type: "input", text: "" }, 290, 40),
      n("model", { type: "local_model", instruction: "Fasse den Text in drei Sätzen zusammen." }, 560, 40),
      n("out", { type: "output" }, 830, 40),
    ],
    edges: [
      { id: "e1", from: "start", from_port: "out", to: "input", to_port: "trigger" },
      { id: "e2", from: "input", from_port: "out", to: "model", to_port: "text" },
      { id: "e3", from: "model", from_port: "out", to: "out", to_port: "answer" },
    ],
  };
}

/** Vorlage „Tagesüberblick“: Termine und Aufgaben lokal zusammenfassen. Kein Internet. */
export function dayTemplate(): WorkflowGraph {
  const n = (id: string, kind: NodeKind, x: number, y: number): WorkflowNode => ({ id, kind, x, y });
  return {
    version: 1,
    nodes: [
      n("start", { type: "manual_start" }, 20, 40),
      n("cal", { type: "calendar", days_ahead: 2, include_tasks: true }, 290, 40),
      n("model", { type: "local_model", instruction: "Fasse meine Termine und Aufgaben als kurzen Tagesplan zusammen. Nenne zuerst, was am wichtigsten ist, und weise auf Überschneidungen hin." }, 560, 40),
      n("out", { type: "output" }, 830, 10),
      n("note", { type: "notify", title: "Dein Tagesüberblick ist fertig" }, 830, 150),
    ],
    edges: [
      { id: "e1", from: "start", from_port: "out", to: "cal", to_port: "trigger" },
      { id: "e2", from: "cal", from_port: "out", to: "model", to_port: "text" },
      { id: "e3", from: "model", from_port: "out", to: "out", to_port: "answer" },
      { id: "e4", from: "model", from_port: "out", to: "note", to_port: "text" },
    ],
  };
}

/** Vorlage „Frage mit Gedächtnis“: Frage beim Start, passende Erinnerungen, Antwort, Vorschlag zum Merken. */
export function memoryTemplate(): WorkflowGraph {
  const n = (id: string, kind: NodeKind, x: number, y: number): WorkflowNode => ({ id, kind, x, y });
  return {
    version: 1,
    nodes: [
      n("start", { type: "manual_start" }, 20, 60),
      n("in", { type: "runtime_input", label: "Deine Frage", public: false }, 290, 60),
      n("mem", { type: "memory_search", max_hits: 5 }, 560, 160),
      n("merge", { type: "merge", template: "Frage:\n{{a}}\n\nBekannte Notizen:\n{{b}}" }, 830, 60),
      n("model", { type: "local_model", instruction: "Beantworte die Frage. Nutze die bekannten Notizen, wenn sie passen, und sage, wenn sie nichts dazu enthalten." }, 1100, 60),
      n("out", { type: "output" }, 1370, 10),
      n("store", { type: "store", target: { kind: "memory" } }, 1370, 150),
    ],
    edges: [
      { id: "e1", from: "start", from_port: "out", to: "in", to_port: "trigger" },
      { id: "e2", from: "in", from_port: "out", to: "mem", to_port: "query" },
      { id: "e3", from: "in", from_port: "out", to: "merge", to_port: "a" },
      { id: "e4", from: "mem", from_port: "out", to: "merge", to_port: "b" },
      { id: "e5", from: "merge", from_port: "out", to: "model", to_port: "text" },
      { id: "e6", from: "model", from_port: "out", to: "out", to_port: "answer" },
      { id: "e7", from: "model", from_port: "out", to: "store", to_port: "text" },
    ],
  };
}

/** Vorlage „Nachricht sortieren“: Eilige bekommen eine Kurzantwort, der Rest wird als Aufgabe vorgeschlagen. */
export function triageTemplate(): WorkflowGraph {
  const n = (id: string, kind: NodeKind, x: number, y: number): WorkflowNode => ({ id, kind, x, y });
  return {
    version: 1,
    nodes: [
      n("start", { type: "manual_start" }, 20, 90),
      n("in", { type: "runtime_input", label: "Nachricht", public: false }, 290, 90),
      n("branch", { type: "branch", rule: { kind: "model_yes_no", question: "Verlangt die Nachricht eine schnelle Antwort?" } }, 560, 90),
      n("quick", { type: "local_model", instruction: "Schreibe eine kurze, freundliche Antwort auf die Nachricht." }, 830, 10),
      n("later", { type: "local_model", instruction: "Fasse die Nachricht in einem Satz als Aufgabe zusammen." }, 830, 190),
      n("join", { type: "join" }, 1100, 90),
      n("out", { type: "output" }, 1370, 90),
      n("store", { type: "store", target: { kind: "task" } }, 1100, 250),
    ],
    edges: [
      { id: "e1", from: "start", from_port: "out", to: "in", to_port: "trigger" },
      { id: "e2", from: "in", from_port: "out", to: "branch", to_port: "text" },
      { id: "e3", from: "branch", from_port: "yes", to: "quick", to_port: "text" },
      { id: "e4", from: "branch", from_port: "no", to: "later", to_port: "text" },
      { id: "e5", from: "quick", from_port: "out", to: "join", to_port: "a" },
      { id: "e6", from: "later", from_port: "out", to: "join", to_port: "b" },
      { id: "e7", from: "join", from_port: "out", to: "out", to_port: "answer" },
      { id: "e8", from: "later", from_port: "out", to: "store", to_port: "text" },
    ],
  };
}

/** Vorlage „Recherche kostenlos“: Wikipedia statt Exa, ohne Schlüssel. */
export function wikiTemplate(): WorkflowGraph {
  const n = (id: string, kind: NodeKind, x: number, y: number): WorkflowNode => ({ id, kind, x, y });
  return {
    version: 1,
    nodes: [
      n("start", { type: "manual_start" }, 20, 40),
      n("in", { type: "runtime_input", label: "Suchbegriff", public: true }, 290, 40),
      n("wiki", { type: "wikipedia_search", num_results: 3, lang: "de" }, 560, 40),
      n("model", { type: "local_model", instruction: "Beantworte die Frage sachlich nur aus den Treffern und nenne die Artikel." }, 830, 40),
      n("out", { type: "output" }, 1100, 40),
    ],
    edges: [
      { id: "e1", from: "start", from_port: "out", to: "in", to_port: "trigger" },
      { id: "e2", from: "in", from_port: "out", to: "wiki", to_port: "query" },
      { id: "e3", from: "wiki", from_port: "results", to: "model", to_port: "results" },
      { id: "e4", from: "model", from_port: "out", to: "out", to_port: "answer" },
    ],
  };
}

/** Vorlage „Wetter im Tagesüberblick“: Ort beim Start, Kalender plus Wetter zusammen zum Tagesplan. */
export function weatherDayTemplate(): WorkflowGraph {
  const n = (id: string, kind: NodeKind, x: number, y: number): WorkflowNode => ({ id, kind, x, y });
  return {
    version: 1,
    nodes: [
      n("start", { type: "manual_start" }, 20, 90),
      n("place", { type: "runtime_input", label: "Ort", public: true }, 290, 190),
      n("cal", { type: "calendar", days_ahead: 2, include_tasks: true }, 290, 10),
      n("weather", { type: "weather", days: 2 }, 560, 190),
      n("merge", { type: "merge", template: "Termine und Aufgaben:\n{{a}}\n\nWetter:\n{{b}}" }, 830, 90),
      n("model", { type: "local_model", instruction: "Mache daraus einen kurzen Tagesplan. Berücksichtige das Wetter bei Terminen im Freien." }, 1100, 90),
      n("out", { type: "output" }, 1370, 90),
    ],
    edges: [
      { id: "e1", from: "start", from_port: "out", to: "cal", to_port: "trigger" },
      { id: "e2", from: "start", from_port: "out", to: "place", to_port: "trigger" },
      { id: "e3", from: "place", from_port: "out", to: "weather", to_port: "query" },
      { id: "e4", from: "cal", from_port: "out", to: "merge", to_port: "a" },
      { id: "e5", from: "weather", from_port: "out", to: "merge", to_port: "b" },
      { id: "e6", from: "merge", from_port: "out", to: "model", to_port: "text" },
      { id: "e7", from: "model", from_port: "out", to: "out", to_port: "answer" },
    ],
  };
}

/** Eingaben beim Start (Knoten-ID, Beschriftung, öffentlich?). */
export function runtimeInputs(graph: WorkflowGraph): { id: string; label: string; public: boolean }[] {
  return graph.nodes.flatMap((n) => (n.kind.type === "runtime_input" ? [{ id: n.id, label: n.kind.label, public: n.kind.public }] : []));
}

export type ExternalService = "exa" | "wikipedia" | "brave" | "open_meteo";

/** Welche externen Dienste der Ablauf braucht (ohne Wiederholung). */
export function externalServices(graph: WorkflowGraph): ExternalService[] {
  const found: ExternalService[] = [];
  const add = (service: ExternalService) => { if (!found.includes(service)) found.push(service); };
  for (const node of graph.nodes) {
    switch (node.kind.type) {
      case "exa_search": case "exa_contents": add("exa"); break;
      case "wikipedia_search": add("wikipedia"); break;
      case "brave_search": add("brave"); break;
      case "weather": add("open_meteo"); break;
    }
  }
  return found;
}

export const SERVICE_NAME: Record<ExternalService, string> = {
  exa: "Exa",
  wikipedia: "Wikipedia",
  brave: "Brave Search",
  open_meteo: "Open-Meteo (Wetter)",
};

/** Ob der Ablauf irgendeinen externen Dienst braucht. */
export function usesExa(graph: WorkflowGraph): boolean {
  return externalServices(graph).length > 0;
}

export function nodeSummary(node: WorkflowNode): string {
  const k = node.kind;
  switch (k.type) {
    case "input": return k.text.trim() ? k.text.trim().slice(0, 60) : "";
    case "exa_search": return `${k.num_results}`;
    case "exa_contents": return `${k.max_characters}`;
    case "local_model": return k.instruction.trim().slice(0, 60);
    case "check": return `${k.criteria.filter((c) => c.trim()).length}`;
    case "condition": return `${k.max_iterations}`;
    case "runtime_input": return k.label.trim().slice(0, 40);
    case "calendar": return `${k.days_ahead}`;
    case "mail_search": return k.from.trim() || `${k.limit}`;
    case "memory_search": return `${k.max_hits}`;
    case "skill": return k.skill_id;
    case "branch": return k.rule.kind === "contains" ? k.rule.text.trim().slice(0, 40) : k.rule.question.trim().slice(0, 40);
    case "merge": return k.template.trim().replace(/\s+/g, " ").slice(0, 40);
    case "notify": return k.title.trim().slice(0, 40);
    case "store": return k.target.kind === "file" ? k.target.relative_path : k.target.kind === "memory" ? tk("Gedächtnis") : tk("Aufgabe");
    case "wikipedia_search": return `${k.lang} · ${k.num_results}`;
    case "brave_search": return `${k.num_results}`;
    case "weather": return `${k.days}`;
    default: return "";
  }
}
