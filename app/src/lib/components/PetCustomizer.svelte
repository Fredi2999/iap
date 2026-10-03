<script lang="ts">
  import { onMount } from "svelte";
  import { t, tk } from "../i18n/index.svelte";
  import { friendlyError } from "../errors";
  import { loadUiState, saveUiState, setPetHotkey } from "../ipc";
  import { resolvedTheme } from "../stores/app.svelte";
  import { DEFAULT_PET_STYLE, petStyle, updatePetStyle } from "../stores/petStyle.svelte";
  import { contrastRatio, MIN_GRAPHIC_CONTRAST } from "../contrast";
  import { COLORS, SHAPES } from "../vendor/bloub/skins";
  import Bloub from "./Bloub.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";

  // Anpassung des Pets: Form, Farbe, Größe und Anzeigewerte. Jede Änderung wird sofort
  // gespeichert und erscheint ohne Neustart im Pet-Fenster. `compact` für das kleine Pet-Fenster.
  interface Props { compact?: boolean }
  let { compact = false }: Props = $props();

  // Globales Kürzel Strg+Alt+V: aus, bis es hier eingeschaltet wird (siehe pet_hotkey.rs).
  let hotkey = $state(false);
  let hotkeyError = $state<string | null>(null);
  onMount(async () => {
    try { hotkey = (await loadUiState("ui.pet_hotkey")) === "on"; } catch (reason) { console.error(reason); }
  });
  async function toggleHotkey(box: HTMLInputElement) {
    hotkeyError = null;
    const enabled = box.checked;
    try {
      hotkey = await setPetHotkey(enabled);
      await saveUiState("ui.pet_hotkey", hotkey ? "on" : "off");
    } catch (reason) {
      // Das Kürzel ließ sich nicht umschalten (etwa schon belegt): Kästchen auf den echten Stand zurück.
      box.checked = hotkey;
      hotkeyError = t(friendlyError(reason).message);
    }
  }

  const SHAPE_NAMES: Record<string, string> = {
    cercle: tk("Kreis"), galet: tk("Kiesel"), squircle: tk("Abgerundetes Quadrat"), capsule: tk("Kapsel"),
    triangle: tk("Dreieck"), hexagone: tk("Sechseck"), nuage: tk("Wolke"), goutte: tk("Tropfen"),
  };
  const COLOR_NAMES: Record<string, string> = {
    encre: tk("Tinte"), brun: tk("Braun"), rouge: tk("Rot"), orange: tk("Orange"), ambre: tk("Bernstein"), vert: tk("Grün"),
    turquoise: tk("Türkis"), bleu: tk("Blau"), violet: tk("Violett"), rose: tk("Rosa"), gris: tk("Grau"), creme: tk("Creme"),
  };

  let error = $state<unknown>(null);
  const paper = $derived(resolvedTheme() === "light" ? "#e6eff4" : "#0a1a27");
  const lowContrast = $derived(petStyle.color !== "auto" && contrastRatio(petStyle.color, paper) < MIN_GRAPHIC_CONTRAST);

  async function change(patch: Partial<typeof petStyle>) {
    error = null;
    try { await updatePetStyle(patch); } catch (reason) { error = reason; }
  }
  const sameColor = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
</script>

