// Eigene Oberflächenfarben: wenige Grundfarben, aus denen alle Design-Tokens abgeleitet werden.
// Warum so wenig: Die App rechnet Flächen, Linien, Grautöne und Hover aus denselben Quellen
// (siehe tokens.css). Wer nur sieben Farben setzt, bekommt eine stimmige Oberfläche; wer jedes
// Token einzeln setzen könnte, würde schnell unlesbare Kombinationen bauen.
import { contrastRatio, luminance } from "./contrast.ts";

export const COLOR_ROLES = ["canvas", "surface", "text", "accent", "danger", "warning", "success"] as const;
export type ColorRole = (typeof COLOR_ROLES)[number];
export type SchemeColors = Partial<Record<ColorRole, string>>;
export type ThemeColors = { dark: SchemeColors; light: SchemeColors };
export type Scheme = "dark" | "light";

/** Standardwerte aus tokens.css; die Farbwähler zeigen sie an, solange nichts überschrieben ist. */
export const DEFAULT_COLORS: Record<Scheme, Record<ColorRole, string>> = {
  dark: { canvas: "#08121a", surface: "#172a36", text: "#eef6fa", accent: "#6cc4ee", danger: "#f08a8a", warning: "#e8c27a", success: "#86d3a4" },
  light: { canvas: "#eaf1f5", surface: "#ffffff", text: "#0f1d26", accent: "#1d78aa", danger: "#b42f2f", warning: "#8a5a00", success: "#1f7a45" },
};

export const EMPTY_COLORS: ThemeColors = { dark: {}, light: {} };

const HEX = /^#[0-9a-fA-F]{6}$/;

/** `#rrggbb` in Kleinbuchstaben oder `null`; so landet nie freier Text in einer CSS-Variable. */
export function normalizeHex(value: unknown): string | null {
  return typeof value === "string" && HEX.test(value) ? value.toLowerCase() : null;
}

/** Liest gespeicherten Text; Unbrauchbares wird verworfen statt die Oberfläche zu stören. */
export function parseThemeColors(raw: string | null | undefined): ThemeColors {
  const result: ThemeColors = { dark: {}, light: {} };
  if (!raw) return result;
  let data: unknown;
  try { data = JSON.parse(raw); } catch { return result; }
  if (typeof data !== "object" || data === null) return result;
  for (const scheme of ["dark", "light"] as const) {
    const source = (data as Record<string, unknown>)[scheme];
    if (typeof source !== "object" || source === null) continue;
    for (const role of COLOR_ROLES) {
      const hex = normalizeHex((source as Record<string, unknown>)[role]);
      if (hex) result[scheme][role] = hex;
    }
  }
  return result;
}

function rgbTriplet(hex: string): string {
  const v = parseInt(hex.slice(1), 16);
  return `${(v >> 16) & 255} ${(v >> 8) & 255} ${v & 255}`;
}

const mix = (a: string, percent: number, b: string) => `color-mix(in srgb, ${a} ${percent}%, ${b})`;

/** Anteil der Textfarbe je Stufe der Grauleiter (zinc 50 bis 950); hell nach dunkel wie in tokens.css. */
const ZINC_STEPS: [string, number][] = [
  ["50", 97], ["100", 92], ["200", 84], ["300", 73], ["400", 62],
  ["500", 52], ["600", 38], ["700", 24], ["800", 12], ["900", 6], ["950", 3],
];

/**
 * Baut aus den gesetzten Grundfarben die CSS-Variablen. Fehlende Rollen bleiben unberührt,
 * dort gilt weiter tokens.css. Eine Rolle ohne Text oder Hintergrund kann trotzdem Ableitungen
 * brauchen; dann greift der Standard des Farbschemas.
 */
