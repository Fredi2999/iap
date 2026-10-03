// Eigene Oberflächenfarben: laden, speichern, auf das Wurzelelement anwenden.
// Die Werte liegen verschlüsselt im Vault (`ui.theme_colors`). Hauptfenster und Pet-Fenster
// teilen sie über ein Fensterereignis; eine Änderung wirkt so in beiden sofort.
import { emit, listen } from "@tauri-apps/api/event";
import { loadUiState, saveUiState } from "../ipc";
import { allThemeVarNames, buildThemeVars, EMPTY_COLORS, fixContrast, normalizeHex, parseThemeColors, type ColorRole, type Scheme, type ThemeColors } from "../themeColors";

const KEY = "ui.theme_colors";
const EVENT = "theme-colors-changed";

export const themeColors = $state<ThemeColors>({ dark: {}, light: {} });

let started = false;

function replace(next: ThemeColors) {
  themeColors.dark = { ...next.dark };
  themeColors.light = { ...next.light };
}

/** Wirksames Schema aus dem Wurzelelement; hier statt in app.svelte.ts, um keinen Importkreis zu bauen. */
function activeScheme(): Scheme {
  const set = document.documentElement.getAttribute("data-theme");
  if (set === "light" || set === "dark") return set;
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/** Schreibt die Farben des gerade wirksamen Schemas als Variablen ans Wurzelelement. */
export function applyThemeColors(scheme: Scheme = activeScheme()) {
  if (typeof document === "undefined") return;
  const style = document.documentElement.style;
  for (const name of allThemeVarNames()) style.removeProperty(name);
  for (const [name, value] of Object.entries(buildThemeVars(scheme, themeColors[scheme]))) style.setProperty(name, value);
}

/**
 * Lädt die Farben des entsperrten Tresors und hält beide Fenster aktuell. Das Laden läuft bei
 * jedem Aufruf, weil nach dem Sperren ein anderer Tresor mit anderen Farben folgen kann; die
 * Ereignis-Abonnements entstehen nur einmal.
 */
export async function initThemeColors(): Promise<void> {
  try { replace(parseThemeColors(await loadUiState(KEY))); } catch (reason) { console.error(reason); }
  applyThemeColors();
  if (started) return;
  started = true;
  try {
    await listen<ThemeColors>(EVENT, (message) => {
      replace(parseThemeColors(JSON.stringify(message.payload)));
      applyThemeColors();
    });
  } catch (reason) { console.error(reason); }
  // „System“ wählt das Schema selbst; wechselt Windows zwischen Hell und Dunkel, gelten andere Farben.
  try {
    window.matchMedia("(prefers-color-scheme: light)").addEventListener("change", () => applyThemeColors());
  } catch (reason) { console.error(reason); }
}

// Änderungen laufen nacheinander, damit zwei schnelle Klicks nicht vom selben alten Stand ausgehen.
let chain: Promise<void> = Promise.resolve();

async function persist(next: ThemeColors): Promise<void> {
  const text = JSON.stringify(next);
  await saveUiState(KEY, text);
  replace(next);
  applyThemeColors();
  try { await emit(EVENT, next); } catch (reason) { console.error(reason); }
}

/** Setzt eine Farbe (`null` = Standard). Ungültige Werte werden abgelehnt. */
export function setThemeColor(scheme: Scheme, role: ColorRole, value: string | null): Promise<void> {
  const hex = value === null ? null : normalizeHex(value);
  if (value !== null && hex === null) return Promise.reject(new Error("Die Farbe muss ein Hexwert wie #3b93f0 sein."));
  const next = chain.catch(() => {}).then(async () => {
    const draft: ThemeColors = { dark: { ...themeColors.dark }, light: { ...themeColors.light } };
    if (hex === null) delete draft[scheme][role];
    else draft[scheme][role] = hex;
    await persist(draft);
  });
  chain = next;
  return next;
}

/** Setzt alle Farben eines Schemas zurück. */
export function resetThemeColors(scheme: Scheme): Promise<void> {
  const next = chain.catch(() => {}).then(async () => {
    const draft: ThemeColors = { dark: { ...themeColors.dark }, light: { ...themeColors.light } };
    draft[scheme] = { ...EMPTY_COLORS[scheme] };
    await persist(draft);
  });
  chain = next;
  return next;
}

/** Ersetzt die Farben beider Schemas auf einmal (Vorlage oder Import). */
export function replaceThemeColors(next: ThemeColors): Promise<void> {
  const clean = parseThemeColors(JSON.stringify(next));
  const done = chain.catch(() => {}).then(() => persist(clean));
  chain = done;
  return done;
}

/** Korrigiert zu schwache Kontraste des Schemas; liefert, ob sich etwas geändert hat. */
export function fixThemeContrast(scheme: Scheme): Promise<boolean> {
  let changed = false;
  const done = chain.catch(() => {}).then(async () => {
    const fixed = fixContrast(scheme, themeColors[scheme]);
    changed = JSON.stringify(fixed) !== JSON.stringify(themeColors[scheme]);
    if (!changed) return;
    await persist({ dark: { ...themeColors.dark }, light: { ...themeColors.light }, [scheme]: fixed });
  }).then(() => changed);
  chain = done.then(() => {});
  return done;
}
