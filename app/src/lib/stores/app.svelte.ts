// Gemeinsamer Oberflächenzustand, den mehrere Bereiche lesen (Kopfzeile, Chat,
// Befehlssuche, Effekte). Persistente Werte liegen im Vault, nicht hier.
import type { ThemePreference } from "../types";
import { applyThemeColors } from "./themeColors.svelte";

export type EffectsLevel = "full" | "calm";
/** „Ausführlich“ zeigt Erklärsätze, „Kompakt“ blendet sie aus (ui.css, data-hint). */
export type HelpMode = "guided" | "compact";
/** Textgröße in Prozent der Standardgröße (am Wurzelelement, die App rechnet in rem). */
export const FONT_SCALES = [90, 100, 115, 130] as const;
export type FontScale = (typeof FONT_SCALES)[number];

export const appState = $state({
  theme: "system" as ThemePreference,
  effects: "full" as EffectsLevel,
  helpMode: "guided" as HelpMode,
  fontScale: 100 as FontScale,
  /** Zuletzt gemessene Antwortgeschwindigkeit des lokalen Modells. */
  tokensPerSecond: null as number | null,
  /** Unterhaltung, die der Chat beim nächsten Anzeigen öffnen soll (Befehlssuche). */
  pendingConversationId: null as string | null,
  /** Aktion, die der Kalender beim nächsten Anzeigen ausführt (Befehlssuche). */
  pendingCalendarAction: null as "event" | "task" | null,
  /** Text, den der Chat beim nächsten Anzeigen in die Eingabe legt. */
  pendingPrompt: null as string | null,
});

/** Setzt Darstellung und Effekt-Stufe am Wurzelelement, damit reines CSS reagieren kann. */
export function applyAppearance(theme: ThemePreference, effects: EffectsLevel) {
  appState.theme = theme;
  appState.effects = effects;
  const root = document.documentElement;
  // Farbwechsel ohne Übergänge: sonst blenden Texte und Flächen unterschiedlich
  // schnell über und die Oberfläche wirkt für einen Moment kaputt.
  root.classList.add("v-theme-switching");
  if (theme === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", theme);
  root.setAttribute("data-effects", effects);
  // Eigene Farben gelten je Schema; nach einem Wechsel müssen die passenden gesetzt werden.
  applyThemeColors();
  void root.offsetHeight;
  const done = () => root.classList.remove("v-theme-switching");
  requestAnimationFrame(() => requestAnimationFrame(done));
  // Fallback, falls das Fenster gerade nicht zeichnet (dann laufen keine Frames).
  window.setTimeout(done, 120);
}

/** Dekorative Bewegung nur, wenn weder System noch Nutzer sie abgeschaltet haben. */
export function decorativeMotionAllowed(): boolean {
  if (typeof window === "undefined") return false;
  if (document.documentElement.getAttribute("data-effects") === "calm") return false;
  return !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Aktuell wirksames Schema, auch wenn „System“ gewählt ist. */
export function resolvedTheme(): "dark" | "light" {
  if (appState.theme !== "system") return appState.theme;
  return typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/** Setzt den Hilfe-Modus am Wurzelelement, damit reines CSS die Erklärtexte ausblenden kann. */
export function applyHelpMode(mode: HelpMode) {
  appState.helpMode = mode;
  if (typeof document !== "undefined") document.documentElement.setAttribute("data-help", mode);
}

/** Skaliert alle Texte; unbekannte Werte fallen auf 100 zurück, damit ein kaputter Wert nichts unlesbar macht. */
export function applyFontScale(percent: number) {
  const scale = (FONT_SCALES as readonly number[]).includes(percent) ? (percent as FontScale) : 100;
  appState.fontScale = scale;
  if (typeof document !== "undefined") document.documentElement.style.fontSize = scale === 100 ? "" : `${scale}%`;
}
