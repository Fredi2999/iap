<script lang="ts">
  import { onMount } from "svelte";
  import { emit } from "@tauri-apps/api/event";
  import { t } from "./lib/i18n/index.svelte";
  import { getUiLanguage } from "./lib/ipc";
  import { setLanguage } from "./lib/i18n/index.svelte";

  // Auswahlrahmen für „Bereich ansehen“: ein durchsichtiges Fenster über dem
  // gewählten Bildschirm. Es nimmt nichts auf, sondern meldet nur das
  // aufgezogene Rechteck (physische Pixel) an das Backend. Escape bricht ab.
  let start = $state<{ x: number; y: number } | null>(null);
  let now = $state<{ x: number; y: number } | null>(null);
  let done = false;

  const rect = $derived.by(() => {
    if (!start || !now) return null;
    const x = Math.min(start.x, now.x);
    const y = Math.min(start.y, now.y);
    return { x, y, width: Math.abs(now.x - start.x), height: Math.abs(now.y - start.y) };
  });

  onMount(() => {
    document.documentElement.classList.add("v-pet-root");
    void getUiLanguage().then(setLanguage).catch(() => {});
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") void cancel(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  async function cancel() {
    if (done) return;
    done = true;
    await emit("region-cancelled", null);
  }

  function down(event: PointerEvent) {
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    start = { x: event.clientX, y: event.clientY };
    now = start;
  }

  function move(event: PointerEvent) {
    if (start) now = { x: event.clientX, y: event.clientY };
  }

  async function up() {
    if (done || !rect) return;
    if (rect.width < 8 || rect.height < 8) {
      start = null;
      now = null;
      return;
    }
    done = true;
    const scale = window.devicePixelRatio || 1;
    await emit("region-picked", {
      x: Math.round(rect.x * scale),
      y: Math.round(rect.y * scale),
      width: Math.round(rect.width * scale),
      height: Math.round(rect.height * scale),
    });
  }
</script>

<div class="v-overlay" role="presentation" onpointerdown={down} onpointermove={move} onpointerup={up}>
  {#if rect}<div class="v-overlay-rect" style={`left:${rect.x}px;top:${rect.y}px;width:${rect.width}px;height:${rect.height}px`}></div>{/if}
  <p class="v-overlay-hint">{t("Bereich mit gedrückter Maustaste aufziehen. Escape bricht ab.")}</p>
</div>

<style>
  :global(html.v-pet-root), :global(html.v-pet-root body) { background: transparent !important; overflow: hidden; }
  .v-overlay { position: fixed; inset: 0; cursor: crosshair; background: rgb(0 0 0 / .32); user-select: none; touch-action: none; }
  .v-overlay-rect { position: absolute; border: 2px solid #7cc4ea; background: rgb(255 255 255 / .06); box-shadow: 0 0 0 9999px rgb(0 0 0 / .32); }
  .v-overlay-hint { position: fixed; top: 1rem; left: 50%; transform: translateX(-50%); margin: 0; padding: .5rem .9rem; border-radius: .6rem; background: rgb(10 26 39 / .92); color: #e8f2f8; font: 500 14px/1.4 system-ui, sans-serif; pointer-events: none; }
</style>
