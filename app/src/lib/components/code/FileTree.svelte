<script lang="ts">
  // Aufklappbarer Dateibaum des Code-Bereichs: Ordner öffnen sich an Ort und Stelle, geänderte
  // Dateien tragen ein Git-Kürzel, Einträge lassen sich per Ziehen in andere Ordner verschieben.
  import { t } from "../../i18n/index.svelte";
  import { changedFolders } from "../../stores/code.svelte";
  import type { WorkspaceEntry } from "../../types";

  interface Props {
    children: Record<string, WorkspaceEntry[]>;
    expanded: Record<string, boolean>;
    marks: Record<string, string>;
    /** Ordner, in dem „Neu“ anlegt. */
    selectedDir: string;
    activePath: string | null;
    openPaths: string[];
    hasUnsaved: (path: string) => boolean;
    onToggle: (entry: WorkspaceEntry) => void;
    onOpen: (entry: WorkspaceEntry) => void;
    onSelectDir: (path: string) => void;
    onRename: (path: string, name: string) => Promise<void>;
    onDelete: (path: string) => Promise<void>;
    onMove: (from: string, toDir: string) => Promise<void>;
    /** Im Ordner auf dem PC löscht IAP nichts (kein Papierkorb-Ordner im Projekt). */
    canDelete?: boolean;
  }
  let { children, expanded, marks, selectedDir, activePath, openPaths, hasUnsaved, onToggle, onOpen, onSelectDir, onRename, onDelete, onMove, canDelete = true }: Props = $props();

  let renaming = $state<{ path: string; value: string } | null>(null);
  let deleting = $state<string | null>(null);
  let dropTarget = $state<string | null>(null);
  let root = $state<HTMLUListElement | null>(null);

  const folderMarks = $derived(changedFolders(marks));
  const ICON_FOLDER = "M4 5a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5Z";
  const ICON_FILE = "M6 3h8l4 4v14H6zM14 3v4h4";

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function parentOf(path: string): string {
    const index = path.lastIndexOf("/");
    return index < 0 ? "" : path.slice(0, index);
  }

  function markFor(entry: WorkspaceEntry): string {
    if (entry.is_directory) return folderMarks.has(entry.relative_path) ? "•" : "";
    return marks[entry.relative_path] ?? "";
  }

  async function submitRename(event: SubmitEvent) {
    event.preventDefault();
    if (!renaming) return;
    const { path, value } = renaming;
    renaming = null;
    await onRename(path, value);
  }

  async function confirmDelete(path: string) {
    deleting = null;
    await onDelete(path);
  }

  // --- Verschieben per Ziehen ------------------------------------------------

  function canDrop(from: string | null, toDir: string): boolean {
    if (!from) return false;
    if (parentOf(from) === toDir) return false;
    // Ein Ordner darf nicht in sich selbst oder einen seiner Unterordner wandern.
    return toDir !== from && !toDir.startsWith(`${from}/`);
  }

  let dragging: string | null = null;

  function dragStart(event: DragEvent, entry: WorkspaceEntry) {
    dragging = entry.relative_path;
    event.dataTransfer?.setData("text/plain", entry.relative_path);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  function dragOver(event: DragEvent, toDir: string) {
    if (!canDrop(dragging, toDir)) return;
    event.preventDefault();
    event.stopPropagation();
    dropTarget = toDir;
  }

  async function drop(event: DragEvent, toDir: string) {
    event.preventDefault();
    event.stopPropagation();
    const from = dragging;
    dragging = null;
    dropTarget = null;
    if (from && canDrop(from, toDir)) await onMove(from, toDir);
  }

  // --- Tastatur ---------------------------------------------------------------

  function visibleButtons(): HTMLButtonElement[] {
    return root ? [...root.querySelectorAll<HTMLButtonElement>("button.v-tree-open")] : [];
  }

  function keydown(event: KeyboardEvent, entry: WorkspaceEntry) {
    const buttons = visibleButtons();
    const index = buttons.indexOf(event.currentTarget as HTMLButtonElement);
    if (event.key === "ArrowDown") { buttons[index + 1]?.focus(); event.preventDefault(); }
    else if (event.key === "ArrowUp") { buttons[index - 1]?.focus(); event.preventDefault(); }
    else if (event.key === "ArrowRight" && entry.is_directory && !expanded[entry.relative_path]) { onToggle(entry); event.preventDefault(); }
    else if (event.key === "ArrowLeft") {
      if (entry.is_directory && expanded[entry.relative_path]) onToggle(entry);
      else buttons.find((button) => button.dataset.path === parentOf(entry.relative_path))?.focus();
      event.preventDefault();
    } else if (event.key === "F2") { renaming = { path: entry.relative_path, value: entry.name }; event.preventDefault(); }
    else if (event.key === "Delete" && canDelete) { deleting = entry.relative_path; event.preventDefault(); }
  }
</script>

{#snippet branch(dir: string, depth: number)}
  {#each children[dir] ?? [] as entry (entry.relative_path)}
    {@const open = entry.is_directory && !!expanded[entry.relative_path]}
    {@const mark = markFor(entry)}
    <li role="none" class:open={openPaths.includes(entry.relative_path)}>
      {#if renaming?.path === entry.relative_path}
        <form class="v-tree-inline" style:padding-left="{depth * 14 + 8}px" onsubmit={submitRename}>
          <input use:focusOnMount bind:value={renaming.value} aria-label={t("Neuer Name")} onkeydown={(event) => event.key === "Escape" && (renaming = null)} />
          <button type="submit" class="v-btn v-btn-primary">{t("Umbenennen")}</button>
          <button type="button" class="v-btn v-btn-ghost" onclick={() => (renaming = null)}>{t("Abbrechen")}</button>
        </form>
      {:else if deleting === entry.relative_path}
        <div class="v-tree-confirm" role="alertdialog" aria-label={t("Entfernen bestätigen")}>
          <span>
            {#if hasUnsaved(entry.relative_path)}{t("„{name}“ hat ungespeicherte Änderungen. Trotzdem in den Papierkorb?", { name: entry.name })}{:else}{t("„{name}“ in den Papierkorb verschieben?", { name: entry.name })}{/if}
          </span>
          <span class="v-row">
            <button type="button" class="v-btn v-btn-primary" onclick={() => confirmDelete(entry.relative_path)}>{t("Ja, verschieben")}</button>
            <button type="button" class="v-btn v-btn-ghost" onclick={() => (deleting = null)}>{t("Behalten")}</button>
          </span>
        </div>
      {:else}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="v-tree-row"
          class:active={activePath === entry.relative_path}
          class:selected={entry.is_directory && selectedDir === entry.relative_path}
          class:drop={dropTarget === entry.relative_path}
          draggable="true"
          ondragstart={(event) => dragStart(event, entry)}
          ondragend={() => { dragging = null; dropTarget = null; }}
          ondragover={entry.is_directory ? (event) => dragOver(event, entry.relative_path) : undefined}
          ondragleave={() => { if (dropTarget === entry.relative_path) dropTarget = null; }}
          ondrop={entry.is_directory ? (event) => drop(event, entry.relative_path) : undefined}
        >
          <button
            type="button"
            class="v-tree-open"
            style:padding-left="{depth * 14 + 6}px"
            role="treeitem"
            aria-selected={activePath === entry.relative_path}
            aria-expanded={entry.is_directory ? open : undefined}
            aria-level={depth + 1}
            data-path={entry.relative_path}
            title={entry.relative_path}
            onclick={() => (entry.is_directory ? onToggle(entry) : onOpen(entry))}
            onkeydown={(event) => keydown(event, entry)}
          >
            <svg class="v-tree-chevron" class:turned={open} class:hidden={!entry.is_directory} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" aria-hidden="true" class:folder={entry.is_directory}><path d={entry.is_directory ? ICON_FOLDER : ICON_FILE} /></svg>
            <span class="v-tree-name">{entry.name}</span>
            {#if mark}<span class="v-tree-mark" class:folder={entry.is_directory} data-mark={mark} title={entry.is_directory ? t("Enthält Änderungen") : t("Geändert seit dem letzten Commit")}>{mark}</span>{/if}
          </button>
          <span class="v-tree-actions">
            <button type="button" class="v-btn-icon" aria-label={t("„{name}“ umbenennen", { name: entry.name })} onclick={() => (renaming = { path: entry.relative_path, value: entry.name })}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16v4Z" /></svg>
            </button>
            {#if canDelete}
              <button type="button" class="v-btn-icon" aria-label={t("„{name}“ entfernen", { name: entry.name })} onclick={() => (deleting = entry.relative_path)}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3m-9 0 1 13h10l1-13" /></svg>
              </button>
            {/if}
          </span>
        </div>
        {#if open}
          <ul role="group" class="v-tree-list">
            {@render branch(entry.relative_path, depth + 1)}
            {#if (children[entry.relative_path] ?? []).length === 0}<li class="v-help v-tree-empty" style:padding-left="{(depth + 1) * 14 + 8}px">{t("Ordner ist leer.")}</li>{/if}
          </ul>
        {/if}
      {/if}
    </li>
  {/each}
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role, a11y_no_static_element_interactions -->
<ul
  bind:this={root}
  role="tree"
  aria-label={t("Dateien")}
  class="v-tree-list v-tree-root"
  class:drop={dropTarget === ""}
  ondragover={(event) => dragOver(event, "")}
  ondragleave={() => { if (dropTarget === "") dropTarget = null; }}
  ondrop={(event) => drop(event, "")}
>
  {@render branch("", 0)}
  {#if (children[""] ?? []).length === 0}<li class="v-help v-tree-empty">{t("Ordner ist leer.")}</li>{/if}
</ul>

<style>
  .v-tree-list { margin: 0; padding: 0; list-style: none; display: grid; gap: 1px; }
  .v-tree-root.drop { outline: 2px dashed var(--v-accent-blue); outline-offset: 2px; border-radius: var(--v-radius-control); }
  .v-tree-empty { padding: var(--v-space-2); }
  .v-tree-row { display: flex; align-items: center; border-radius: var(--v-radius-control); }
  .v-tree-row:hover, .v-tree-row:focus-within { background: rgb(var(--v-tint) / .06); }
  .v-tree-row.active { background: rgb(var(--v-tint) / .12); }
  .v-tree-row.selected .v-tree-name { font-weight: 600; color: var(--v-text-primary); }
  .v-tree-row.drop { background: var(--v-accent-blue-soft); outline: 1px dashed var(--v-accent-blue); }
  .v-tree-open { display: flex; align-items: center; gap: 6px; flex: 1; min-width: 0; min-height: 2rem; padding-right: var(--v-space-2); border: 0; background: transparent; color: var(--v-text-secondary); font-size: var(--v-text-sm); text-align: left; }
  .v-tree-row.active .v-tree-open, li.open > .v-tree-row .v-tree-open { color: var(--v-text-primary); }
  .v-tree-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .v-tree-open svg { flex: 0 0 auto; color: var(--v-text-muted); }
  .v-tree-open svg.folder { color: var(--v-accent-blue); }
  .v-tree-chevron { transition: transform 120ms ease-out; }
  .v-tree-chevron.turned { transform: rotate(90deg); }
  .v-tree-chevron.hidden { visibility: hidden; }
  .v-tree-mark { flex: 0 0 auto; min-width: 1rem; text-align: center; font-family: var(--v-font-mono); font-size: var(--v-text-xs); color: var(--v-warning); }
  .v-tree-mark[data-mark="A"], .v-tree-mark[data-mark="??"] { color: var(--v-success); }
  .v-tree-mark[data-mark="D"] { color: var(--v-danger); }
  .v-tree-mark.folder { color: var(--v-warning); }
  /* Aktionen erscheinen bei Hover und Fokus, bleiben aber mit der Tastatur erreichbar. */
  .v-tree-actions { display: flex; opacity: 0; transition: opacity 120ms ease-out; }
  .v-tree-row:hover .v-tree-actions, .v-tree-row:focus-within .v-tree-actions { opacity: 1; }
  @media (hover: none) { .v-tree-actions { opacity: 1; } }
  .v-tree-inline { display: flex; flex-wrap: wrap; gap: var(--v-space-2); align-items: center; }
  .v-tree-inline input { flex: 1 1 8rem; min-width: 0; }
  .v-tree-confirm { display: grid; gap: var(--v-space-2); padding: var(--v-space-2); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-control); font-size: var(--v-text-sm); color: var(--v-text-secondary); }
  @media (prefers-reduced-motion: reduce) { .v-tree-chevron, .v-tree-actions { transition: none; } }
</style>