export function buildThemeVars(scheme: Scheme, colors: SchemeColors): Record<string, string> {
  const vars: Record<string, string> = {};
  const base = DEFAULT_COLORS[scheme];
  const canvas = colors.canvas ?? base.canvas;
  const surface = colors.surface ?? base.surface;
  const text = colors.text ?? base.text;

  if (colors.canvas) {
    vars["--v-canvas"] = canvas;
    vars["--v-canvas-base"] = `linear-gradient(145deg, ${canvas} 0%, ${mix(canvas, 94, text)} 50%, ${canvas} 100%)`;
  }
  if (colors.surface) {
    vars["--v-surface-solid"] = surface;
    vars["--v-surface-input"] = mix(surface, 70, canvas);
    vars["--v-surface-1"] = mix(surface, 62, "transparent");
    vars["--v-surface-2"] = mix(surface, 78, "transparent");
    vars["--v-surface-3"] = mix(mix(surface, 86, text), 92, "transparent");
  }
  if (colors.text || colors.canvas) {
    vars["--v-text-primary"] = text;
    vars["--v-text-secondary"] = mix(text, 84, canvas);
    vars["--v-text-muted"] = mix(text, 66, canvas);
    vars["--v-text-disabled"] = mix(text, 42, canvas);
    vars["--v-tint"] = rgbTriplet(text);
    vars["--v-line"] = mix(text, 14, "transparent");
    vars["--v-line-strong"] = mix(text, 28, "transparent");
    for (const [step, share] of ZINC_STEPS) vars[`--color-zinc-${step}`] = mix(text, share, canvas);
    vars["--color-white"] = text;
    vars["--color-black"] = canvas;
  }
  if (colors.accent) {
    const accent = colors.accent;
    vars["--v-accent-blue"] = accent;
    vars["--v-accent-blue-soft"] = mix(accent, 16, "transparent");
    vars["--v-focus-ring"] = accent;
    vars["--v-cta-bg"] = accent;
    vars["--v-cta-bg-hover"] = mix(accent, 85, text);
    // Schrift auf der Schaltfläche: die Farbe mit dem größeren Kontrast, damit sie lesbar bleibt.
    vars["--v-cta-text"] = contrastRatio(accent, "#000000") >= contrastRatio(accent, "#ffffff") ? "#000000" : "#ffffff";
    vars["--v-canvas-glow-1"] = mix(accent, 30, "transparent");
    vars["--v-canvas-glow-2"] = mix(accent, 18, "transparent");
  }
  if (colors.danger) {
    vars["--v-danger"] = colors.danger;
    vars["--v-danger-soft"] = mix(colors.danger, 14, "transparent");
  }
  if (colors.warning) vars["--v-warning"] = colors.warning;
  if (colors.success) vars["--v-success"] = colors.success;
  return vars;
}

/** Namen aller Variablen, die `buildThemeVars` je setzen kann; zum vollständigen Aufräumen. */
export function allThemeVarNames(): string[] {
  const names = new Set<string>();
  for (const scheme of ["dark", "light"] as const) {
    const full = Object.fromEntries(COLOR_ROLES.map((role) => [role, DEFAULT_COLORS[scheme][role]])) as SchemeColors;
    for (const key of Object.keys(buildThemeVars(scheme, full))) names.add(key);
  }
  return [...names];
}

export type ColorWarning = "text" | "accent";

/** Hinweise auf schlecht lesbare Kombinationen (WCAG: Text 4,5:1, Akzent als Grafik 3:1). */
export function colorWarnings(scheme: Scheme, colors: SchemeColors): ColorWarning[] {
  const base = DEFAULT_COLORS[scheme];
  const canvas = colors.canvas ?? base.canvas;
  const surface = colors.surface ?? base.surface;
  const text = colors.text ?? base.text;
  const accent = colors.accent ?? base.accent;
  const warnings: ColorWarning[] = [];
  if (contrastRatio(text, canvas) < 4.5 || contrastRatio(text, surface) < 4.5) warnings.push("text");
  if (contrastRatio(accent, canvas) < 3) warnings.push("accent");
  return warnings;
}

/** Mischt zwei `#rrggbb`-Farben; `share` ist der Anteil von `b` (0 bis 1). */
export function mixHex(a: string, b: string, share: number): string {
  const pa = parseInt(a.slice(1), 16);
  const pb = parseInt(b.slice(1), 16);
  const channel = (shift: number) => {
    const from = (pa >> shift) & 255;
    const to = (pb >> shift) & 255;
    return Math.round(from + (to - from) * share);
  };
  return `#${[16, 8, 0].map((shift) => channel(shift).toString(16).padStart(2, "0")).join("")}`;
}

export interface WavePalette { horizonColor: string; waveColor: string; crestColor: string; baseColor: string }

/** Standardfarben der animierten Hintergrundwellen, wie sie vor den eigenen Farben festgelegt waren. */
const DEFAULT_WAVES: Record<Scheme, WavePalette> = {
  dark: { horizonColor: "#12344c", waveColor: "#175676", crestColor: "#69b8e0", baseColor: "#07131d" },
  light: { horizonColor: "#dfeaf1", waveColor: "#c4d9e6", crestColor: "#8fc0dd", baseColor: "#e9f0f4" },
};

// Anteil des Akzents an Horizont, Welle und Kamm; im hellen Schema zarter, damit der Text lesbar bleibt.
const WAVE_SHARES: Record<Scheme, [number, number, number]> = { dark: [0.22, 0.34, 1], light: [0.06, 0.2, 0.5] };

/**
 * Farben der Hintergrundwellen. Ohne eigenen Hintergrund oder Akzent bleiben die Standardfarben
 * (das Aussehen ändert sich dann nicht); sonst werden sie aus Hintergrund und Akzent abgeleitet,
 * damit der große animierte Hintergrund nicht in der alten Farbe stehen bleibt.
 */
export function wavePalette(scheme: Scheme, colors: SchemeColors): WavePalette {
  if (!colors.canvas && !colors.accent) return DEFAULT_WAVES[scheme];
  const base = DEFAULT_COLORS[scheme];
  const canvas = colors.canvas ?? base.canvas;
  const accent = colors.accent ?? base.accent;
  const [horizon, wave, crest] = WAVE_SHARES[scheme];
  return {
    baseColor: canvas,
    horizonColor: mixHex(canvas, accent, horizon),
    waveColor: mixHex(canvas, accent, wave),
    crestColor: mixHex(canvas, accent, crest),
  };
}

