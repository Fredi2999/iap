// Offene Dateien des Code-Bereichs. Der Zustand liegt außerhalb der Seite, damit ein
// Wechsel zu Chat oder Start ungespeicherte Änderungen nicht wegwirft.

import type { EditorState } from "@codemirror/state";
import type { WorkspaceEntry } from "../types";

export interface CodeTab {
  path: string;
  /** Inhalt, wie er zuletzt gespeichert oder gelesen wurde. */
  original: string;
  /** Aktueller Inhalt im Editor. */
  content: string;
}

export const code = $state<{ tabs: CodeTab[]; active: string | null; dir: string }>({
  tabs: [],
  active: null,
  dir: "",
});

/** Editor-Zustände (Verlauf, Markierung) je Datei. Bewusst nicht reaktiv. */
export const editorStates = new Map<string, EditorState>();

export function isDirty(tab: CodeTab): boolean {
  return tab.content !== tab.original;
}

export function activeTab(): CodeTab | null {
  return code.tabs.find((tab) => tab.path === code.active) ?? null;
}

export function dirtyCount(): number {
  return code.tabs.filter(isDirty).length;
}

/** Schließt eine Datei und wählt die Nachbarin aus. */
export function closeTab(path: string): void {
  const index = code.tabs.findIndex((tab) => tab.path === path);
  if (index < 0) return;
  code.tabs.splice(index, 1);
  editorStates.delete(path);
  if (code.active === path) {
    code.active = code.tabs[Math.min(index, code.tabs.length - 1)]?.path ?? null;
  }
}

/** Überträgt offene Tabs nach einem Umbenennen (Datei oder Ordner). */
export function renameTabs(from: string, to: string): void {
  for (const tab of code.tabs) {
    const inside = tab.path === from || tab.path.startsWith(`${from}/`);
    if (!inside) continue;
    const next = to + tab.path.slice(from.length);
    const state = editorStates.get(tab.path);
    editorStates.delete(tab.path);
    if (state) editorStates.set(next, state);
    if (code.active === tab.path) code.active = next;
    tab.path = next;
  }
}

/** Schließt Tabs einer entfernten Datei oder eines entfernten Ordners. */
export function dropTabs(path: string): void {
  for (const tab of [...code.tabs]) {
    if (tab.path === path || tab.path.startsWith(`${path}/`)) closeTab(tab.path);
  }
}

/**
 * Dateibaum: geladene Ordnerinhalte, aufgeklappte Ordner und Git-Zustand je Datei.
 * Liegt außerhalb der Seite, damit der Baum beim Seitenwechsel so bleibt, wie er war.
 */
export const tree = $state<{
  children: Record<string, WorkspaceEntry[]>;
  expanded: Record<string, boolean>;
  /** Git-Kürzel je Dateipfad (`M`, `A`, `D`, `??`). */
  marks: Record<string, string>;
}>({ children: {}, expanded: {}, marks: {} });

/** Alle Ordner, die geänderte Dateien enthalten, damit zugeklappte Ordner es anzeigen. */
export function changedFolders(marks: Record<string, string>): Set<string> {
  const folders = new Set<string>();
  for (const path of Object.keys(marks)) {
    let index = path.lastIndexOf("/");
    while (index > 0) {
      folders.add(path.slice(0, index));
      index = path.lastIndexOf("/", index - 1);
    }
  }
  return folders;
}

/** Überträgt aufgeklappte Ordner nach einem Verschieben oder Umbenennen. */
export function renameExpanded(from: string, to: string): void {
  for (const key of Object.keys(tree.expanded)) {
    if (key === from || key.startsWith(`${from}/`)) {
      const next = to + key.slice(from.length);
      tree.expanded[next] = tree.expanded[key];
      delete tree.expanded[key];
    }
  }
}
