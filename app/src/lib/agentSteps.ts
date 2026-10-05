// Darstellung des Code-Agenten: Schritte verständlich beschriften und den Verlauf gruppieren.
// Reine Funktionen ohne Oberfläche, damit sie sich ohne Fenster prüfen lassen
// (`node --experimental-strip-types --test scripts/agentSteps.test.ts`).

/** Eine Zeile im Verlauf des Agenten. */
export interface AgentLine {
  kind: "user" | "step" | "answer" | "error";
  text: string;
  /** Werkzeug und Argumente eines Schritts, getrennt, damit die Anzeige sie lesbar machen kann. */
  tool?: string;
  args?: string;
  /** Kurze Vorschau des Ergebnisses eines Schritts. */
  detail?: string;
}

export type AgentBlock =
  | { kind: "user"; text: string }
  | { kind: "steps"; steps: AgentLine[]; open: boolean }
  | { kind: "answer"; text: string }
  | { kind: "error"; text: string };

/** Hinweis, wenn das Modell statt einer Antwort einen kaputten Werkzeugaufruf als Text liefert. */
export const ENVELOPE_ERROR_TEXT = "Das Modell hat keinen gültigen Werkzeugaufruf geliefert. Versuche es noch einmal oder formuliere die Aufgabe anders.";

const LABELS: Record<string, string> = {
  list_dir: "Ordner ansehen",
  read_file: "Datei lesen",
  search: "Suchen",
  propose_edit: "Änderung vorgeschlagen",
  run_command: "Befehl",
};

/** Deutsche Beschriftung eines Werkzeugs (der Aufrufer übersetzt mit `t`). */
export function stepLabel(tool: string): string {
  return LABELS[tool] ?? tool;
}

function parsed(args: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(args);
    return value !== null && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** Liest ein Textfeld auch aus abgeschnittenem JSON (die Vorschau ist auf 240 Zeichen gekürzt). */
function loose(args: string, key: string): string | null {
  const match = new RegExp(`"${key}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)`).exec(args);
  return match ? match[1].replace(/\\"/g, '"').replace(/\\\\/g, "\\") : null;
}

function field(args: string, key: string): string | null {
  const value = parsed(args)?.[key];
  return typeof value === "string" ? value : loose(args, key);
}

function oneLine(text: string, max = 120): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max)} …` : flat;
}

/** Worauf sich ein Schritt bezieht: Pfad, Suchbegriff oder Befehl. */
export function stepTarget(tool: string, args: string): string {
  switch (tool) {
    case "list_dir":
      return field(args, "path") ?? ".";
    case "read_file":
    case "propose_edit":
      return field(args, "path") ?? oneLine(args);
    case "search":
      return field(args, "query") ?? oneLine(args);
    case "run_command": {
      const program = field(args, "program");
      if (!program) return oneLine(args);
      const list = parsed(args)?.args;
      const rest = Array.isArray(list) ? list.filter((item): item is string => typeof item === "string") : [];
      return oneLine([program, ...rest].join(" "));
    }
    default:
      return args.trim() === "{}" ? "" : oneLine(args);
  }
}

/** Teilt einen Pfad in Ordner (mit abschließendem Schrägstrich) und Dateinamen. */
export function splitPath(path: string): { dir: string; name: string } {
  const normal = path.replace(/\\/g, "/");
  const cut = normal.lastIndexOf("/");
  return cut < 0 ? { dir: "", name: normal } : { dir: normal.slice(0, cut + 1), name: normal.slice(cut + 1) };
}

function looksLikeEnvelope(text: string): boolean {
  const trimmed = text.trim();
  return trimmed.startsWith("{") && trimmed.includes('"action"');
}

/**
 * Fasst aufeinanderfolgende Schritte zu einer Gruppe zusammen. Die letzte Gruppe ist nur offen,
 * solange der Agent arbeitet; danach genügt eine Zeile „n Schritte“ mit Aufklappen.
 */
export function groupAgentLines(lines: readonly AgentLine[], running = false): AgentBlock[] {
  const blocks: AgentBlock[] = [];
  for (const line of lines) {
    if (line.kind === "step") {
      const last = blocks[blocks.length - 1];
      if (last?.kind === "steps") last.steps.push(line);
      else blocks.push({ kind: "steps", steps: [line], open: false });
    } else if (line.kind === "answer" && (line.text === ENVELOPE_ERROR_TEXT || looksLikeEnvelope(line.text))) {
      blocks.push({ kind: "error", text: ENVELOPE_ERROR_TEXT });
    } else {
      blocks.push({ kind: line.kind, text: line.text });
    }
  }
  const last = blocks[blocks.length - 1];
  if (running && last?.kind === "steps") last.open = true;
  return blocks;
}
