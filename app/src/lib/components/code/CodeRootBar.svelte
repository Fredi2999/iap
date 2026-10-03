<script lang="ts">
  // Leiste über dem Code-Bereich: wo IAP gerade arbeitet (Stick oder ein Ordner auf dem PC)
  // und wie man den Ordner wechselt. Ein Ordner auf dem PC braucht einmal eine bestätigte
  // Freigabe; IAP merkt sie und der Nutzer kann sie jederzeit widerrufen.
  import { onMount } from "svelte";
  import { t } from "../../i18n/index.svelte";
  import {
    codeRootCheck,
    codeRootOpenHost,
    codeRootRevoke,
    codeRootsStatus,
    codeRootUseStick,
    pickFolderDialog,
  } from "../../ipc";
  import type { CodeRootsStatus } from "../../types";
  import ErrorNotice from "../ErrorNotice.svelte";

  interface Props {
    /** Fragt die Seite, ob gewechselt werden darf (z. B. wegen ungespeicherter Dateien). */
    canSwitch: () => boolean;
    /** Wird nach einem erfolgreichen Wechsel aufgerufen; die Seite lädt dann alles neu. */
    onSwitched: (status: CodeRootsStatus) => void;
  }
  let { canSwitch, onSwitched }: Props = $props();

  let roots = $state<CodeRootsStatus | null>(null);
  let pending = $state<string | null>(null);
  let busy = $state(false);
  let error = $state<unknown>(null);
  let note = $state("");
  let confirmButton = $state<HTMLButtonElement | null>(null);

  let onHost = $derived(roots?.host_path != null);

  onMount(() => {
    void codeRootsStatus().then((status) => (roots = status)).catch((reason) => (error = reason));
  });

  /** Führt einen Wechsel aus, nachdem die Seite zugestimmt hat. */
  async function switchTo(work: () => Promise<CodeRootsStatus>) {
    if (!canSwitch()) {
      note = t("Speichere oder schließe zuerst die Dateien mit ungespeicherten Änderungen.");
      return;
    }
    note = "";
    busy = true;
    error = null;
    try {
      roots = await work();
      onSwitched(roots);
    } catch (reason) {
      error = reason;
    } finally {
      busy = false;
    }
  }

  async function chooseFolder() {
    if (!canSwitch()) {
      note = t("Speichere oder schließe zuerst die Dateien mit ungespeicherten Änderungen.");
      return;
    }
    note = "";
    error = null;
    try {
      const chosen = await pickFolderDialog(t("Ordner auf dem PC wählen"));
      if (!chosen) return;
      const check = await codeRootCheck(chosen);
      if (check.approved) await switchTo(() => codeRootOpenHost(check.path, false));
      else {
        pending = check.path;
        queueMicrotask(() => confirmButton?.focus());
      }
    } catch (reason) {
      error = reason;
    }
  }

  async function confirm() {
    const path = pending;
    pending = null;
    if (path) await switchTo(() => codeRootOpenHost(path, true));
  }

  async function revoke(path: string) {
    error = null;
    try {
      const wasActive = roots?.host_path?.toLowerCase() === path.toLowerCase();
      roots = await codeRootRevoke(path);
      if (wasActive) onSwitched(roots);
    } catch (reason) {
      error = reason;
    }
  }
</script>

