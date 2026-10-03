<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "../i18n/index.svelte";
  import { friendlyError } from "../errors";
  import { screenLook, screenPickRegion, screenSources, screenStatus } from "../ipc";
  import type { ScreenAnswer, ScreenSources, ScreenStatus, ScreenTarget } from "../types";

  // „Bildschirm ansehen“: Bildschirm oder Fenster wählen, optional eine Frage
  // stellen. Jeder Klick auf „Ansehen“ ist ein eigener Auftrag mit genau einer
  // Aufnahme. Das Bild bleibt im Arbeitsspeicher; Frage und Antwort erscheinen
  // in der Unterhaltung. Fehlt der Bildpfad, wird nur der Status gezeigt.
  interface Props {
    conversationId: string | null;
    onClose: () => void;
    onDone: (answer: ScreenAnswer) => void;
  }
  let { conversationId, onClose, onDone }: Props = $props();

  let status = $state<ScreenStatus | null>(null);
  let sources = $state<ScreenSources | null>(null);
  let choice = $state("monitor:0");
  let question = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let ready = $state<HTMLButtonElement | null>(null);

  onMount(() => {
    (async () => {
      try {
        status = await screenStatus();
        if (status.available) {
          sources = await screenSources();
          const primary = sources.monitors.find((monitor) => monitor.primary) ?? sources.monitors[0];
          if (primary) choice = `monitor:${primary.index}`;
        }
      } catch (reason) {
        error = friendlyError(reason).message;
      }
    })();
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape" && !busy) onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  async function target(): Promise<ScreenTarget | null> {
    const [kind, value] = choice.split(":");
    if (kind === "monitor") return { kind: "monitor", index: Number(value) };
    // Fenster: Windows-Fenstergriffe passen als Zahl, sind aber keine Bildschirmkoordinaten.
    if (kind === "window") return { kind: "window", handle: Number(value) };
    // Bereich: erst auf dem Bildschirm aufziehen lassen; Abbruch nimmt nichts auf.
    if (kind === "region") return screenPickRegion(Number(value));
    return null;
  }

  async function look() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const selected = await target();
      if (!selected) {
        busy = false;
        return;
      }
      onDone(await screenLook({ target: selected, question, conversation_id: conversationId }));
    } catch (reason) {
      error = friendlyError(reason).message;
      busy = false;
    }
  }
</script>

<div class="v-look-layer">
  <button type="button" class="v-look-shade" aria-label={t("Schließen")} onclick={() => { if (!busy) onClose(); }}></button>
  <div class="v-look" role="dialog" aria-modal="true" aria-labelledby="v-look-title">
    <h2 id="v-look-title">{t("Bildschirm ansehen")}</h2>
    {#if status && !status.available}
      <p class="v-look-blocked" role="status"><strong>{t("Bildschirmverständnis nicht eingerichtet")}</strong>
        {#if status.detail}<span>{t(status.detail)}</span>{/if}</p>
    {:else if !sources}
      <p class="v-look-note">{t("Quellen werden gesucht …")}</p>
    {:else}
      <p class="v-look-note">{t("IAP nimmt genau einmal auf, wertet das Bild lokal aus und speichert es nicht. Eine weitere Aufnahme braucht einen neuen Auftrag.")}</p>
      <fieldset class="v-look-sources" disabled={busy}>
        <legend>{t("Was soll IAP ansehen?")}</legend>
        {#each sources.monitors as monitor (monitor.index)}
          <label><input type="radio" name="source" value={`monitor:${monitor.index}`} bind:group={choice} /><span>{t("Bildschirm {n}", { n: monitor.index + 1 })} · {monitor.width}×{monitor.height}{monitor.primary ? ` · ${t("Hauptbildschirm")}` : ""}</span></label>
          <label><input type="radio" name="source" value={`region:${monitor.index}`} bind:group={choice} /><span>{t("Bereich auf Bildschirm {n} aufziehen", { n: monitor.index + 1 })}</span></label>
        {/each}
        {#each sources.windows as window (window.handle)}
          <label><input type="radio" name="source" value={`window:${window.handle}`} bind:group={choice} /><span>{t("Fenster")}: {window.title}</span></label>
        {/each}
      </fieldset>
      <label class="v-field"><span class="v-label">{t("Frage zum Bild (optional)")}</span>
        <input bind:value={question} maxlength="500" placeholder={t("Zum Beispiel: Was steht in der Fehlermeldung?")} disabled={busy} />
      </label>
    {/if}
    {#if error}<p class="v-look-error" role="alert">{t(error)}</p>{/if}
    {#if busy}<p class="v-look-note" aria-live="polite">{t("Aufnahme läuft, das Bild wird lokal ausgewertet. Das kann etwas dauern.")}</p>{/if}
    <div class="v-look-actions">
      <button type="button" class="v-btn v-btn-ghost" onclick={onClose} disabled={busy}>{t(status && !status.available ? "Schließen" : "Abbrechen")}</button>
      {#if status?.available}<button type="button" class="v-btn v-btn-primary" bind:this={ready} onclick={look} disabled={busy || !sources}>{t(busy ? "Wird ausgewertet …" : "Ansehen")}</button>{/if}
    </div>
  </div>
</div>

<style>
  .v-look-layer { position: fixed; inset: 0; z-index: 85; display: flex; align-items: center; justify-content: center; padding: 1rem; }
  .v-look-shade { position: absolute; inset: 0; border: 0; border-radius: 0; background: rgb(var(--v-shade) / .5); cursor: default; }
  .v-look { position: relative; width: min(34rem, 100%); max-height: calc(100dvh - 2rem); overflow-y: auto; display: flex; flex-direction: column; gap: var(--v-space-4); padding: var(--v-space-6); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: 0 25px 70px rgb(var(--v-shade) / .42); }
  h2 { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-lg); font-weight: 600; }
  .v-look-note { margin: 0; color: var(--v-text-muted); font-size: var(--v-text-sm); line-height: 1.5; }
  .v-look-blocked { display: grid; gap: .4rem; margin: 0; padding: var(--v-space-3) var(--v-space-4); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-field); background: rgb(var(--v-tint) / .05); font-size: var(--v-text-sm); line-height: 1.5; }
  .v-look-blocked span { color: var(--v-text-secondary); }
  .v-look-sources { display: grid; gap: .35rem; min-width: 0; margin: 0; padding: 0; border: 0; }
  .v-look-sources legend { margin-bottom: .4rem; color: var(--v-text-secondary); font-size: var(--v-text-sm); font-weight: 600; }
  .v-look-sources label { display: flex; align-items: flex-start; gap: .55rem; padding: .45rem .6rem; border: 1px solid var(--v-line); border-radius: var(--v-radius-field); font-size: var(--v-text-sm); overflow-wrap: anywhere; cursor: pointer; }
  .v-look-sources label:has(input:checked) { border-color: var(--v-accent-blue); background: rgb(var(--v-tint) / .06); }
  .v-look-error { margin: 0; color: var(--v-danger); font-size: var(--v-text-sm); }
  .v-look-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--v-space-2); }
  .v-look { transition: opacity 200ms var(--v-ease-out-strong), transform 200ms var(--v-ease-out-strong); }
  .v-look-shade { transition: opacity 200ms ease; }
  @starting-style { .v-look { opacity: 0; transform: scale(.96); } .v-look-shade { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .v-look { transition: opacity 120ms ease; } @starting-style { .v-look { transform: none; } } }
</style>
