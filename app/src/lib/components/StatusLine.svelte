<script lang="ts">
  // Zeile unter einer laufenden Antwort: Zustand, Dauer und auf Wunsch die einzelnen Schritte.
  import { t, tk } from "../i18n/index.svelte";
  interface Props { working: boolean; steps?: string[]; startedAt?: number; durationMs?: number; label?: string; }
  let { working, steps = [], startedAt = Date.now(), durationMs, label = tk("IAP arbeitet") }: Props = $props();
  let now = $state(Date.now());
  let open = $state(false);
  const seconds = $derived(((durationMs ?? Math.max(0, now - startedAt)) / 1000).toFixed(1));
  $effect(() => {
    if (!working) return;
    const timer = window.setInterval(() => { if (!document.hidden) now = Date.now(); }, 200);
    return () => clearInterval(timer);
  });
</script>

<div class="status-line" class:working aria-live="polite">
  <button type="button" disabled={steps.length === 0} aria-expanded={steps.length > 0 ? open : undefined} onclick={() => (open = !open)}>
    <span class="dot" aria-hidden="true"></span>
    <span>{working ? t(label) : t("Bearbeitung abgeschlossen")}</span>
    <time>{seconds} s</time>
    {#if steps.length > 0}<svg class:open width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>{/if}
  </button>
  {#if open && steps.length > 0}
    <ol>{#each steps as step, index (index)}<li><b>{index + 1}</b>{step}</li>{/each}</ol>
  {/if}
</div>

<style>
  .status-line { width: fit-content; max-width: 100%; margin: .2rem 0 .65rem; color: var(--v-text-secondary); font-size: .73rem; }
  button { display: inline-flex; align-items: center; gap: .45rem; padding: .2rem 0; border: 0; background: transparent; color: inherit; text-align: left; cursor: pointer; }
  button:disabled { cursor: default; }
  button:focus-visible { outline: 1px solid var(--v-focus-ring); outline-offset: 3px; border-radius: .25rem; }
  .dot { width: .5rem; height: .5rem; border-radius: 50%; background: var(--v-text-muted); }
  .working .dot { background: var(--v-accent-blue); animation: status-pulse 1.4s ease-in-out infinite; }
  time { font-family: var(--v-font-mono); color: var(--v-text-muted); font-size: .65rem; font-variant-numeric: tabular-nums; }
  svg { transition: transform 160ms ease; }
  svg.open { transform: rotate(180deg); }
  ol { margin: .2rem 0 .1rem 1rem; padding: .3rem 0 .25rem .8rem; border-left: 1px solid var(--v-line); list-style: none; color: var(--v-text-muted); }
  li { display: flex; gap: .5rem; padding: .2rem 0; }
  li b { color: var(--v-accent-blue); font-family: var(--v-font-mono); font-weight: 500; }
  @keyframes status-pulse { 50% { opacity: .35; transform: scale(.8); } }
  @media (prefers-reduced-motion: reduce) { .working .dot { animation: none; } svg { transition: none; } }
</style>
