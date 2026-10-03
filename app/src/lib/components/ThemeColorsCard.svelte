<script lang="ts">
  // Eigene Farben der Oberfläche. Sieben Grundfarben je Farbschema; alles andere (Flächen, Linien,
  // Grautöne, Hover) leitet die App daraus ab. Die Wahl liegt verschlüsselt im Tresor.
  import { t } from "../i18n/index.svelte";
  import { errorText } from "../errors";
  import { notify } from "../notifications";
  import { appState } from "../stores/app.svelte";
  import { fixThemeContrast, replaceThemeColors, resetThemeColors, setThemeColor, themeColors } from "../stores/themeColors.svelte";
  import { colorWarnings, COLOR_ROLES, DEFAULT_COLORS, parseThemeFile, PRESETS, serializeThemeColors, type ColorRole, type Scheme } from "../themeColors";

  const LABELS: Record<ColorRole, string> = {
    canvas: "Hintergrund",
    surface: "Flächen und Dialoge",
    text: "Text",
    accent: "Akzent und Schaltflächen",
    danger: "Fehler und Löschen",
    warning: "Warnung",
    success: "Erfolg",
  };

  const systemScheme = (): Scheme =>
    typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  const activeScheme = $derived<Scheme>(appState.theme === "system" ? systemScheme() : appState.theme);

  // Bearbeitet wird zunächst das gerade sichtbare Schema; der Umschalter erlaubt das andere.
  let chosen = $state<Scheme | null>(null);
  const scheme = $derived<Scheme>(chosen ?? activeScheme);
  const colors = $derived(themeColors[scheme]);
  const warnings = $derived(colorWarnings(scheme, colors));
  const customized = $derived(Object.keys(colors).length > 0);

  async function change(role: ColorRole, value: string | null) {
    try { await setThemeColor(scheme, role, value); } catch (reason) { notify("Farbe nicht gespeichert", errorText(reason), "error"); }
  }
  async function applyPreset(id: string) {
    const preset = PRESETS.find((entry) => entry.id === id);
    if (!preset) return;
    try { await replaceThemeColors(preset.colors); } catch (reason) { notify("Farben nicht gespeichert", errorText(reason), "error"); }
  }
  async function fixContrastNow() {
    try { await fixThemeContrast(scheme); } catch (reason) { notify("Farben nicht gespeichert", errorText(reason), "error"); }
  }
  function exportColors() {
    const blob = new Blob([serializeThemeColors(themeColors)], { type: "application/json" });
    const link = document.createElement("a");
    link.href = URL.createObjectURL(blob);
    link.download = "iap-farben.json";
    link.click();
    URL.revokeObjectURL(link.href);
  }
  async function importColors(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const parsed = parseThemeFile(await file.text());
    if (!parsed) { notify("Datei nicht gelesen", "Das ist keine IAP-Farbdatei.", "error"); return; }
    try { await replaceThemeColors(parsed); } catch (reason) { notify("Farben nicht gespeichert", errorText(reason), "error"); }
  }
  async function resetAll() {
    try { await resetThemeColors(scheme); } catch (reason) { notify("Farben nicht zurückgesetzt", errorText(reason), "error"); }
  }
</script>

<section class="v-card v-stack" aria-labelledby="set-colors">
  <div class="head">
    <h2 id="set-colors" class="v-card-title">{t("Eigene Farben")}</h2>
    <div class="v-segmented" role="group" aria-label={t("Farben bearbeiten für")}>
      <button type="button" class:active={scheme === "dark"} aria-pressed={scheme === "dark"} onclick={() => (chosen = "dark")}>{t("Dunkel")}</button>
      <button type="button" class:active={scheme === "light"} aria-pressed={scheme === "light"} onclick={() => (chosen = "light")}>{t("Hell")}</button>
    </div>
  </div>
  <p data-hint class="v-help">{t("Je Farbschema eigene Farben. Flächen und Grautöne leitet IAP ab. Gilt auch für das Pet.")}</p>

  <div class="presets" role="group" aria-label={t("Vorlagen")}>
    <span class="v-label">{t("Vorlagen")}</span>
    {#each PRESETS as preset (preset.id)}
      <button type="button" class="v-chip-btn" onclick={() => applyPreset(preset.id)}>{t(preset.label)}</button>
    {/each}
  </div>

  <ul class="rows">
    {#each COLOR_ROLES as role (role)}
      <li class="row">
        <label for={`color-${role}`}>{t(LABELS[role])}</label>
        <input id={`color-${role}`} type="color" value={colors[role] ?? DEFAULT_COLORS[scheme][role]} onchange={(event) => change(role, event.currentTarget.value)} />
        <button type="button" class="v-chip-btn" disabled={!colors[role]} onclick={() => change(role, null)}>{t("Zurücksetzen")}</button>
      </li>
    {/each}
  </ul>

  {#if warnings.includes("text")}<p class="v-help warn" role="status">{t("Text und Hintergrund heben sich zu schwach ab. Das ist schwer lesbar.")}</p>{/if}
  {#if warnings.includes("accent")}<p class="v-help warn" role="status">{t("Der Akzent hebt sich auf dem Hintergrund schlecht ab.")}</p>{/if}

  <div class="actions">
    {#if warnings.length > 0}<button type="button" class="v-chip-btn" onclick={fixContrastNow}>{t("Kontrast korrigieren")}</button>{/if}
    <button type="button" class="v-chip-btn" disabled={!customized} onclick={resetAll}>{t("Alle Farben zurücksetzen")}</button>
    <button type="button" class="v-chip-btn" onclick={exportColors}>{t("Exportieren")}</button>
    <label class="v-chip-btn file">{t("Importieren")}<input type="file" accept="application/json,.json" onchange={importColors} /></label>
  </div>
</section>

<style>
  .head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; }
  .presets, .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .file { display: inline-flex; align-items: center; cursor: pointer; }
  .file input { position: absolute; width: 1px; height: 1px; opacity: 0; pointer-events: none; }
  .rows { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .row { display: grid; grid-template-columns: 1fr auto auto; align-items: center; gap: 12px; }
  .row label { font-size: var(--v-text-md); color: var(--v-text-secondary); }
  input[type="color"] { width: 2.5rem; height: 2rem; padding: 0; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-control); background: transparent; cursor: pointer; }
  .v-chip-btn { min-height: 2.25rem; padding: 0 12px; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-control); background: transparent; color: var(--v-text-primary); font-size: var(--v-text-sm); }
  .v-chip-btn:disabled { opacity: .45; cursor: default; }
  .warn { color: var(--v-warning); }
</style>