/** Fertige Farbwelten; jede besteht für Dunkel und Hell die Kontrastprüfung (siehe Test). */
export const PRESETS: { id: string; label: string; colors: ThemeColors }[] = [
  {
    id: "contrast",
    label: "Hoher Kontrast",
    colors: {
      dark: { canvas: "#000000", surface: "#121212", text: "#ffffff", accent: "#ffe600", danger: "#ff6b6b", warning: "#ffd166", success: "#5cff9d" },
      light: { canvas: "#ffffff", surface: "#ffffff", text: "#000000", accent: "#0033cc", danger: "#b00020", warning: "#7a4a00", success: "#006b2d" },
    },
  },
  {
    id: "warm",
    label: "Warm",
    colors: {
      dark: { canvas: "#1b130f", surface: "#2a1d16", text: "#fbeee0", accent: "#ff9f5a", danger: "#ff7a7a", warning: "#ffcf70", success: "#9ed98f" },
      light: { canvas: "#fbf3ea", surface: "#fffaf4", text: "#2b1a10", accent: "#b4531a", danger: "#a62b2b", warning: "#8a5a00", success: "#2f6b2f" },
    },
  },
  {
    id: "forest",
    label: "Wald",
    colors: {
      dark: { canvas: "#0c1a14", surface: "#14281f", text: "#e8f5ec", accent: "#5fd38d", danger: "#f08a8a", warning: "#e8c27a", success: "#86d3a4" },
      light: { canvas: "#eef6f0", surface: "#ffffff", text: "#10231a", accent: "#1f7a4a", danger: "#b42f2f", warning: "#8a5a00", success: "#1f7a45" },
    },
  },
  {
    id: "mono",
    label: "Monochrom",
    colors: {
      dark: { canvas: "#101010", surface: "#1c1c1c", text: "#f2f2f2", accent: "#d0d0d0", danger: "#ff8a8a", warning: "#e0c070", success: "#9ad4a8" },
      light: { canvas: "#f2f2f2", surface: "#ffffff", text: "#151515", accent: "#404040", danger: "#b42f2f", warning: "#7a5200", success: "#1f6a3f" },
    },
  },
];

/**
 * Zieht schlecht lesbare Farben in kleinen Schritten zu Schwarz oder Weiß (je nach Hintergrund),
 * bis Text 4,5:1 gegen Hintergrund und Flächen und der Akzent 3:1 gegen den Hintergrund erreicht.
 * Der Farbton bleibt so weit wie möglich erhalten. Gesetzt wird nur, was sich ändern musste.
 */
export function fixContrast(scheme: Scheme, colors: SchemeColors): SchemeColors {
  const base = DEFAULT_COLORS[scheme];
  const canvas = colors.canvas ?? base.canvas;
  const surface = colors.surface ?? base.surface;
  const extreme = luminance(canvas) > 0.4 ? "#000000" : "#ffffff";
  const pull = (start: string, ok: (candidate: string) => boolean): string => {
    for (let step = 0; step <= 20; step++) {
      const candidate = mixHex(start, extreme, step / 20);
      if (ok(candidate)) return candidate;
    }
    return extreme;
  };
  const next: SchemeColors = { ...colors };
  let text = colors.text ?? base.text;
  if (contrastRatio(text, canvas) < 4.5 || contrastRatio(text, surface) < 4.5) {
    text = pull(text, (c) => contrastRatio(c, canvas) >= 4.5 && contrastRatio(c, surface) >= 4.5);
    next.text = text;
  }
  // Passt der Text zum Hintergrund, aber nicht zu den Flächen (etwa heller Hintergrund mit
  // dunklen Standardflächen), rücken die Flächen zum Hintergrund, weil der Text allein beides nicht schafft.
  if (contrastRatio(text, surface) < 4.5) {
    for (let step = 1; step <= 20; step++) {
      const candidate = mixHex(surface, canvas, step / 20);
      if (contrastRatio(text, candidate) >= 4.5) { next.surface = candidate; break; }
      if (step === 20) next.surface = canvas;
    }
  }
  const accent = colors.accent ?? base.accent;
  if (contrastRatio(accent, canvas) < 3) next.accent = pull(accent, (c) => contrastRatio(c, canvas) >= 3);
  return next;
}

const FILE_FORMAT = "iap-colors";

/** Text für die Exportdatei; enthält nur Farben, nie Pfade oder Namen. */
export function serializeThemeColors(colors: ThemeColors): string {
  return JSON.stringify({ format: FILE_FORMAT, version: 1, dark: colors.dark, light: colors.light }, null, 2);
}

/** Liest eine Exportdatei; `null`, wenn es keine IAP-Farbdatei ist. Ungültige Werte fallen weg. */
export function parseThemeFile(text: string): ThemeColors | null {
  let data: unknown;
  try { data = JSON.parse(text); } catch { return null; }
  if (typeof data !== "object" || data === null || (data as Record<string, unknown>).format !== FILE_FORMAT) return null;
  return parseThemeColors(JSON.stringify(data));
}
