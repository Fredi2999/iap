<script lang="ts">
  // Angehängte Dokumente einer Unterhaltung. IAP legt pro Frage die
  // passenden Abschnitte daraus in den Prompt und nummeriert sie als Quellen.
  import { t } from "../i18n/index.svelte";
  import { conversationDocuments, importDocument, listDocuments, setDocumentAttached } from "../ipc";
  import { DOCUMENT_ACCEPT } from "../documents";
  import type { DocumentInfo } from "../types";

  interface Props {
    conversationId: string | null;
    /** Legt bei Bedarf eine Unterhaltung an, damit ein Dokument daran hängen kann. */
    ensureConversation: () => Promise<string>;
    pickerOpen: boolean;
    onError: (reason: unknown) => void;
    onManage: () => void;
  }
  let { conversationId, ensureConversation, pickerOpen = $bindable(false), onError, onManage }: Props = $props();

  let attached = $state<DocumentInfo[]>([]);
  let library = $state<DocumentInfo[]>([]);
  let importing = $state<string | null>(null);
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    const id = conversationId;
    if (!id) { attached = []; return; }
    conversationDocuments(id).then((documents) => { if (conversationId === id) attached = documents; }).catch(onError);
  });

  $effect(() => {
    if (pickerOpen) listDocuments().then((documents) => (library = documents)).catch(onError);
  });

  async function toggle(document: DocumentInfo, attach: boolean) {
    try {
      const id = conversationId ?? (await ensureConversation());
      await setDocumentAttached(id, document.id, attach);
      attached = attach ? [...attached.filter((item) => item.id !== document.id), document] : attached.filter((item) => item.id !== document.id);
    } catch (reason) { onError(reason); }
  }

  async function upload(event: Event) {
    const files = Array.from((event.target as HTMLInputElement).files ?? []);
    for (const file of files) {
      importing = file.name;
      try {
        const document = await importDocument(file);
        library = [document, ...library];
        await toggle(document, true);
      } catch (reason) { onError(reason); }
    }
    importing = null;
    if (input) input.value = "";
  }
</script>

{#if attached.length > 0 || pickerOpen}
  <div class="v-docbar">
    {#each attached as document (document.id)}
      <span class="v-doc-chip" title={document.name}>
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M6 3h9l4 4v14H6V3ZM15 3v4h4"/></svg>
        <span>{document.name}</span>
        <button type="button" aria-label={t("Abnehmen: {name}", { name: document.name })} onclick={() => toggle(document, false)}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg>
        </button>
      </span>
    {/each}
    <button type="button" class="v-doc-add" aria-expanded={pickerOpen} onclick={() => (pickerOpen = !pickerOpen)}>{t("Dokument anhängen")}</button>

    {#if pickerOpen}
      <div class="v-doc-picker" role="group" aria-label={t("Dokumente für diese Unterhaltung")}>
        <label class="v-btn v-btn-ghost v-file-button">
          {t(importing ? "Übernehme …" : "Neue Datei hochladen")}
          <input bind:this={input} type="file" accept={DOCUMENT_ACCEPT} multiple onchange={upload} disabled={importing !== null} />
        </label>
        {#if library.length > 0}
          <ul>
            {#each library as document (document.id)}
              {@const isAttached = attached.some((item) => item.id === document.id)}
              <li><label><input type="checkbox" checked={isAttached} onchange={() => toggle(document, !isAttached)} /> <span>{document.name}</span></label></li>
            {/each}
          </ul>
        {:else}
          <p>{t("Noch keine Dokumente im Tresor.")}</p>
        {/if}
        <div class="v-doc-picker-foot">
          <button type="button" class="v-link" onclick={onManage}>{t("Dokumente verwalten")}</button>
          <button type="button" class="v-btn v-btn-ghost" onclick={() => (pickerOpen = false)}>{t("Fertig")}</button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .v-docbar { position: relative; display: flex; flex-wrap: wrap; align-items: center; gap: var(--v-space-2); margin-bottom: var(--v-space-2); }
  .v-doc-chip { display: inline-flex; align-items: center; gap: 6px; max-width: 16rem; padding: 3px 4px 3px 10px; border: 1px solid var(--v-line); border-radius: 999px; background: rgb(var(--v-tint) / .05); color: var(--v-text-secondary); font-size: var(--v-text-xs); }
  .v-doc-chip > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .v-doc-chip button { display: inline-flex; padding: 3px; border: 0; border-radius: 999px; background: transparent; color: var(--v-text-muted); cursor: pointer; }
  .v-doc-chip button:hover { color: var(--v-text-primary); background: rgb(var(--v-tint) / .1); }
  .v-doc-add { padding: 3px 10px; border: 1px dashed var(--v-line-strong); border-radius: 999px; background: transparent; color: var(--v-text-muted); font-size: var(--v-text-xs); cursor: pointer; }
  .v-doc-add:hover { color: var(--v-text-primary); }
  .v-doc-picker { position: absolute; z-index: 14; bottom: calc(100% + .5rem); left: 0; width: min(24rem, calc(100vw - 2rem)); display: flex; flex-direction: column; gap: var(--v-space-3); padding: var(--v-space-4); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: 0 18px 40px rgb(var(--v-shade) / .4); }
  .v-doc-picker ul { max-height: 14rem; margin: 0; padding: 0; overflow-y: auto; list-style: none; }
  .v-doc-picker li label { display: flex; align-items: center; gap: var(--v-space-2); padding: 6px 2px; color: var(--v-text-secondary); font-size: var(--v-text-sm); cursor: pointer; }
  .v-doc-picker li span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .v-doc-picker p { margin: 0; color: var(--v-text-muted); font-size: var(--v-text-sm); }
  .v-doc-picker-foot { display: flex; align-items: center; justify-content: space-between; }
  .v-link { padding: 0; border: 0; background: transparent; color: var(--v-accent-blue); font-size: var(--v-text-sm); cursor: pointer; }
  /* Auswahl wächst aus dem Knopf unten links; kurz, weil sie oft geöffnet wird. */
  .v-doc-picker { transform-origin: bottom left; transition: opacity 150ms var(--v-ease-out-strong), transform 150ms var(--v-ease-out-strong); }
  @starting-style { .v-doc-picker { opacity: 0; transform: scale(.97); } }
  .v-doc-chip, .v-doc-add { transition: color 150ms ease, border-color 150ms ease, background-color 150ms ease; }
  @media (prefers-reduced-motion: reduce) { .v-doc-picker { transition: none; } }
</style>
