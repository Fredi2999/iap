// Zustand des Code-Agenten. Er liegt außerhalb der Seite, damit Verlauf und laufende Anfrage
// einen Wechsel zu Chat oder Start überstehen. Das Ereignis-Abo wird einmal angelegt.

import type { UnlistenFn } from "@tauri-apps/api/event";

import { errorText } from "../errors";
import { codeAgentChanges, codeAgentCommandRespond, codeAgentDiscard, codeAgentReset, codeAgentSend, onCodeAgentCommand, onCodeAgentEvent } from "../ipc";
import type { AgentChange, AgentEvent, CommandPrompt } from "../types";

export interface AgentLine {
  kind: "user" | "step" | "answer" | "error";
  text: string;
  /** Kurze Vorschau des Ergebnisses eines Schritts. */
  detail?: string;
}

export const agent = $state<{
  lines: AgentLine[];
  running: boolean;
  step: number;
  changes: AgentChange[];
  /** Ein Befehl wartet auf deine Bestätigung. */
  pendingCommand: CommandPrompt | null;
}>({ lines: [], running: false, step: 0, changes: [], pendingCommand: null });

let unlisten: Promise<UnlistenFn> | null = null;
let unlistenCommand: Promise<UnlistenFn> | null = null;

export async function refreshChanges(): Promise<void> {
  try {
    agent.changes = await codeAgentChanges();
  } catch {
    // Die Liste bleibt, wie sie war; der nächste Ereignis- oder Seitenwechsel liest neu.
  }
}

function handle(event: AgentEvent): void {
  switch (event.kind) {
    case "working":
      agent.step = event.step;
      break;
    case "tool_call":
      agent.lines.push({ kind: "step", text: `${event.tool} ${event.arguments}` });
      break;
    case "tool_result": {
      const last = agent.lines[agent.lines.length - 1];
      if (last?.kind === "step") last.detail = event.preview;
      break;
    }
    case "tool_error":
      agent.lines.push({ kind: "error", text: event.message });
      break;
    case "done":
      agent.running = false;
      agent.pendingCommand = null;
      agent.lines.push({ kind: "answer", text: event.text });
      void refreshChanges();
      break;
    case "failed":
      agent.running = false;
      agent.pendingCommand = null;
      agent.lines.push({ kind: "error", text: event.message });
      void refreshChanges();
      break;
    case "cancelled":
      agent.running = false;
      agent.pendingCommand = null;
      void refreshChanges();
      break;
  }
}

/** Legt das Abo an (einmal) und liest die offenen Vorschläge. */
export function ensureAgentListener(): void {
  if (!unlisten) unlisten = onCodeAgentEvent(handle);
  if (!unlistenCommand) unlistenCommand = onCodeAgentCommand((prompt) => (agent.pendingCommand = prompt));
  void refreshChanges();
}

export async function sendToAgent(text: string, activeFile: string | null): Promise<void> {
  agent.lines.push({ kind: "user", text });
  agent.running = true;
  agent.step = 0;
  try {
    await codeAgentSend(text, activeFile);
  } catch (reason) {
    agent.running = false;
    agent.lines.push({ kind: "error", text: errorText(reason) });
  }
}

/** Antwort auf den Befehlsdialog; der Dialog schließt in jedem Fall. */
export async function answerCommand(allow: boolean): Promise<void> {
  const prompt = agent.pendingCommand;
  agent.pendingCommand = null;
  if (!prompt) return;
  try {
    await codeAgentCommandRespond(prompt.id, allow);
  } catch (reason) {
    agent.lines.push({ kind: "error", text: errorText(reason) });
  }
}

/** Neue Aufgabe: Verlauf und Vorschläge verwerfen. */
export async function resetAgent(): Promise<void> {
  await codeAgentReset();
  agent.lines.splice(0);
  agent.changes = [];
}

/** Verwirft einen Vorschlag (nach dem Speichern oder Ablehnen). */
export async function discardChange(path: string): Promise<void> {
  try {
    await codeAgentDiscard(path);
  } finally {
    await refreshChanges();
  }
}

/** Nach einem Ordnerwechsel gehört nichts mehr vom alten Ordner in den Verlauf. */
export function clearAgentView(): void {
  agent.lines.splice(0);
  agent.changes = [];
  agent.running = false;
  agent.pendingCommand = null;
}
