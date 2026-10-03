<script lang="ts">
  import { t, locale } from "../lib/i18n/index.svelte";
  import { onMount, untrack } from "svelte";
  import { listWorkspace, readWorkspaceFile } from "../lib/ipc";
  import type { WorkspaceEntry, WorkspaceListing } from "../lib/types";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";

  let listing = $state<WorkspaceListing | null>(null);
  let currentPath = $state<string>("");
  let selectedFile = $state<string | null>(null);
  let preview = $state<string>("");
  let error = $state<unknown>(null);

  const FOLDER_ICON = "M4 5a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5Z";
  const FILE_ICON = "M6 3h8l4 4v14H6zM14 3v4h4";

  onMount(() => { untrack(() => reload("")); });

  async function reload(path: string) {
    error = null;
    try {
      const response = await listWorkspace(path);
      listing = response;
      currentPath = response.relative_path;
      selectedFile = null;
      preview = "";
    } catch (reason) {
      error = reason;
    }
  }

  async function openFile(path: string) {
    error = null;
    try {
      preview = await readWorkspaceFile(path);
      selectedFile = path;
    } catch (reason) {
      error = reason;
    }
  }

  function enter(entry: WorkspaceEntry) {
    if (entry.is_directory) void reload(entry.relative_path);
    else void openFile(entry.relative_path);
  }

  let crumbs = $derived(currentPath ? currentPath.split("/") : []);

  function humanBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
</script>

<div class="v-page">
  <PageHeader title={t("Dateien")} description="Dein Arbeitsordner auf dem Stick. Werkzeuge von IAP lesen und schreiben nur hier, und jede Änderung musst du bestätigen." />

  {#if error}<ErrorNotice {error} onRetry={() => reload(currentPath)} onDismiss={() => (error = null)} />{/if}

  <div class="v-files">
    <section class="v-card v-files-list" aria-label={t("Ordnerinhalt")}>
      <nav class="v-crumbs" aria-label={t("Pfad")}>
        <button type="button" onclick={() => reload("")} class:current={crumbs.length === 0}>{t("Arbeitsordner")}</button>
        {#each crumbs as crumb, index (index)}
          <span aria-hidden="true">/</span>
          <button type="button" class:current={index === crumbs.length - 1} onclick={() => reload(crumbs.slice(0, index + 1).join("/"))}>{crumb}</button>
        {/each}
      </nav>
      {#if listing}
        {#if listing.entries.length === 0}
          <EmptyState title={t("Dieser Ordner ist leer")} icon={FOLDER_ICON} />
        {:else}
          <ul class="v-list">
            {#each listing.entries as entry (entry.relative_path)}
              <li class:selected={selectedFile === entry.relative_path}>
                <button type="button" class="v-file-row" onclick={() => enter(entry)}>
                  <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" aria-hidden="true" class:folder={entry.is_directory}><path d={entry.is_directory ? FOLDER_ICON : FILE_ICON}/></svg>
                  <span class="v-file-name">{entry.name}</span>
                  <span class="v-help v-num">{entry.is_directory ? "" : humanBytes(entry.bytes)}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}
    </section>

    <section class="v-card v-files-preview" aria-label={t("Vorschau")}>
      {#if selectedFile}
        <h2 class="v-card-title">{selectedFile.split("/").pop()} <small>{t("Vorschau, bis 64 KB")}</small></h2>
        <pre>{preview}</pre>
      {:else}
        <EmptyState title={t("Keine Datei gewählt")} text="Wähle links eine Datei, um sie hier anzusehen." icon={FILE_ICON} />
      {/if}
    </section>
  </div>
</div>

<style>
  .v-files { display: grid; grid-template-columns: minmax(16rem, 22rem) minmax(0, 1fr); gap: var(--v-space-4); align-items: start; }
  @media (max-width: 900px) { .v-files { grid-template-columns: 1fr; } }
  .v-crumbs { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; margin-bottom: var(--v-space-3); color: var(--v-text-muted); font-size: var(--v-text-sm); }
  .v-crumbs button { padding: 2px 6px; border: 0; border-radius: 6px; background: transparent; color: var(--v-accent-blue); font-size: inherit; }
  .v-crumbs button.current { color: var(--v-text-primary); font-weight: 600; }
  .v-crumbs button:hover { background: rgb(var(--v-tint) / .07); }
  .v-files-list .v-list > li { padding: 0; }
  .v-files-list .v-list > li.selected { background: rgb(var(--v-tint) / .1); border-radius: var(--v-radius-control); }
  .v-file-row { display: flex; align-items: center; gap: var(--v-space-3); width: 100%; min-height: 2.5rem; padding: 0 var(--v-space-3); border: 0; border-radius: var(--v-radius-control); background: transparent; color: var(--v-text-primary); font-size: var(--v-text-sm); text-align: left; }
  .v-file-row:hover { background: rgb(var(--v-tint) / .06); }
  .v-file-row:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: -2px; }
  .v-file-row svg { flex: 0 0 auto; color: var(--v-text-muted); }
  .v-file-row svg.folder { color: var(--v-accent-blue); }
  .v-file-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .v-files-preview pre { max-height: 62vh; margin: 0; padding: var(--v-space-3); overflow: auto; border-radius: var(--v-radius-field); background: rgb(var(--v-shade) / .18); color: var(--v-text-primary); font-family: var(--v-font-mono); font-size: var(--v-text-xs); white-space: pre-wrap; }
</style>
