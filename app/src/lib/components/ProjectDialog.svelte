<script lang="ts">
  // Projekt anlegen oder bearbeiten. Die Grundanweisung gilt für jede
  // Unterhaltung im Projekt, etwa „Antworte für die 3B in einfacher Sprache“.
  import { onMount, untrack } from "svelte";
  import { t } from "../i18n/index.svelte";
  import { deleteProject, saveProject } from "../ipc";
  import { friendlyError } from "../errors";
  import type { Project } from "../types";

  interface Props {
    project: Project | null;
    onClose: () => void;
    onSaved: (project: Project | null) => void;
  }
  let { project, onClose, onSaved }: Props = $props();

  // Startwerte bewusst einmalig übernehmen; danach gehört der Entwurf dem Dialog.
  let name = $state(untrack(() => project?.name ?? ""));
  let systemPrompt = $state(untrack(() => project?.system_prompt ?? ""));
  let busy = $state(false);
  let error = $state<string | null>(null);
  let nameField = $state<HTMLInputElement | null>(null);

  onMount(() => {
    nameField?.focus();
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape" && !busy) onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!name.trim()) return;
    busy = true;
    error = null;
    try {
      onSaved(await saveProject(project?.id ?? null, name, systemPrompt));
    } catch (reason) {
      error = friendlyError(reason).message;
      busy = false;
    }
  }

  async function remove() {
    if (!project || !confirm(t("Projekt „{name}“ löschen? Die Unterhaltungen bleiben erhalten.", { name: project.name }))) return;
    busy = true;
    try {
      await deleteProject(project.id);
      onSaved(null);
    } catch (reason) {
      error = friendlyError(reason).message;
      busy = false;
    }
  }
</script>

<div class="v-project-layer">
  <button type="button" class="v-project-shade" aria-label={t("Schließen")} onclick={onClose}></button>
  <div class="v-project" role="dialog" aria-modal="true" aria-labelledby="v-project-title">
  <form class="v-project-form" onsubmit={save}>
    <h2 id="v-project-title">{t(project ? "Projekt bearbeiten" : "Neues Projekt")}</h2>
    <label class="v-field"><span class="v-label">{t("Name")}</span>
      <input bind:this={nameField} bind:value={name} maxlength="80" placeholder={t("Zum Beispiel: Mathematik 3B")} />
    </label>
    <label class="v-field"><span class="v-label">{t("Grundanweisung")}</span>
      <textarea bind:value={systemPrompt} rows="5" maxlength="4000" placeholder={t("Zum Beispiel: Erkläre alles für 13-Jährige und nenne am Ende eine Übungsaufgabe.")}></textarea>
      <span data-hint class="v-help">{t("Gilt für jede Unterhaltung in diesem Projekt.")}</span>
    </label>
    {#if error}<p class="v-project-error" role="alert">{t(error)}</p>{/if}
    <div class="v-row v-project-actions">
      {#if project}<button type="button" class="v-btn v-btn-ghost v-project-delete" onclick={remove} disabled={busy}>{t("Löschen")}</button>{/if}
      <span class="v-project-spacer"></span>
      <button type="button" class="v-btn v-btn-ghost" onclick={onClose} disabled={busy}>{t("Abbrechen")}</button>
      <button type="submit" class="v-btn v-btn-primary" disabled={busy || !name.trim()}>{t(busy ? "Speichere …" : "Speichern")}</button>
    </div>
  </form>
  </div>
</div>

<style>
  .v-project-layer { position: fixed; inset: 0; z-index: 85; display: flex; align-items: center; justify-content: center; padding: 1rem; }
  .v-project-shade { position: absolute; inset: 0; border: 0; border-radius: 0; background: rgb(var(--v-shade) / .5); cursor: default; }
  .v-project { position: relative; width: min(32rem, 100%); display: flex; flex-direction: column; gap: var(--v-space-4); padding: var(--v-space-6); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: 0 25px 70px rgb(var(--v-shade) / .42); }
  .v-project-form { display: flex; flex-direction: column; gap: var(--v-space-4); }
  h2 { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-lg); font-weight: 600; }
  textarea { resize: vertical; }
  .v-project-actions { gap: var(--v-space-2); }
  .v-project-spacer { flex: 1; }
  .v-project-delete { color: var(--v-danger); }
  .v-project-error { margin: 0; color: var(--v-danger); font-size: var(--v-text-sm); }
  /* Dialoge erscheinen zentriert: kurz aus 96 % mit Blende, ohne Federn (Emil: Modals bleiben mittig). */
  .v-project { transition: opacity 200ms var(--v-ease-out-strong), transform 200ms var(--v-ease-out-strong); }
  .v-project-shade { transition: opacity 200ms ease; }
  @starting-style { .v-project { opacity: 0; transform: scale(.96); } .v-project-shade { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .v-project { transition: opacity 120ms ease; } @starting-style { .v-project { transform: none; } } }
</style>
