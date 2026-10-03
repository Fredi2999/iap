// Vorlagen für häufige Aufträge. Sie liegen verschlüsselt im Tresor
// (Schlüssel `ui.templates`) und erscheinen im Chat im „/“-Menü.
import { loadUiState, saveUiState } from "./ipc";

export interface PromptTemplate {
  id: string;
  name: string;
  /** Text mit optionalem Platzhalter `{{text}}` für die eigene Eingabe. */
  text: string;
}

const KEY = "ui.templates";
export const PLACEHOLDER = "{{text}}";

export async function loadTemplates(): Promise<PromptTemplate[]> {
  const raw = await loadUiState(KEY);
  if (!raw) return [];
  const parsed: unknown = JSON.parse(raw);
  if (!Array.isArray(parsed)) return [];
  return parsed.filter((item): item is PromptTemplate =>
    typeof item === "object" && item !== null && typeof item.id === "string" && typeof item.name === "string" && typeof item.text === "string");
}

export function saveTemplates(templates: PromptTemplate[]): Promise<void> {
  return saveUiState(KEY, JSON.stringify(templates));
}

/** Befehlsname im „/“-Menü: klein, ohne Leerzeichen, damit er sich gut tippen lässt. */
export function commandName(name: string): string {
  const slug = name
    .trim()
    .toLocaleLowerCase()
    .replace(/\s+/g, "-")
    .replace(/[^\p{L}\p{N}_-]/gu, "")
    .slice(0, 32);
  return "/" + (slug || "vorlage");
}

/** Setzt eine Vorlage in die Eingabe; liefert Text und die Cursor-Position am Platzhalter. */
export function applyTemplate(text: string): { value: string; cursor: number } {
  const index = text.indexOf(PLACEHOLDER);
  if (index < 0) return { value: text, cursor: text.length };
  return { value: text.slice(0, index) + text.slice(index + PLACEHOLDER.length), cursor: index };
}
