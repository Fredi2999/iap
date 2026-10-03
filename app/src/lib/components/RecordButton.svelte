<script lang="ts">
  // Aufnahme-Knopf: ein Tippen startet oder beendet, ein Zug nach links verwirft.
  import { t } from "../i18n/index.svelte";
  interface Props { recording: boolean; elapsedMs: number; levels: number[]; busy?: boolean; onToggle: () => void; onCancel: () => void; }
  let { recording, elapsedMs, levels, busy = false, onToggle, onCancel }: Props = $props();
  const DISCARD_PX = 64;
  let startX = $state<number | null>(null);
  let offset = $state(0);
  let swallowClick = false;
  const clock = $derived(`${String(Math.floor(elapsedMs / 60000)).padStart(2, "0")}:${String(Math.floor((elapsedMs % 60000) / 1000)).padStart(2, "0")}`);

  function down(event: PointerEvent) {
    if (!recording || busy) return;
    startX = event.clientX;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }
  function move(event: PointerEvent) {
    if (startX !== null) offset = Math.min(0, event.clientX - startX);
  }
  function up() {
    if (recording && offset <= -DISCARD_PX) {
      swallowClick = true;
      onCancel();
    }
    startX = null;
    offset = 0;
  }
  function click() {
    if (swallowClick) { swallowClick = false; return; }
    onToggle();
  }
</script>

<div class="record">
  {#if recording}<span class="discard" aria-hidden="true">← {t("Abbrechen")}</span>{/if}
  <button type="button" class:recording class:dragging={startX !== null && offset !== 0} disabled={busy} aria-pressed={recording}
    aria-label={t(recording ? "Aufnahme beenden; nach links ziehen zum Verwerfen" : "Aufnahme starten")}
    title={t(recording ? "Tippen: beenden · nach links ziehen: verwerfen" : "Aufnahme starten")}
    style={`--offset:${offset}px`} onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onclick={click}>
    {#if recording}
      <span class="stop" aria-hidden="true"></span>
      <span class="bars" aria-hidden="true">{#each levels as level, index (index)}<i style={`transform:scaleY(${Math.max(.13, Math.min(1, level))})`}></i>{/each}</span>
      <time>{clock}</time>
    {:else}
      <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><rect x="9" y="3" width="6" height="12" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3M8 21h8"/></svg>
      <span>{t("Aufnahme starten")}</span>
    {/if}
  </button>
  <p>{t(recording ? "Tippen zum Beenden · nach links ziehen zum Verwerfen" : "Mikrofon bleibt lokal")}</p>
</div>

<style>
  .record { position: relative; display: flex; flex-direction: column; align-items: center; gap: .6rem; }
  button { min-height: 3.2rem; min-width: 11rem; display: inline-flex; align-items: center; justify-content: center; gap: .75rem; padding: .55rem 1.1rem; border: 1px solid var(--v-line-strong); border-radius: 99px; background: var(--v-surface-solid); color: var(--v-text-primary); font-size: var(--v-text-sm); font-weight: 590; touch-action: pan-y; transform: translateX(var(--offset)); transition: border-color 160ms ease, transform 200ms ease; }
  button.dragging { transition: none; }
  button:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: 3px; }
  @media (hover: hover) and (pointer: fine) { button:hover { border-color: var(--v-accent-blue); } }
  button.recording { min-width: 16rem; border-color: var(--v-accent-blue); }
  .stop { width: .8rem; height: .8rem; border-radius: .2rem; background: var(--v-accent-blue); }
  .bars { height: 2rem; display: flex; align-items: center; gap: 2px; }
  .bars i { display: block; width: 2px; height: 30px; border-radius: 2px; background: var(--v-accent-blue); transition: transform 80ms linear; }
  time { font-family: var(--v-font-mono); font-variant-numeric: tabular-nums; font-size: var(--v-text-xs); }
  .discard { position: absolute; right: calc(100% + .35rem); top: .95rem; white-space: nowrap; color: var(--v-text-muted); font-size: var(--v-text-xs); }
  p { margin: 0; color: var(--v-text-muted); font-size: var(--v-text-xs); text-align: center; }
  @media (prefers-reduced-motion: reduce) { button, .bars i { transition: none; } }
</style>
