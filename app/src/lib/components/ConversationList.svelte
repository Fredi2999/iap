<script lang="ts">
  import { t } from "../i18n/index.svelte";
  import type { Conversation, Project } from "../types";

  interface Props {
    conversations: Conversation[];
    projects: Project[];
    currentId: string | null;
    open: boolean;
    onSelect: (id: string) => void;
    onNew: () => void;
    onRename: (id: string, title: string) => void;
    onDelete: (id: string) => void;
    onClose: () => void;
    /** Öffnet den Projektdialog; `null` legt ein neues Projekt an. */
    onEditProject: (project: Project | null) => void;
    onMove: (conversationId: string, projectId: string | null) => void;
  }

  let { conversations, projects, currentId, open, onSelect, onNew, onRename, onDelete, onClose, onEditProject, onMove }: Props = $props();
  let renameTarget = $state<string | null>(null);
  let renameDraft = $state("");
  let moveTarget = $state<string | null>(null);
  let filter = $state("");

  let visible = $derived(filter.trim()
    ? conversations.filter((conversation) => conversation.title.toLocaleLowerCase().includes(filter.trim().toLocaleLowerCase()))
    : conversations);
  // Unterhaltungen je Projekt; ohne Projekt (oder mit gelöschtem Projekt) kommen nach unten.
  let projectIds = $derived(new Set(projects.map((project) => project.id)));
  let loose = $derived(visible.filter((conversation) => !conversation.project_id || !projectIds.has(conversation.project_id)));

  function commit() {
    if (!renameTarget) return;
    onRename(renameTarget, renameDraft.trim() || t("Ohne Titel"));
    renameTarget = null;
  }

  // Gruppierung nach Alter, damit lange Listen überschaubar bleiben.
  function bucket(updated: number) {
    const day = 86_400_000;
    const age = Date.now() - updated;
    if (age < day) return t("Heute");
    if (age < 7 * day) return t("Letzte 7 Tage");
    return t("Älter");
  }
</script>

