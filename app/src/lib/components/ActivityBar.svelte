<script lang="ts">
  // Sichtbarer Status für Langläufer: Alles, was die Job-Warteschlange abarbeitet (Antwort, Agent,
  // Workflow, Spracherkennung, Modellwechsel ...), erscheint hier mit Laufzeit und Abbrechen,
  // unabhängig davon, auf welcher Seite man gerade ist. Eine eigene Statusquelle gibt es nicht.
  import { onDestroy } from "svelte";
  import { t, tk } from "../i18n/index.svelte";
  import { cancelJob } from "../ipc";
  import { errorText } from "../errors";
  import { notify } from "../notifications";
  import { jobs } from "../stores/jobs.svelte";
  import type { JobKind } from "../types";

  interface Props {
    /** Auf der Chat-Seite zeigt der Chat die Antwort selbst; dort entfällt der Eintrag „Antwort“. */
    hideChat?: boolean;
  }
  let { hideChat = false }: Props = $props();

  const KIND_LABELS: Record<JobKind, string> = {
    chat: tk("Antwort"),
    agent: tk("Agent"),
    agent_flow: tk("Agent Flow"),
    workflow: tk("Ablauf"),
    vision: tk("Bildschirm"),
    speech_to_text: tk("Spracherkennung"),
    text_to_speech: tk("Sprachausgabe"),
    model_switch: tk("Modellwechsel"),
    compare: tk("Vergleich"),
    mail: tk("Mail"),
  };

  const active = $derived(jobs.list.filter((job) => (job.status === "running" || job.status === "waiting") && !(hideChat && job.kind === "chat")));
  const current = $derived(active.find((job) => job.status === "running") ?? active[0] ?? null);
  const others = $derived(active.length - (current ? 1 : 0));

  // Die Laufzeit zählt nur, solange etwas läuft; ohne Eintrag gibt es keinen Takt.
  let now = $state(Date.now());
  let timer: ReturnType<typeof setInterval> | null = null;
  $effect(() => {
    if (current && !timer) timer = setInterval(() => { now = Date.now(); }, 1000);
    if (!current && timer) { clearInterval(timer); timer = null; }
  });
  onDestroy(() => { if (timer) clearInterval(timer); });

  function elapsed(startMs: number): string {
    const seconds = Math.max(0, Math.floor((now - startMs) / 1000));
    return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
  }

  let cancelling = $state<string | null>(null);
  async function cancel(id: string) {
    cancelling = id;
    try { await cancelJob(id); } catch (reason) { notify("Abbrechen nicht möglich", errorText(reason), "error"); }
    finally { cancelling = null; }
  }
</script>

{#if current}
  <div class="activity" role="status" aria-live="polite">
    <span class="dot" class:waiting={current.status === "waiting"} aria-hidden="true"></span>
    <span class="kind">{t(KIND_LABELS[current.kind])}</span>
    <span class="label">{current.label}</span>
    {#if current.status === "waiting"}
      <span class="meta">{t("wartet")}{current.queue_position ? ` (${current.queue_position})` : ""}</span>
    {:else}
      <span class="meta v-num">{elapsed(current.started_unix_ms)}</span>
    {/if}
    {#if others > 0}<span class="meta">{t("+{n} weitere", { n: others })}</span>{/if}
    {#if current.cancelable}
      <button type="button" class="stop" disabled={cancelling === current.id} onclick={() => cancel(current.id)}>{t("Abbrechen")}</button>
    {/if}
  </div>
{/if}

<style>
  .activity { position: relative; z-index: 15; flex: 0 0 auto; display: flex; align-items: center; gap: var(--v-space-3); min-height: 2.25rem; padding: .3rem clamp(.75rem, 2vw, 1.5rem); border-bottom: 1px solid var(--v-line); background: color-mix(in oklab, var(--v-accent-blue) 10%, var(--v-surface-solid)); font-size: var(--v-text-sm); color: var(--v-text-secondary); }
  .dot { width: .55rem; height: .55rem; border-radius: 50%; background: var(--v-accent-blue); animation: pulse 1.4s ease-in-out infinite; }
  .dot.waiting { background: var(--v-warning); animation: none; }
  .kind { font-weight: 600; color: var(--v-text-primary); }
  .label { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { color: var(--v-text-muted); white-space: nowrap; }
  .stop { min-height: 1.75rem; padding: 0 .75rem; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-control); background: transparent; color: var(--v-text-primary); font-size: var(--v-text-xs); }
  .stop:disabled { opacity: .5; }
  @keyframes pulse { 50% { opacity: .35; } }
  :global(:root[data-effects="calm"]) .dot, :global(:root[data-help]) .dot { animation-duration: 2.4s; }
  @media (prefers-reduced-motion: reduce) { .dot { animation: none; } }
</style>
