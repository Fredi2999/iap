// CodeMirror-Bausteine für den Code-Bereich: Sprache, Farben aus den IAP-Tokens,
// Tastenkürzel und Statusinformationen. Die Datei kennt weder IPC noch Svelte.

import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { indentWithTab } from "@codemirror/commands";
import { gotoLine, openSearchPanel } from "@codemirror/search";
import { HighlightStyle, indentUnit, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import { basicSetup } from "codemirror";
import { rust } from "@codemirror/lang-rust";
import { python } from "@codemirror/lang-python";
import { javascript } from "@codemirror/lang-javascript";
import { markdown } from "@codemirror/lang-markdown";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";

export interface LanguageInfo {
  label: string;
  extension: Extension;
}

const PLAIN: LanguageInfo = { label: "Text", extension: [] };

/** Wählt Sprache und Anzeigenamen nach Dateiendung. Unbekanntes bleibt reiner Text. */
export function languageFor(path: string): LanguageInfo {
  const name = path.toLowerCase().split("/").pop() ?? "";
  const ext = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1) : "";
  switch (ext) {
    case "rs":
      return { label: "Rust", extension: rust() };
    case "py":
    case "pyw":
      return { label: "Python", extension: python() };
    case "js":
    case "mjs":
    case "cjs":
      return { label: "JavaScript", extension: javascript() };
    case "jsx":
      return { label: "JavaScript (JSX)", extension: javascript({ jsx: true }) };
    case "ts":
      return { label: "TypeScript", extension: javascript({ typescript: true }) };
    case "tsx":
      return { label: "TypeScript (TSX)", extension: javascript({ typescript: true, jsx: true }) };
    case "json":
    case "jsonc":
      return { label: "JSON", extension: javascript() };
    case "md":
    case "markdown":
      return { label: "Markdown", extension: markdown() };
    case "html":
    case "htm":
    case "svelte":
    case "vue":
      return { label: ext === "svelte" ? "Svelte" : "HTML", extension: html() };
    case "css":
    case "scss":
      return { label: "CSS", extension: css() };
    // Ohne eigene Grammatik, aber mit sinnvollem Namen und – wo es passt – ähnlicher Hervorhebung.
    case "toml":
      return { label: "TOML", extension: [] };
    case "yaml":
    case "yml":
      return { label: "YAML", extension: [] };
    case "sh":
    case "bash":
    case "zsh":
      return { label: "Shell", extension: [] };
    case "ps1":
      return { label: "PowerShell", extension: [] };
    case "xml":
      return { label: "XML", extension: html() };
    default:
      return PLAIN;
  }
}

// Zeilenumbruch ist umschaltbar, ohne den Editor neu aufzubauen. Der Merker gilt auch für
// danach geöffnete Dateien, damit die Einstellung beim Wechsel erhalten bleibt.
const wrapCompartment = new Compartment();
let wordWrapOn = false;

/** Schaltet den Zeilenumbruch im laufenden Editor um und merkt sich die Wahl. */
export function setWordWrap(view: EditorView, on: boolean): void {
  wordWrapOn = on;
  view.dispatch({ effects: wrapCompartment.reconfigure(on ? EditorView.lineWrapping : []) });
}

/** Aktueller Zustand des Zeilenumbruchs. */
export function wordWrapEnabled(): boolean {
  return wordWrapOn;
}

/** Öffnet „Gehe zu Zeile“ im Editor. */
export function goToLine(view: EditorView): void {
  gotoLine(view);
  view.focus();
}

/** Öffnet die Suchen-/Ersetzen-Leiste im Editor. */
export function openSearch(view: EditorView): void {
  openSearchPanel(view);
}

/** Rückt mit Tabulator oder der Einrückung ein, die die Datei schon benutzt. */
export function detectIndent(text: string, fallback = 2): string {
  for (const line of text.split("\n", 400)) {
    if (line.startsWith("\t")) return "\t";
    const match = /^( {2,8})\S/.exec(line);
    if (match) {
      const width = match[1].length;
      return " ".repeat(width % 4 === 0 ? 4 : width % 2 === 0 ? 2 : fallback);
    }
  }
  return " ".repeat(fallback);
}

// Farben aus den Tokens, damit Hell und Dunkel stimmen und der Kontrast erhalten bleibt.
const highlight = HighlightStyle.define([
  { tag: [t.keyword, t.modifier, t.controlKeyword, t.operatorKeyword], color: "var(--v-accent-blue)" },
  { tag: [t.string, t.special(t.string), t.regexp], color: "var(--v-success)" },
  { tag: [t.number, t.bool, t.null, t.atom], color: "var(--v-warning)" },
  { tag: [t.comment, t.lineComment, t.blockComment], color: "var(--v-text-muted)", fontStyle: "italic" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.definition(t.function(t.variableName))], color: "color-mix(in srgb, var(--v-accent-blue) 55%, var(--v-text-primary))" },
  { tag: [t.typeName, t.className, t.namespace], color: "color-mix(in srgb, var(--v-success) 55%, var(--v-accent-blue))" },
  { tag: [t.propertyName, t.attributeName], color: "color-mix(in srgb, var(--v-accent-blue) 35%, var(--v-text-primary))" },
  { tag: [t.tagName, t.angleBracket], color: "var(--v-accent-blue)" },
  { tag: [t.meta, t.processingInstruction, t.annotation], color: "var(--v-warning)" },
  { tag: [t.operator, t.punctuation, t.separator, t.bracket], color: "var(--v-text-secondary)" },
  { tag: [t.heading], color: "var(--v-text-primary)", fontWeight: "700" },
  { tag: [t.emphasis], fontStyle: "italic" },
  { tag: [t.strong], fontWeight: "700" },
  { tag: [t.link, t.url], color: "var(--v-accent-blue)", textDecoration: "underline" },
  { tag: [t.invalid], color: "var(--v-danger)" },
]);