<section class="v-card v-code-root" aria-label={t("Arbeitsordner")}>
  <div class="v-code-root-main">
    <span class="v-chip" class:accent={onHost}>{onHost ? t("Ordner auf dem PC") : t("Arbeitsordner auf dem Stick")}</span>
    <span class="v-code-root-path v-num" title={roots?.host_path ?? roots?.stick_path ?? ""}>{roots?.host_path ?? roots?.stick_path ?? ""}</span>
    <span class="v-row v-code-root-actions">
      <button type="button" class="v-btn v-btn-ghost" onclick={chooseFolder} disabled={busy}>{t("Ordner auf dem PC öffnen …")}</button>
      {#if onHost}
        <button type="button" class="v-btn v-btn-ghost" onclick={() => switchTo(codeRootUseStick)} disabled={busy}>{t("Zurück zum Stick")}</button>
      {/if}
    </span>
  </div>
  {#if note}<p class="v-help" role="status">{note}</p>{/if}
  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}
  {#if roots && roots.approved.length > 0}
    <details class="v-code-root-list">
      <summary>{t("Freigegebene Ordner ({n})", { n: roots.approved.length })}</summary>
      <ul class="v-list">
        {#each roots.approved as path (path)}
          <li>
            <div class="v-list-main"><strong class="v-num v-code-root-item">{path}</strong></div>
            <button type="button" class="v-btn v-btn-ghost" onclick={() => switchTo(() => codeRootOpenHost(path, false))} disabled={busy}>{t("Öffnen")}</button>
            <button type="button" class="v-btn v-btn-ghost" onclick={() => revoke(path)} disabled={busy}>{t("Freigabe widerrufen")}</button>
          </li>
        {/each}
      </ul>
    </details>
  {/if}
</section>

{#if pending}
  <div class="v-root-layer">
    <button type="button" class="v-root-shade" aria-label={t("Abbrechen")} onclick={() => (pending = null)}></button>
    <div class="v-root-dialog" role="alertdialog" aria-modal="true" aria-labelledby="v-root-title" aria-describedby="v-root-text">
      <h2 id="v-root-title">{t("Diesen Ordner für IAP freigeben?")}</h2>
      <p class="v-root-path v-num">{pending}</p>
      <div id="v-root-text" class="v-root-text">
        <p>{t("IAP darf hier lesen, durchsuchen und Änderungen vorschlagen. Gespeichert wird erst nach deiner Bestätigung.")}</p>
        <p>{t("IAP löscht nichts und legt keine versteckten Ordner an. Außerhalb des Ordners gibt es keinen Zugriff. Die Freigabe liegt verschlüsselt im Tresor und lässt sich jederzeit widerrufen.")}</p>
      </div>
      <div class="v-root-actions">
        <button type="button" class="v-btn v-btn-ghost" onclick={() => (pending = null)}>{t("Abbrechen")}</button>
        <button type="button" class="v-btn v-btn-primary" bind:this={confirmButton} onclick={confirm}>{t("Freigeben und öffnen")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .v-code-root { display: grid; gap: var(--v-space-2); }
  .v-code-root-main { display: flex; flex-wrap: wrap; align-items: center; gap: var(--v-space-2) var(--v-space-3); min-width: 0; }
  .v-code-root-path { min-width: 0; flex: 1 1 12rem; overflow: hidden; color: var(--v-text-secondary); font-size: var(--v-text-sm); text-overflow: ellipsis; white-space: nowrap; }
  .v-code-root-actions { flex-wrap: wrap; gap: var(--v-space-2); }
  .v-code-root-list summary { cursor: pointer; color: var(--v-text-secondary); font-size: var(--v-text-sm); }
  .v-code-root-item { overflow: hidden; font-weight: 500; text-overflow: ellipsis; white-space: nowrap; }
  .v-root-layer { position: fixed; inset: 0; z-index: 90; display: flex; align-items: center; justify-content: center; padding: 1rem; }
  .v-root-shade { position: absolute; inset: 0; border: 0; background: rgb(var(--v-shade) / .5); cursor: default; }
  .v-root-dialog { position: relative; width: min(34rem, 100%); display: flex; flex-direction: column; gap: var(--v-space-4); padding: var(--v-space-6); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: 0 25px 70px rgb(var(--v-shade) / .42); }
  h2 { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-lg); font-weight: 600; }
  .v-root-path { margin: 0; padding: var(--v-space-2) var(--v-space-3); overflow-wrap: anywhere; border-radius: var(--v-radius-control); background: var(--v-surface-muted, rgb(var(--v-shade) / .08)); color: var(--v-text-primary); font-size: var(--v-text-sm); }
  .v-root-text { display: grid; gap: var(--v-space-3); color: var(--v-text-secondary); font-size: var(--v-text-sm); line-height: 1.55; }
  .v-root-text p { margin: 0; }
  .v-root-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--v-space-2); }
</style>
