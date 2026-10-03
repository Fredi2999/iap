// Läuft ohne Zusatzpaket: node --experimental-strip-types --test scripts/themeColors.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { allThemeVarNames, buildThemeVars, colorWarnings, normalizeHex, parseThemeColors } from "../src/lib/themeColors.ts";

test("only #rrggbb is accepted, so no free text reaches a CSS variable", () => {
  assert.equal(normalizeHex("#ABCDEF"), "#abcdef");
  for (const bad of ["red", "#fff", "#12345g", "url(x)", "#123456; x", "", null, 5]) assert.equal(normalizeHex(bad), null);
});

test("stored data with garbage is cleaned instead of breaking the UI", () => {
  assert.deepEqual(parseThemeColors("kaputt"), { dark: {}, light: {} });
  assert.deepEqual(parseThemeColors('{"dark":{"accent":"#ff0000","text":"javascript:1","x":"#000000"},"light":5}'), { dark: { accent: "#ff0000" }, light: {} });
});

test("no overrides means no variables, so tokens.css stays in charge", () => {
  assert.deepEqual(buildThemeVars("dark", {}), {});
});

test("accent sets button text with readable contrast", () => {
  assert.equal(buildThemeVars("dark", { accent: "#ffee00" })["--v-cta-text"], "#000000");
  assert.equal(buildThemeVars("dark", { accent: "#102030" })["--v-cta-text"], "#ffffff");
});

test("text or canvas derive the grey ladder from both", () => {
  const vars = buildThemeVars("dark", { canvas: "#101010" });
  assert.ok(vars["--color-zinc-400"].includes("#101010"));
  assert.equal(vars["--color-black"], "#101010");
});

test("every produced variable name is known for cleanup", () => {
  const known = new Set(allThemeVarNames());
  for (const scheme of ["dark", "light"] as const) {
    const all = buildThemeVars(scheme, { canvas: "#000000", surface: "#111111", text: "#ffffff", accent: "#00ff00", danger: "#ff0000", warning: "#ffff00", success: "#00ff00" });
    for (const key of Object.keys(all)) assert.ok(known.has(key), key);
  }
});

test("unreadable combinations are flagged", () => {
  assert.deepEqual(colorWarnings("dark", {}), []);
  assert.ok(colorWarnings("dark", { text: "#222222" }).includes("text"));
  assert.ok(colorWarnings("dark", { accent: "#08121a" }).includes("accent"));
});

test("wave palette keeps the defaults without custom canvas or accent", async () => {
  const { wavePalette } = await import("../src/lib/themeColors.ts");
  assert.equal(wavePalette("dark", {}).baseColor, "#07131d");
  assert.equal(wavePalette("light", { text: "#101010" }).crestColor, "#8fc0dd");
});

test("wave palette follows a custom canvas and accent", async () => {
  const { wavePalette, mixHex } = await import("../src/lib/themeColors.ts");
  const palette = wavePalette("dark", { canvas: "#200030", accent: "#ff80c0" });
  assert.equal(palette.baseColor, "#200030");
  assert.equal(palette.crestColor, "#ff80c0");
  assert.equal(palette.waveColor, mixHex("#200030", "#ff80c0", 0.34));
});

test("mixHex blends channels and handles the ends", async () => {
  const { mixHex } = await import("../src/lib/themeColors.ts");
  assert.equal(mixHex("#000000", "#ffffff", 0), "#000000");
  assert.equal(mixHex("#000000", "#ffffff", 1), "#ffffff");
  assert.equal(mixHex("#000000", "#ffffff", 0.5), "#808080");
});

test("every preset passes the contrast check in both schemes", async () => {
  const { PRESETS, colorWarnings } = await import("../src/lib/themeColors.ts");
  for (const preset of PRESETS) {
    for (const scheme of ["dark", "light"] as const) {
      assert.deepEqual(colorWarnings(scheme, preset.colors[scheme]), [], `${preset.id}/${scheme}`);
    }
  }
});

test("fixContrast repairs weak text and accent and leaves good colors alone", async () => {
  const { fixContrast, colorWarnings } = await import("../src/lib/themeColors.ts");
  const weak = { canvas: "#101010", text: "#222222", accent: "#151515" };
  const fixed = fixContrast("dark", weak);
  assert.deepEqual(colorWarnings("dark", fixed), []);
  assert.equal(fixed.canvas, "#101010");
  const good = { canvas: "#101010", text: "#ffffff", accent: "#ffcc00" };
  assert.deepEqual(fixContrast("dark", good), good);
  // Ohne gesetzten Text reicht ein neuer heller Hintergrund, der Standardtext würde unlesbar.
  const lightCanvasOnDarkDefaults = fixContrast("dark", { canvas: "#f5f5f5" });
  assert.deepEqual(colorWarnings("dark", lightCanvasOnDarkDefaults), []);
});

test("export and import round-trip and reject foreign files", async () => {
  const { serializeThemeColors, parseThemeFile } = await import("../src/lib/themeColors.ts");
  const colors = { dark: { accent: "#ff7ab8" }, light: { canvas: "#ffffff" } };
  assert.deepEqual(parseThemeFile(serializeThemeColors(colors)), colors);
  assert.equal(parseThemeFile("{\"hello\":1}"), null);
  assert.equal(parseThemeFile("kein json"), null);
  assert.equal(parseThemeFile(serializeThemeColors({ dark: { accent: "red; x" as string }, light: {} }))?.dark.accent, undefined);
});