{#snippet item(conversation: Conversation)}
  <li class="iap-conv-item" class:active={currentId === conversation.id}>
    {#if renameTarget === conversation.id}
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={renameDraft} aria-label={t("Neuer Titel")} autofocus onblur={commit}
        onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); commit(); } if (event.key === "Escape") renameTarget = null; }} />
    {:else if moveTarget === conversation.id}
      <!-- svelte-ignore a11y_autofocus -->
      <select aria-label={t("Projekt für {name}", { name: conversation.title })} autofocus value={conversation.project_id ?? ""}
        onchange={(event) => { onMove(conversation.id, (event.currentTarget as HTMLSelectElement).value || null); moveTarget = null; }}
        onblur={() => (moveTarget = null)} onkeydown={(event) => { if (event.key === "Escape") moveTarget = null; }}>
        <option value="">{t("Ohne Projekt")}</option>
        {#each projects as project (project.id)}<option value={project.id}>{project.name}</option>{/each}
      </select>
    {:else}
      <button type="button" class="iap-conv-title" aria-current={currentId === conversation.id ? "true" : undefined} onclick={() => onSelect(conversation.id)}>{conversation.title}</button>
      {#if projects.length > 0}
        <button type="button" class="v-btn-icon" title={t("In Projekt verschieben")} aria-label={t("In Projekt verschieben: {name}", { name: conversation.title })} onclick={() => (moveTarget = conversation.id)}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="M4 6a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6Z"/></svg>
        </button>
      {/if}
      <button type="button" class="v-btn-icon" title={t("Umbenennen")} aria-label={t("Umbenennen: {name}", { name: conversation.title })} onclick={() => { renameTarget = conversation.id; renameDraft = conversation.title; }}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="m15 4 5 5M4 20l4.5-1 11-11a2 2 0 0 0-3-3l-11 11L4 20Z"/></svg>
      </button>
      <button type="button" class="v-btn-icon" title={t("Löschen")} aria-label={t("Löschen: {name}", { name: conversation.title })} onclick={() => onDelete(conversation.id)}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3m-9 0 1 13h10l1-13"/></svg>
      </button>
    {/if}
  </li>
{/snippet}

{#if open}<button type="button" class="iap-conv-scrim" aria-label={t("Unterhaltungen schließen")} onclick={onClose}></button>{/if}
<aside class="iap-conv-panel" class:open aria-label={t("Unterhaltungen")}>
  <div class="iap-conv-head">
    <button type="button" class="v-btn v-btn-primary iap-conv-new" onclick={onNew}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>
      {t("Neue Unterhaltung")}
    </button>
    <button type="button" class="v-btn-icon iap-conv-close" aria-label={t("Unterhaltungen schließen")} onclick={onClose}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg>
    </button>
  </div>
  {#if conversations.length > 6}
    <input class="iap-conv-filter" bind:value={filter} placeholder={t("Unterhaltungen filtern")} aria-label={t("Unterhaltungen filtern")} />
  {/if}
  <ul>
    <li class="iap-conv-group iap-conv-projects-head">
      <span>{t("Projekte")}</span>
      <button type="button" class="v-btn-icon" title={t("Neues Projekt")} aria-label={t("Neues Projekt")} onclick={() => onEditProject(null)}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>
      </button>
    </li>
    {#each projects as project (project.id)}
      {@const members = visible.filter((conversation) => conversation.project_id === project.id)}
      <li class="iap-conv-project">
        <span class="iap-conv-project-name">{project.name}</span>
        <button type="button" class="v-btn-icon" title={t("Projekt bearbeiten")} aria-label={t("Projekt bearbeiten: {name}", { name: project.name })} onclick={() => onEditProject(project)}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="m15 4 5 5M4 20l4.5-1 11-11a2 2 0 0 0-3-3l-11 11L4 20Z"/></svg>
        </button>
      </li>
      {#each members as conversation (conversation.id)}{@render item(conversation)}{/each}
      {#if members.length === 0 && !filter.trim()}<li class="iap-conv-hint">{t("Verschiebe Unterhaltungen über das Ordnersymbol hierher.")}</li>{/if}
    {/each}
    {#if projects.length === 0}<li class="iap-conv-hint">{t("Projekte bündeln Unterhaltungen mit einer gemeinsamen Grundanweisung.")}</li>{/if}

    {#if conversations.length === 0}<li class="iap-conv-empty">{t("Deine Unterhaltungen erscheinen hier.")}</li>{/if}
    {#each loose as conversation, index (conversation.id)}
      {@const group = bucket(conversation.updated_at_unix_ms)}
      {#if index === 0 || bucket(loose[index - 1].updated_at_unix_ms) !== group}<li class="iap-conv-group">{group}</li>{/if}
      {@render item(conversation)}
    {/each}
  </ul>
</aside>

<style>
  .iap-conv-panel { flex: 0 0 16rem; min-height: 0; display: flex; flex-direction: column; border: 1px solid var(--v-line); border-radius: var(--v-radius-card); background: var(--v-surface-1); backdrop-filter: blur(15px); -webkit-backdrop-filter: blur(15px); overflow: hidden; }
  .iap-conv-head { display: flex; align-items: center; gap: var(--v-space-2); padding: var(--v-space-3); }
  .iap-conv-new { flex: 1 1 auto; justify-content: flex-start; }
  .iap-conv-close { display: none; }
  .iap-conv-filter { margin: 0 var(--v-space-3) var(--v-space-2); font-size: var(--v-text-sm); padding: .45rem .7rem; }
  ul { flex: 1 1 auto; min-height: 0; margin: 0; padding: 0 var(--v-space-2) var(--v-space-3); overflow-y: auto; list-style: none; }
  .iap-conv-group { padding: var(--v-space-3) var(--v-space-2) var(--v-space-1); color: var(--v-text-muted); font-size: var(--v-text-xs); font-weight: 600; }
  .iap-conv-item { display: flex; align-items: center; gap: 2px; min-height: 2.5rem; padding: 2px 4px 2px 2px; border-radius: var(--v-radius-control); }
  .iap-conv-item .v-btn-icon { opacity: 0; min-width: 1.75rem; min-height: 1.75rem; }
  .iap-conv-item:hover .v-btn-icon, .iap-conv-item:focus-within .v-btn-icon, .iap-conv-item.active .v-btn-icon { opacity: 1; }
  .iap-conv-item:hover { background: rgb(var(--v-tint) / .06); }
  .iap-conv-item.active { background: rgb(var(--v-tint) / .12); }
  .iap-conv-item input { width: 100%; padding: .35rem .55rem; font-size: var(--v-text-sm); }
  .iap-conv-title { flex: 1 1 auto; min-width: 0; min-height: 2.25rem; padding: 0 var(--v-space-2); overflow: hidden; border: 0; border-radius: var(--v-radius-control); background: transparent; color: var(--v-text-secondary); font-size: var(--v-text-sm); text-align: left; text-overflow: ellipsis; white-space: nowrap; }
  .iap-conv-item.active .iap-conv-title { color: var(--v-text-primary); font-weight: 550; }
  .iap-conv-title:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: -2px; }
  .iap-conv-empty { margin: var(--v-space-2) var(--v-space-2); color: var(--v-text-muted); font-size: var(--v-text-sm); }
  .iap-conv-projects-head { display: flex; align-items: center; justify-content: space-between; padding-right: 2px; }
  .iap-conv-project { display: flex; align-items: center; gap: 2px; min-height: 2rem; padding: var(--v-space-2) 4px 0 var(--v-space-2); color: var(--v-text-primary); font-size: var(--v-text-sm); font-weight: 600; }
  .iap-conv-project-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .iap-conv-project .v-btn-icon { opacity: 0; min-width: 1.75rem; min-height: 1.75rem; }
  .iap-conv-project:hover .v-btn-icon, .iap-conv-project:focus-within .v-btn-icon { opacity: 1; }
  .iap-conv-hint { padding: 2px var(--v-space-2) var(--v-space-2); color: var(--v-text-muted); font-size: var(--v-text-xs); line-height: 1.4; }
  .iap-conv-item select { width: 100%; padding: .35rem .55rem; font-size: var(--v-text-sm); }
  .iap-conv-scrim { display: none; }

  /* Unter 1100 px wird die Liste zur Schublade von links; die Promptbar bleibt frei. */
  @media (max-width: 1100px) {
    .iap-conv-panel { position: absolute; z-index: 30; top: 0; bottom: 0; left: 0; width: min(20rem, 88%); background: var(--v-surface-solid); box-shadow: 18px 0 50px rgb(var(--v-shade) / .3); transform: translateX(calc(-100% - 12px)); visibility: hidden; transition: transform 260ms var(--v-ease-drawer), visibility 0s linear 260ms; }
    .iap-conv-panel.open { transform: translateX(0); visibility: visible; transition: transform 260ms var(--v-ease-drawer), visibility 0s; }
    .iap-conv-close { display: inline-flex; }
    .iap-conv-scrim { display: block; position: absolute; inset: 0; z-index: 29; border: 0; border-radius: var(--v-radius-card); background: rgb(var(--v-shade) / .35); }
    .iap-conv-item .v-btn-icon, .iap-conv-project .v-btn-icon { opacity: 1; }
  }
  @media (prefers-reduced-motion: reduce) { .iap-conv-panel { transition: none !important; } }
</style>
