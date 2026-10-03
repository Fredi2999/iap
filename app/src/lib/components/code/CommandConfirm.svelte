<script lang="ts">
  // Bestätigung eines Befehls, den der Code-Agent ausführen möchte. Der Dialog nennt offen, was
  // die Prüfung nicht verhindern kann: Es gibt keine Sandbox. Abgelehnt ist die sichere Vorgabe.
  import { onMount } from "svelte";
  import { t } from "../../i18n/index.svelte";
  import type { CommandPrompt } from "../../types";

  interface Props {
    prompt: CommandPrompt;
    onAnswer: (allow: boolean) => void;
  }
  let { prompt, onAnswer }: Props = $props();
  let refuse = $state<HTMLButtonElement | null>(null);

  onMount(() => {
    refuse?.focus();
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); onAnswer(false); } };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<div class="v-cmd-layer">
  <button type="button" class="v-cmd-shade" aria-label={t("Ablehnen")} onclick={() => onAnswer(false)}></button>
  <div class="v-cmd" role="alertdialog" aria-modal="true" aria-labelledby="v-cmd-title" aria-describedby="v-cmd-text">
    <h2 id="v-cmd-title">{t("Diesen Befehl ausführen?")}</h2>
    <pre class="v-cmd-line v-num">{prompt.command}</pre>
    <p class="v-help v-num">{t("Ordner: {ordner} · Zeitgrenze: {s} s", { ordner: prompt.cwd, s: prompt.timeout_seconds })}</p>
    {#if prompt.derived_from_content}
      <p class="v-notice warn" role="alert">{t("Achtung: Dieser Befehl stammt aus dem Inhalt einer gelesenen Datei, nicht aus deiner Aufgabe. Führe ihn nur aus, wenn du ihn erkennst und willst.")}</p>
    {/if}
    <p id="v-cmd-text" class="v-cmd-text">{t("IAP kann nicht verhindern, dass das Programm selbst Dateien außerhalb des Projekts ändert oder das Netz nutzt (ein Build lädt zum Beispiel Pakete). Führe nur Befehle aus, die du verstehst.")}</p>
    <div class="v-cmd-actions">
      <button type="button" class="v-btn v-btn-primary" bind:this={refuse} onclick={() => onAnswer(false)}>{t("Ablehnen")}</button>
      <button type="button" class="v-btn v-btn-ghost" onclick={() => onAnswer(true)}>{t("Ausführen")}</button>
    </div>
  </div>
</div>

<style>
  .v-cmd-layer { position: fixed; inset: 0; z-index: 95; display: flex; align-items: center; justify-content: center; padding: 1rem; }
  .v-cmd-shade { position: absolute; inset: 0; border: 0; background: rgb(var(--v-shade) / .5); cursor: default; }
  .v-cmd { position: relative; width: min(36rem, 100%); display: flex; flex-direction: column; gap: var(--v-space-3); padding: var(--v-space-6); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: 0 25px 70px rgb(var(--v-shade) / .42); }
  h2 { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-lg); font-weight: 600; }
  .v-cmd-line { margin: 0; padding: var(--v-space-3); overflow-x: auto; border-radius: var(--v-radius-control); background: var(--v-surface-muted, rgb(var(--v-shade) / .08)); color: var(--v-text-primary); font-size: var(--v-text-sm); white-space: pre-wrap; overflow-wrap: anywhere; }
  .v-cmd-text { margin: 0; color: var(--v-text-secondary); font-size: var(--v-text-sm); line-height: 1.55; }
  .v-cmd-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--v-space-2); }
</style>
