<script lang="ts">
  import { t } from "../lib/i18n/index.svelte";
  import { onDestroy } from "svelte";
  import { sendMessage, subscribeToStream } from "../lib/ipc";
  import type { StreamEvent } from "../lib/types";
  import Markdown from "../lib/components/Markdown.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";

  type Phase = "work" | "break" | "idle";

  let phase = $state<Phase>("idle");
  let secondsLeft = $state(25 * 60);
  let round = $state(0);
  let workLen = $state(25);
  let breakLen = $state(5);
  let running = $state(false);
  let timer: number | null = null;
  let showConfig = $state(false);

  let prompt = $state("");
  let response = $state("");
  let streaming = $state(false);
  let unlisten: (() => void) | null = null;

  function start() {
    phase = "work";
    secondsLeft = workLen * 60;
    running = true;
    tick();
  }

  function pause() {
    running = false;
    if (timer) { clearInterval(timer); timer = null; }
  }

  function reset() {
    pause();
    phase = "idle";
    secondsLeft = workLen * 60;
    round = 0;
  }

  function tick() {
    if (timer) clearInterval(timer);
    timer = window.setInterval(() => {
      if (!running) return;
      secondsLeft = Math.max(0, secondsLeft - 1);
      if (secondsLeft === 0) {
        if (phase === "work") {
          round += 1;
          phase = "break";
          secondsLeft = breakLen * 60;
        } else {
          phase = "work";
          secondsLeft = workLen * 60;
        }
      }
    }, 1000);
  }

  function fmt(sec: number) {
    const m = Math.floor(sec / 60).toString().padStart(2, "0");
    const s = (sec % 60).toString().padStart(2, "0");
    return `${m}:${s}`;
  }

  const progress = $derived(() => {
    const total = (phase === "break" ? breakLen : workLen) * 60;
    if (total === 0) return 0;
    return (total - secondsLeft) / total;
  });

  async function ask() {
    const text = prompt.trim();
    if (!text || streaming) return;
    response = "";
    streaming = true;
    try {
      if (!unlisten) {
        unlisten = await subscribeToStream((event: StreamEvent) => {
          if (event.kind === "delta") {
            response += event.text;
          } else if (event.kind === "finished") {
            streaming = false;
          } else if (event.kind === "failed") {
            streaming = false;
            response += `\n[Fehler: ${event.message}]`;
          }
        });
      }
      await sendMessage({ conversation_id: null, content: text });
      prompt = "";
    } catch (err: any) {
      streaming = false;
      response = t("Fehler: {reason}", { reason: String(err?.message || err) });
    }
  }

  onDestroy(() => {
    if (timer) clearInterval(timer);
    if (unlisten) unlisten();
  });
</script>

<div class="v-page">
  <PageHeader title={t("Fokus")} description="Konzentriert arbeiten mit Zeitschaltung. Kurze Fragen stellst du nebenbei, ohne den Chat zu öffnen." />

  <div class="v-focus">
    <section class="v-card v-focus-timer" aria-label={t("Timer")}>
      <div class="v-ring">
        <svg viewBox="0 0 200 200" aria-hidden="true">
          <circle cx="100" cy="100" r="90" fill="none" stroke="rgb(var(--v-tint) / .1)" stroke-width="5"/>
          <circle cx="100" cy="100" r="90" fill="none" stroke={phase === "break" ? "var(--v-success)" : "var(--v-accent-blue)"} stroke-width="5" stroke-linecap="round"
            stroke-dasharray="565.48" stroke-dashoffset={565.48 * (1 - progress())} class="v-ring-progress" />
        </svg>
        <div class="v-ring-center">
          <span class="v-help">{t(phase === "idle" ? "Bereit" : phase === "work" ? "Fokuszeit" : "Pause")}</span>
          <span class="v-ring-time v-num" aria-live="polite">{fmt(secondsLeft)}</span>
          {#if round > 0}<span class="v-help">{t("Runde {n}", { n: round })}</span>{/if}
        </div>
      </div>
      <div class="v-row" style="justify-content: center">
        {#if !running}
          <button class="v-btn v-btn-primary" onclick={start}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M8 5v14l11-7L8 5Z"/></svg>
            {t(phase === "idle" ? "Starten" : "Weiter")}
          </button>
        {:else}
          <button class="v-btn v-btn-ghost" onclick={pause}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="5" width="4" height="14"/><rect x="14" y="5" width="4" height="14"/></svg>
            {t("Pause")}
          </button>
        {/if}
        <button class="v-btn v-btn-ghost" onclick={reset}>{t("Zurücksetzen")}</button>
      </div>
      <details class="v-details" bind:open={showConfig}>
        <summary>{t("Dauer anpassen")}</summary>
        <div class="v-grid-2">
          <label class="v-field"><span class="v-label">{t("Fokus (Minuten)")}</span>
            <input type="number" min="5" max="90" bind:value={workLen} onchange={() => phase === "idle" && (secondsLeft = workLen * 60)} />
          </label>
          <label class="v-field"><span class="v-label">{t("Pause (Minuten)")}</span>
            <input type="number" min="1" max="30" bind:value={breakLen} />
          </label>
        </div>
      </details>
    </section>

    <section class="v-card v-stack" aria-label={t("Schnellfrage")}>
      <h2 class="v-card-title">{t("Schnellfrage")}</h2>
      <form class="v-stack" onsubmit={(e) => { e.preventDefault(); ask(); }}>
        <textarea bind:value={prompt} rows="3" placeholder={t("Kurze Frage an IAP")} aria-label={t("Frage an IAP")}
          onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); ask(); } }}></textarea>
        <div class="v-row" style="justify-content: space-between">
          <span data-hint class="v-help">{t("Enter sendet, Umschalt+Enter für eine neue Zeile")}</span>
          <button type="submit" class="v-btn v-btn-primary" disabled={streaming || !prompt.trim()}>{t(streaming ? "Antwortet …" : "Fragen")}</button>
        </div>
      </form>
      {#if response}
        <div class="v-focus-answer"><Markdown content={response} isStreaming={streaming} /></div>
      {/if}
    </section>
  </div>
</div>

<style>
  .v-focus { display: grid; grid-template-columns: minmax(18rem, 24rem) minmax(0, 1fr); gap: var(--v-space-4); align-items: start; }
  @media (max-width: 960px) { .v-focus { grid-template-columns: 1fr; } }
  .v-focus-timer { display: flex; flex-direction: column; align-items: stretch; gap: var(--v-space-4); }
  .v-ring { position: relative; width: 15rem; height: 15rem; margin: var(--v-space-2) auto 0; }
  .v-ring svg { position: absolute; inset: 0; transform: rotate(-90deg); }
  .v-ring-progress { transition: stroke-dashoffset 600ms var(--v-ease-out-strong), stroke 300ms ease; }
  .v-ring-center { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 2px; }
  .v-ring-time { color: var(--v-text-primary); font-size: 3rem; font-weight: 300; letter-spacing: -.02em; }
  .v-focus-answer { padding-top: var(--v-space-3); border-top: 1px solid var(--v-line); color: var(--v-text-secondary); font-size: var(--v-text-md); line-height: 1.6; }
  @media (prefers-reduced-motion: reduce) { .v-ring-progress { transition: none; } }
</style>
