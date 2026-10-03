<script lang="ts">
  import { t, locale } from "../lib/i18n/index.svelte";
  import { errorText } from "../lib/errors";
  import { onDestroy } from "svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import RecordButton from "../lib/components/RecordButton.svelte";
  import { pointerGlow } from "../lib/actions/pointerGlow";
  import { notify } from "../lib/notifications";

  interface Recording {
    id: string;
    started_at: number;
    duration_ms: number;
    url: string;
    blob: Blob;
  }

  let mediaRecorder: MediaRecorder | null = null;
  let stream: MediaStream | null = null;
  let recording = $state(false);
  let starting = $state(false);
  let elapsedMs = $state(0);
  let startTime = 0;
  let interval: number | null = null;
  let recordings = $state<Recording[]>([]);
  let error = $state<unknown>(null);
  let levels = $state<number[]>(new Array(24).fill(0));
  let audioContext: AudioContext | null = null;
  let analyser: AnalyserNode | null = null;
  let rafId: number | null = null;
  let finalizeRecording: (save: boolean) => void = () => {};
  let disposed = false;

  async function start() {
    if (recording || starting) return;
    starting = true;
    error = null;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      if (disposed) { stream.getTracks().forEach((track) => track.stop()); stream = null; return; }
      mediaRecorder = new MediaRecorder(stream);
      const chunks: Blob[] = [];
      let saveRecording = true;
      finalizeRecording = (save) => { saveRecording = save; };
      mediaRecorder.ondataavailable = (e) => e.data.size && chunks.push(e.data);
      mediaRecorder.onstop = () => {
        if (!saveRecording) return;
        const blob = new Blob(chunks, { type: "audio/webm" });
        recordings = [{
          id: `rec-${Date.now()}`,
          started_at: startTime,
          duration_ms: Date.now() - startTime,
          url: URL.createObjectURL(blob),
          blob,
        }, ...recordings];
        notify("Aufnahme bereit", "Mit Export dauerhaft speichern.", "success");
      };
      mediaRecorder.start(200);
      recording = true;
      startTime = Date.now();
      interval = window.setInterval(() => (elapsedMs = Date.now() - startTime), 100);

      // Live-Level für Wellenform
      audioContext = new AudioContext();
      const source = audioContext.createMediaStreamSource(stream);
      analyser = audioContext.createAnalyser();
      analyser.fftSize = 64;
      source.connect(analyser);
      const data = new Uint8Array(analyser.frequencyBinCount);
      const draw = () => {
        if (!analyser) return;
        analyser.getByteFrequencyData(data);
        levels = Array.from(data).slice(0, 24).map((v) => v / 255);
        rafId = requestAnimationFrame(draw);
      };
      draw();
    } catch (e) {
      error = t("Mikrofon nicht verfügbar: {reason}", { reason: errorText(e) });
      notify("Mikrofon nicht verfügbar", errorText(e), "error");
      finalizeRecording(false);
      if (mediaRecorder?.state === "recording") mediaRecorder.stop();
      stream?.getTracks().forEach((track) => track.stop());
      stream = null;
      if (interval) clearInterval(interval);
      if (rafId) cancelAnimationFrame(rafId);
      if (audioContext) void audioContext.close();
      audioContext = null;
      analyser = null;
      recording = false;
    } finally {
      starting = false;
    }
  }

  function stop(save = true, quiet = false) {
    finalizeRecording(save);
    if (mediaRecorder?.state === "recording") mediaRecorder.stop();
    stream?.getTracks().forEach((track) => track.stop());
    if (interval) clearInterval(interval);
    if (rafId) cancelAnimationFrame(rafId);
    if (audioContext) audioContext.close();
    audioContext = null;
    analyser = null;
    mediaRecorder = null;
    stream = null;
    recording = false;
    if (!save && !quiet) notify("Aufnahme verworfen", "Es wurde keine Datei erzeugt.");
    elapsedMs = 0;
    levels = new Array(24).fill(0);
  }

  function download(rec: Recording) {
    const a = document.createElement("a");
    a.href = rec.url;
    a.download = `${rec.id}.webm`;
    a.click();
    notify("Wird gespeichert", "Die Aufnahme wird als Audiodatei abgelegt.", "success");
  }

  function remove(id: string) {
    const item = recordings.find((rec) => rec.id === id);
    if (item) URL.revokeObjectURL(item.url);
    recordings = recordings.filter((r) => r.id !== id);
  }

  function fmtMs(ms: number) {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60).toString().padStart(2, "0")}:${(s % 60).toString().padStart(2, "0")}`;
  }

  onDestroy(() => {
    disposed = true;
    if (recording) stop(false, true);
    recordings.forEach((r) => URL.revokeObjectURL(r.url));
  });
</script>

<div class="v-page v-voice">
  <PageHeader title={t("Sprachnotizen")} description="Sprich eine Notiz ein, hör sie an und speichere sie als Audiodatei. Aufnahmen bleiben nur bis zum Schließen, außer du speicherst sie." />

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  <section use:pointerGlow class="v-card v-stack v-voice-console">
    <div class="v-wave" aria-hidden="true">
      {#each levels as level, i (i)}
        <span class:live={recording} style={`transform: scaleY(${Math.max(0.1, level)})`}></span>
      {/each}
    </div>
    <!-- Die Kapsel nutzt den echten Mikrofonpegel und verwirft nur auf Wunsch. -->
    <RecordButton {recording} {elapsedMs} {levels} busy={starting} onToggle={() => { if (recording) stop(); else void start(); }} onCancel={() => stop(false)} />
  </section>

  <section class="v-card v-stack">
    <h2 class="v-card-title">{t("Aufnahmen")} <small>{recordings.length}</small></h2>
    {#if recordings.length === 0}
      <EmptyState title={t("Noch keine Aufnahmen")} text="Tippe oben auf Aufnehmen und sprich los." icon="M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3ZM5 11a7 7 0 0 0 14 0M12 18v3" />
    {:else}
      <ul class="v-list">
        {#each recordings as rec (rec.id)}
          <li>
            <audio src={rec.url} controls class="v-audio"></audio>
            <span class="v-num v-help">{fmtMs(rec.duration_ms)} · {new Date(rec.started_at).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" })}</span>
            <button class="v-btn v-btn-ghost" onclick={() => download(rec)}>{t("Speichern")}</button>
            <button class="v-btn-icon" onclick={() => remove(rec.id)} aria-label={t("Aufnahme löschen")} title={t("Löschen")}>
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3m-9 0 1 13h10l1-13"/></svg>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<style>
  .v-voice { max-width: 52rem; margin-inline: auto; }
  .v-voice-console { align-items: center; padding-block: var(--v-space-5); }
  .v-wave { display: flex; align-items: center; justify-content: center; gap: 5px; width: 100%; height: 5rem; }
  .v-wave span { width: 6px; height: 64px; border-radius: 9999px; background: rgb(var(--v-tint) / .2); transform-origin: center; transition: transform 90ms linear; }
  .v-wave span.live { background: var(--v-accent-blue); }
  .v-audio { flex: 1 1 auto; min-width: 0; height: 2.25rem; }
  @media (prefers-reduced-motion: reduce) { .v-wave span { transition: none; } }
</style>