<div class="v-customizer" class:compact>
  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  <div class="preview" aria-hidden="true">
    <Bloub avatarState="ready" size={compact ? 84 : Math.min(petStyle.size, 150)} shape={petStyle.shape} color={petStyle.color} />
  </div>

  <fieldset>
    <legend class="v-label">{t("Form")}</legend>
    <div class="grid">
      {#each SHAPES as shape (shape.id)}
        <button type="button" class="v-chip-btn" class:active={petStyle.shape === shape.id} aria-pressed={petStyle.shape === shape.id} onclick={() => change({ shape: shape.id })}>{t(SHAPE_NAMES[shape.id])}</button>
      {/each}
    </div>
  </fieldset>

  <fieldset>
    <legend class="v-label">{t("Farbe")}</legend>
    <div class="swatches">
      <button type="button" class="v-chip-btn" class:active={petStyle.color === "auto"} aria-pressed={petStyle.color === "auto"} onclick={() => change({ color: "auto" })}>{t("Automatisch")}</button>
      {#each COLORS as color (color.id)}
        <button type="button" class="swatch" class:active={sameColor(petStyle.color, color.hex)} style={`--sw:${color.hex}`} aria-pressed={sameColor(petStyle.color, color.hex)} aria-label={t(COLOR_NAMES[color.id])} title={t(COLOR_NAMES[color.id])} onclick={() => change({ color: color.hex })}></button>
      {/each}
      <label class="custom"><span class="sr-only">{t("Eigene Farbe")}</span>
        <input type="color" value={petStyle.color === "auto" ? "#3b93f0" : petStyle.color} onchange={(event) => change({ color: event.currentTarget.value })} />
      </label>
    </div>
    {#if lowContrast}<p class="v-help warn" role="status">{t("Diese Farbe hebt sich auf deinem Hintergrund schlecht ab.")}</p>{/if}
  </fieldset>

  <label class="v-field">
    <span class="v-label">{t("Größe")}: {petStyle.size} px</span>
    <input type="range" min="80" max="180" step="4" value={petStyle.size} onchange={(event) => change({ size: Number(event.currentTarget.value) })} />
  </label>

  <fieldset>
    <legend class="v-label">{t("Am Pet anzeigen")}</legend>
    <label class="v-row v-help"><input type="checkbox" checked={petStyle.show_task} onchange={(e) => change({ show_task: e.currentTarget.checked })} /> {t("Aktuelle Aufgabe")}</label>
    <label class="v-row v-help"><input type="checkbox" checked={petStyle.show_cpu} onchange={(e) => change({ show_cpu: e.currentTarget.checked })} /> {t("Prozessor")}</label>
    <label class="v-row v-help"><input type="checkbox" checked={petStyle.show_ram} onchange={(e) => change({ show_ram: e.currentTarget.checked })} /> {t("Arbeitsspeicher")}</label>
    <label class="v-row v-help"><input type="checkbox" checked={petStyle.show_storage} onchange={(e) => change({ show_storage: e.currentTarget.checked })} /> {t("Speicherplatz des Sticks")}</label>
    <p class="v-help">{t("Nur gemessen, solange das Pet sichtbar ist. Nie gespeichert oder gesendet.")}</p>
  </fieldset>

  <label class="v-field">
    <span class="v-label">{t("Messung alle {n} Sekunden", { n: petStyle.refresh_secs })}</span>
    <input type="range" min="1" max="10" step="1" value={petStyle.refresh_secs} onchange={(event) => change({ refresh_secs: Number(event.currentTarget.value) })} />
  </label>

  <fieldset>
    <legend class="v-label">{t("Tastenkürzel")}</legend>
    <label class="v-row v-help"><input type="checkbox" checked={hotkey} onchange={(e) => toggleHotkey(e.currentTarget)} /> {t("Strg+Alt+V öffnet das Schreibfeld")}</label>
    {#if hotkeyError}<p class="v-help warn" role="alert">{hotkeyError}</p>{/if}
  </fieldset>

  <button type="button" class="v-btn v-btn-ghost" onclick={() => change({ ...DEFAULT_PET_STYLE })}>{t("Zurücksetzen")}</button>
</div>

<style>
  .v-customizer { display: grid; gap: var(--v-space-4); }
  .v-customizer.compact { gap: var(--v-space-3); }
  fieldset { display: grid; gap: var(--v-space-2); margin: 0; padding: 0; border: 0; min-width: 0; }
  legend { padding: 0; margin-bottom: 4px; }
  .preview { display: grid; place-items: center; min-height: 100px; border: 1px solid var(--v-line); border-radius: var(--v-radius-control); background: var(--v-surface-solid); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(8.5rem, 1fr)); gap: 6px; }
  .compact .grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .swatches { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .v-chip-btn { min-height: 2.25rem; padding: 0 12px; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-control); background: transparent; color: var(--v-text-primary); font-size: var(--v-text-sm); }
  .v-chip-btn.active { background: var(--v-accent-blue-soft); border-color: var(--v-accent-blue); }
  .swatch { width: 2rem; height: 2rem; border-radius: 50%; border: 2px solid var(--v-line-strong); background: var(--sw); padding: 0; }
  .swatch.active { outline: 2px solid var(--v-text-primary); outline-offset: 2px; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: 2px; }
  .custom input { width: 2.4rem; height: 2.2rem; padding: 0; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-control); background: transparent; }
  .warn { color: var(--v-warning); }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
</style>
