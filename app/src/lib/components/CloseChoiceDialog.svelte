<script lang="ts">
  // Erster Hinweis beim Schließen des Hauptfensters: Schließen lässt IAP als
  // kleines Pet weiterlaufen, „vollständig beenden“ beendet alles. Der Hinweis
  // erscheint nur einmal; danach wechselt das Schließen direkt zum Pet.
  import { onMount } from "svelte";
  import { t } from "../i18n/index.svelte";

  interface Props {
    onPet: () => void;
    onQuit: () => void;
    onCancel: () => void;
  }
  let { onPet, onQuit, onCancel }: Props = $props();
  let primary = $state<HTMLButtonElement | null>(null);

  onMount(() => {
    primary?.focus();
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); onCancel(); } };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<div class="v-close-layer">
  <div class="v-close-shade" aria-hidden="true"></div>
  <div class="v-close" role="alertdialog" aria-modal="true" aria-labelledby="v-close-title" aria-describedby="v-close-text">
    <h2 id="v-close-title">{t("Fenster schließen oder IAP beenden?")}</h2>
    <div id="v-close-text" class="v-close-text">
      <p><strong>{t("Fenster schließen")}</strong> {t("blendet das Hauptfenster aus. IAP läuft als kleines Pet unten rechts weiter, der Tresor bleibt offen. Workflows und Agent-Flow-Läufe werden dabei gestoppt; Ergebnisse bleiben erhalten.")}</p>
      <p><strong>{t("IAP vollständig beenden")}</strong> {t("schließt Pet, Sprache, Modell und Tresor und beendet alle Prozesse von IAP. Über das Symbol im Infobereich der Taskleiste geht beides jederzeit.")}</p>
    </div>
    <div class="v-close-actions">
      <button type="button" class="v-btn v-btn-ghost" onclick={onCancel}>{t("Abbrechen")}</button>
      <button type="button" class="v-btn v-btn-ghost" onclick={onQuit}>{t("IAP vollständig beenden")}</button>
      <button type="button" class="v-btn v-btn-primary" bind:this={primary} onclick={onPet}>{t("Als Pet weiterlaufen")}</button>
    </div>
  </div>
</div>

<style>
  .v-close-layer { position: fixed; inset: 0; z-index: 90; display: flex; align-items: center; justify-content: center; padding: 1rem; }
  .v-close-shade { position: absolute; inset: 0; background: rgb(var(--v-shade) / .5); }
  .v-close { position: relative; width: min(34rem, 100%); display: flex; flex-direction: column; gap: var(--v-space-4); padding: var(--v-space-6); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: 0 25px 70px rgb(var(--v-shade) / .42); }
  h2 { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-lg); font-weight: 600; }
  .v-close-text { display: grid; gap: var(--v-space-3); color: var(--v-text-secondary); font-size: var(--v-text-sm); line-height: 1.55; }
  .v-close-text p { margin: 0; }
  .v-close-text strong { color: var(--v-text-primary); font-weight: 620; }
  .v-close-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--v-space-2); }
</style>
