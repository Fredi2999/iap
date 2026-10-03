<script lang="ts">
  // Dokumentbibliothek: Dateien übernehmen, ansehen, löschen. Im Chat hängt man
  // sie an eine Unterhaltung; IAP zitiert dann die passenden Abschnitte.
  import { onMount } from "svelte";
  import { t, locale } from "../lib/i18n/index.svelte";
  import { deleteDocument, importDocument, listDocuments } from "../lib/ipc";
  import type { DocumentInfo } from "../lib/types";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import { notify } from "../lib/notifications";
  import { formatBytes, DOCUMENT_ACCEPT } from "../lib/documents";

  let documents = $state<DocumentInfo[]>([]);
  let loaded = $state(false);
  let importing = $state<string | null>(null);
  let error = $state<unknown>(null);
  let input = $state<HTMLInputElement | null>(null);

  onMount(refresh);

  async function refresh() {
    try { documents = await listDocuments(); } catch (reason) { error = reason; }
    loaded = true;
  }

  async function onFiles(event: Event) {
    const files = Array.from((event.target as HTMLInputElement).files ?? []);
    error = null;
    for (const file of files) {
      importing = file.name;
      try {
        const document = await importDocument(file);
        notify("Dokument übernommen", t("{name}: {n} Abschnitte", { name: document.name, n: document.chunk_count }), "success");
      } catch (reason) {
        error = reason;
      }
    }
    importing = null;
    if (input) input.value = "";
    await refresh();
  }

  async function remove(document: DocumentInfo) {
    if (!confirm(t("„{name}“ wirklich löschen? Antworten, die es zitieren, behalten ihre Auszüge.", { name: document.name }))) return;
    try {
      await deleteDocument(document.id);
      await refresh();
    } catch (reason) { error = reason; }
  }
</script>

<div class="v-page">
  <PageHeader title={t("Dokumente")} description="Eigene Dateien, auf die IAP im Chat mit Quellenangabe antworten kann.">
    {#snippet actions()}
      <label class="v-btn v-btn-primary v-file-button">
        {t(importing ? "Übernehme …" : "Dokument hinzufügen")}
        <input bind:this={input} type="file" accept={DOCUMENT_ACCEPT} multiple onchange={onFiles} disabled={importing !== null} />
      </label>
    {/snippet}
    {#snippet help()}{t("Unterstützt werden TXT, Markdown, Word (DOCX) und PDF bis 25 MB. Die Texte liegen verschlüsselt im Tresor. Im Chat hängst du ein Dokument über „+“ oder „/dokument“ an eine Unterhaltung.")}{/snippet}
  </PageHeader>

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}
  {#if importing}<p class="v-notice" aria-live="polite">{t("„{name}“ wird gelesen und in Abschnitte geteilt …", { name: importing })}</p>{/if}

  <section class="v-card">
    {#if loaded && documents.length === 0}
      <EmptyState title={t("Noch keine Dokumente")} text="Füge oben eine Datei hinzu. Danach kannst du sie im Chat anhängen und Fragen dazu stellen." icon="M6 3h9l4 4v14H6V3ZM15 3v4h4M9 12h6M9 16h4" />
    {:else}
      <ul class="v-list">
        {#each documents as document (document.id)}
          <li>
            <span class="v-doc-kind">{document.kind.toUpperCase()}</span>
            <div class="v-list-main">
              <strong>{document.name}</strong>
              <span>{formatBytes(document.size_bytes)} · {t("{n} Abschnitte", { n: document.chunk_count })} · {new Date(document.created_at_unix_ms).toLocaleDateString(locale())}</span>
            </div>
            <button type="button" class="v-btn-icon" title={t("Löschen")} aria-label={t("Löschen: {name}", { name: document.name })} onclick={() => remove(document)}>
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3m-9 0 1 13h10l1-13"/></svg>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<style>
  .v-doc-kind { flex: 0 0 auto; min-width: 2.8rem; padding: 2px 6px; border: 1px solid var(--v-line); border-radius: var(--v-radius-control); color: var(--v-text-muted); font-size: var(--v-text-xs); font-weight: 600; text-align: center; }
</style>