const theme = EditorView.theme({
  "&": { backgroundColor: "transparent", color: "var(--v-text-primary)", height: "100%" },
  ".cm-scroller": { fontFamily: "var(--v-font-mono)", fontSize: "var(--v-text-sm)", lineHeight: "1.6" },
  ".cm-content": { caretColor: "var(--v-accent-blue)" },
  ".cm-gutters": { backgroundColor: "rgb(var(--v-shade) / .12)", color: "var(--v-text-muted)", border: "0", borderRight: "1px solid var(--v-line)" },
  ".cm-activeLine": { backgroundColor: "rgb(var(--v-tint) / .05)" },
  ".cm-activeLineGutter": { backgroundColor: "rgb(var(--v-tint) / .08)", color: "var(--v-text-primary)" },
  "&.cm-focused": { outline: "none" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "var(--v-accent-blue-soft) !important" },
  ".cm-cursor": { borderLeftColor: "var(--v-accent-blue)" },
  ".cm-matchingBracket": { backgroundColor: "var(--v-accent-blue-soft)", outline: "1px solid var(--v-line-strong)" },
  ".cm-nonmatchingBracket": { color: "var(--v-danger)" },
  ".cm-foldPlaceholder": { backgroundColor: "rgb(var(--v-tint) / .1)", border: "1px solid var(--v-line)", color: "var(--v-text-muted)" },
  ".cm-searchMatch": { backgroundColor: "rgb(var(--v-tint) / .18)", outline: "1px solid var(--v-line-strong)" },
  ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--v-accent-blue-soft)" },
  ".cm-panels": { backgroundColor: "var(--v-surface-solid)", color: "var(--v-text-primary)", borderColor: "var(--v-line)" },
  ".cm-panels input, .cm-panels button": { font: "inherit", color: "var(--v-text-primary)" },
  ".cm-panels input": { backgroundColor: "var(--v-surface-input)", border: "1px solid var(--v-line)", borderRadius: "6px", padding: "2px 6px" },
  ".cm-panels button": { backgroundImage: "none", backgroundColor: "rgb(var(--v-tint) / .08)", border: "1px solid var(--v-line)", borderRadius: "6px" },
  ".cm-tooltip": { backgroundColor: "var(--v-surface-solid)", color: "var(--v-text-primary)", border: "1px solid var(--v-line-strong)", borderRadius: "8px", overflow: "hidden" },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "var(--v-accent-blue-soft)", color: "var(--v-text-primary)" },
});

/** Was der Editor nach außen meldet. */
export interface CursorInfo {
  line: number;
  column: number;
  /** Anzahl markierter Zeichen (0 ohne Markierung). */
  selected: number;
  /** Anzahl Zeilen der Markierung (0 ohne Markierung). */
  selectedLines: number;
}

export interface EditorHandlers {
  onChange(doc: string): void;
  onSave(): void;
  onCursor(info: CursorInfo): void;
}

/** Baut den Editor-Zustand einer Datei. Jede geöffnete Datei behält so ihren Verlauf. */
export function buildState(doc: string, path: string, handlers: EditorHandlers): EditorState {
  return EditorState.create({
    doc,
    extensions: [
      basicSetup,
      keymap.of([
        {
          key: "Mod-s",
          preventDefault: true,
          run: () => {
            handlers.onSave();
            return true;
          },
        },
        indentWithTab,
      ]),
      keymap.of([
        {
          key: "Mod-g",
          preventDefault: true,
          run: (target) => {
            gotoLine(target);
            return true;
          },
        },
      ]),
      wrapCompartment.of(wordWrapOn ? EditorView.lineWrapping : []),
      indentUnit.of(detectIndent(doc)),
      languageFor(path).extension,
      syntaxHighlighting(highlight),
      theme,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) handlers.onChange(update.state.doc.toString());
        if (update.docChanged || update.selectionSet) {
          const main = update.state.selection.main;
          const line = update.state.doc.lineAt(main.head);
          const fromLine = update.state.doc.lineAt(main.from).number;
          const toLine = update.state.doc.lineAt(main.to).number;
          handlers.onCursor({
            line: line.number,
            column: main.head - line.from + 1,
            selected: main.to - main.from,
            selectedLines: main.empty ? 0 : toLine - fromLine + 1,
          });
        }
      }),
    ],
  });
}
