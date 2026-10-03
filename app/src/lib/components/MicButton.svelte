<script lang="ts">
  import { t } from "../i18n/index.svelte";
  import { notify } from "../notifications";
  import { discardListening, startListening, stopListening, voice, voiceCommands } from "../stores/voice.svelte";

  // Beschrifteter Mikrofonknopf. Eine Aufnahme startet nur durch diesen Knopf
  // (oder das Tastenkürzel, das denselben Knopf auslöst) und ist immer sichtbar
  // mit Zeit, Stopp und Verwerfen. Der erkannte Text wird nie gesendet, sondern
  // geht als editierbarer Entwurf an `onText`.
  interface Props {
    onText: (text: string) => void;
    /** Kompakte Darstellung ohne Beschriftung (Promptbar). */
    compact?: boolean;
    /** Beendet die Aufnahme selbst, wenn nach dem Sprechen eine Pause entsteht (Pet, Sprachsitzung). */
    autoStop?: boolean;
  }
  let { onText, compact = false, autoStop = false }: Props = $props();

  let host = $state<HTMLElement | null>(null);
  let working = $state(false);

  const listening = $derived(voice.state === "listening");
  const transcribing = $derived(voice.state === "transcribing");
  const blocked = $derived(voice.status?.available === false);
  const timer = $derived(`${Math.floor(voice.elapsedMs / 60000)}:${String(Math.floor(voice.elapsedMs / 1000) % 60).padStart(2, "0")}`);

  async function finish() {
    if (working) return;
    working = true;
    try {
      const transcript = await stopListening();
      if (!transcript) return;
      if (transcript.command) {
        // Nur harmlose Ansichtswechsel; alles andere bleibt bei seinen Freigaben.
        voiceCommands.handler?.(transcript.command);
        return;
      }
      if (transcript.text.trim()) onText(transcript.text.trim());
      else notify("Nichts erkannt", "Es wurde keine Sprache erkannt. Bitte noch einmal versuchen.", "info");
    } finally {
      working = false;
    }
  }

  async function toggle() {
    if (working) return;
    if (listening) await finish();
    else if (voice.state === "idle") await startListening();
  }

  // Pausenerkennung: Erst wenn gesprochen wurde und danach gut anderthalb Sekunden Ruhe
  // herrschen, endet die Aufnahme. Der Knopf „Stopp“ bleibt jederzeit möglich; ohne
  // erkannte Sprache (zu leise) geschieht nichts von allein.
  const SPEECH_LEVEL = 0.12;
  const PAUSE_MS = 1600;
  let spoke = false;
  let quietSince = 0;
  $effect(() => {
    const level = voice.level;
    if (!listening || !(autoStop || voice.conversationMode)) {
      spoke = false;
      quietSince = 0;
      return;
    }
    const now = performance.now();
    if (level > SPEECH_LEVEL) {
      spoke = true;
      quietSince = 0;
    } else if (spoke) {
      if (quietSince === 0) quietSince = now;
      else if (now - quietSince > PAUSE_MS && voice.elapsedMs > 1200) void finish();
    }
  });

  // Höchstdauer erreicht: wie ein Klick auf Stopp behandeln.
  $effect(() => {
    if (listening && voice.notice === "max_duration") void finish();
  });

  // Tastenkürzel Strg+Umschalt+Leertaste; nur der sichtbare Knopf reagiert.
  function shortcut() {
    if (host && host.offsetParent !== null) void toggle();
  }
  $effect(() => {
    window.addEventListener("iap-voice-toggle", shortcut);
    return () => window.removeEventListener("iap-voice-toggle", shortcut);
  });

  const title = $derived(blocked ? (voice.blockReason ?? t("Sprache ist nicht verfügbar")) : listening ? t("Aufnahme beenden und Text erkennen") : t("Sprechen (Strg+Umschalt+Leertaste)"));
</script>

<span class="v-mic" bind:this={host} class:compact class:live={listening} style={`--mic-level:${voice.level}`}>
  {#if listening}
    <button type="button" class="v-btn v-btn-primary v-mic-main" onclick={toggle} aria-label={t("Aufnahme beenden und Text erkennen")} {title}>
      <span class="v-mic-dot" aria-hidden="true"></span>
      {#if !compact}<span>{t("Aufnahme läuft")} · {timer}</span>{:else}<span>{timer}</span>{/if}
      <span class="v-mic-stop">{t("Stopp")}</span>
    </button>
    <button type="button" class="v-btn v-btn-ghost" onclick={() => discardListening()} aria-label={t("Aufnahme verwerfen")}>{t("Verwerfen")}</button>
  {:else if transcribing || working}
    <button type="button" class="v-btn v-btn-ghost v-mic-main" disabled aria-live="polite">
      <span class="v-mic-spin" aria-hidden="true"></span><span>{t("Erkennt Sprache …")}</span>
    </button>
    <button type="button" class="v-btn v-btn-ghost" onclick={() => discardListening()}>{t("Verwerfen")}</button>
  {:else}
    <button type="button" class="v-btn v-btn-ghost v-mic-main" disabled={blocked || voice.state !== "idle"} onclick={toggle} aria-label={t("Sprechen")} {title}>
      <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3ZM5 11a7 7 0 0 0 14 0M12 18v3M8 21h8"/></svg>
      {#if !compact}<span>{t("Sprechen")}</span>{/if}
    </button>
  {/if}
</span>

<style>
  .v-mic { display: inline-flex; flex-wrap: wrap; align-items: center; gap: var(--v-space-2); }
  .v-mic-main { gap: .5rem; }
  .v-mic-dot { width: .6rem; height: .6rem; border-radius: 50%; background: var(--v-danger); box-shadow: 0 0 0 calc(2px + var(--mic-level) * 8px) color-mix(in oklab, var(--v-danger) 30%, transparent); transition: box-shadow 90ms linear; }
  .v-mic-stop { padding-left: .5rem; border-left: 1px solid rgb(var(--v-tint) / .3); font-weight: 650; }
  .v-mic-spin { width: .9rem; height: .9rem; border: 2px solid var(--v-line-strong); border-top-color: var(--v-accent-blue); border-radius: 50%; animation: v-mic-spin 800ms linear infinite; }
  @keyframes v-mic-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .v-mic-spin { animation: none; } .v-mic-dot { transition: none; } }
</style>
