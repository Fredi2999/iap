<script lang="ts">
  import { t, locale } from "../lib/i18n/index.svelte";
  import { onMount } from "svelte";
  import { exportMemory, forgetFact, learnCancel, learnFromConversation, listActiveFacts, listConversations, memoryGraph, retrieveMemory, updateFact, upsertFact } from "../lib/ipc";
  import type { Conversation, Fact, FactCategory, GraphNode, MemoryGraph as Graph, MemoryHit } from "../lib/types";
  import MemoryGraphView from "../lib/components/MemoryGraph.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import { FACT_CATEGORIES, FACT_CATEGORY_LABELS } from "../lib/labels";
  import { notify } from "../lib/notifications";

  let facts = $state<Fact[]>([]);
  let filter = $state("");
  let category = $state<FactCategory | "all">("all");
  let query = $state("");
  let hits = $state<MemoryHit[] | null>(null);
  let newFactText = $state("");
  let newFactCategory = $state<FactCategory>("preference");
  let newFactVerified = $state(true);
  let error = $state<unknown>(null);
  let loaded = $state(false);

  // Ansicht: Liste oder Graph. Der Graph wird erst beim ersten Öffnen geladen.
  let view = $state<"list" | "graph">("list");
  let graph = $state<Graph>({ nodes: [], edges: [] });
  let showOld = $state(false);
  let selected = $state<GraphNode | null>(null);
  let editText = $state("");
  let editCategory = $state<FactCategory>("other");
  let editVerified = $state(true);
  let conversations = $state<Conversation[]>([]);
  let learnFrom = $state("");
  let learning = $state(false);
  let allFacts = $derived(new Map(facts.map((fact) => [fact.id, fact] as const)));

  onMount(() => { void reload(); });

  async function reload() {
    try {
      facts = await listActiveFacts();
      if (view === "graph") graph = await memoryGraph(showOld);
      error = null;
    }
    catch (reason) { error = reason; }
    finally { loaded = true; }
  }

  async function openView(next: "list" | "graph") {
    view = next;
    if (next === "graph") {
      try {
        graph = await memoryGraph(showOld);
        if (conversations.length === 0) {
          conversations = await listConversations();
          learnFrom = conversations[0]?.id ?? "";
        }
      } catch (reason) { error = reason; }
    }
  }

  function select(node: GraphNode | null) {
    selected = node;
    const fact = node?.kind === "fact" ? allFacts.get(node.id.slice("fact:".length)) : undefined;
    if (fact) { editText = fact.text; editCategory = fact.category; editVerified = fact.user_verified; }
  }

  let selectedFact = $derived(selected?.kind === "fact" ? allFacts.get(selected.id.slice("fact:".length)) ?? null : null);

  async function saveEdit(event: SubmitEvent) {
    event.preventDefault();
    if (!selectedFact || !editText.trim()) return;
    error = null;
    try {
      await updateFact({ id: selectedFact.id, text: editText.trim(), category: editCategory, user_verified: editVerified });
      notify("Fakt gespeichert", "IAP berücksichtigt ihn ab jetzt.", "success");
      selected = null;
      await reload();
    } catch (reason) { error = reason; }
  }

  async function createFromGhost() {
    if (!selected || selected.kind !== "ghost") return;
    error = null;
    try {
      await upsertFact({ id: null, text: selected.label, category: "other", user_verified: true });
      selected = null;
      await reload();
    } catch (reason) { error = reason; }
  }

  async function learn() {
    if (!learnFrom || learning) return;
    learning = true; error = null;
    try {
      const learned = await learnFromConversation(learnFrom);
      notify(t("Gelernt"), t("{n} neue Fakten aus der Unterhaltung.", { n: learned.length }), "success");
      await reload();
    } catch (reason) { error = reason; }
    finally { learning = false; }
  }

  async function search(event: SubmitEvent) {
    event.preventDefault();
    error = null;
    if (!query.trim()) { hits = null; return; }
    try { hits = (await retrieveMemory(query)).hits; }
    catch (reason) { error = reason; }
  }

  async function saveFact(event: SubmitEvent) {
    event.preventDefault();
    if (!newFactText.trim()) return;
    error = null;
    try {
      await upsertFact({ id: null, text: newFactText.trim(), category: newFactCategory, user_verified: newFactVerified });
      newFactText = "";
      notify("Fakt gespeichert", "IAP berücksichtigt ihn ab jetzt.", "success");
      await reload();
    } catch (reason) { error = reason; }
  }

  async function remove(fact: Fact) {
    if (!confirm(`${t("Diesen Fakt vergessen?")}\n\n${fact.text}`)) return;
    error = null;
    try { await forgetFact(fact.id); selected = null; await reload(); }
    catch (reason) { error = reason; }
  }

  async function download() {
    error = null;
    try {
      const dump = await exportMemory();
      const url = URL.createObjectURL(new Blob([JSON.stringify(dump, null, 2)], { type: "application/json" }));
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = `iap-gedaechtnis-${new Date(dump.exported_at_unix_ms).toISOString().slice(0, 10)}.json`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (reason) { error = reason; }
  }

  let visible = $derived(facts.filter((fact) =>
    (category === "all" || fact.category === category)
    && (!filter.trim() || fact.text.toLocaleLowerCase("de-AT").includes(filter.trim().toLocaleLowerCase("de-AT")))));
</script>

<div class="v-page">
  <PageHeader title={t("Fakten")} description="Was sich IAP gemerkt hat. Prüfen, ergänzen oder löschen.">
    {#snippet actions()}<button class="v-btn v-btn-ghost" onclick={download} disabled={facts.length === 0}>{t("Exportieren")}</button>{/snippet}
    {#snippet help()}{t("IAP lernt wichtige Fakten aus Unterhaltungen und zieht sie bei passenden Fragen heran. „Bestätigt“ heißt, du hast den Fakt selbst angelegt oder geprüft.")}{/snippet}
  </PageHeader>

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  <div class="v-row v-view-switch" role="group" aria-label={t("Ansicht")}>
    <button class="v-btn" class:v-btn-primary={view === "list"} class:v-btn-ghost={view !== "list"} aria-pressed={view === "list"} onclick={() => openView("list")}>{t("Liste")}</button>
    <button class="v-btn" class:v-btn-primary={view === "graph"} class:v-btn-ghost={view !== "graph"} aria-pressed={view === "graph"} onclick={() => openView("graph")}>{t("Graph")}</button>
  </div>

  <div class="v-grid-aside">
    {#if view === "graph"}
    <section class="v-card v-stack" aria-label={t("Gedächtnis-Graph")}>
      <div class="v-fact-filter">
        <input type="search" placeholder={t("Im Graph suchen")} bind:value={filter} aria-label={t("Im Graph suchen")} />
        <label class="v-row v-help"><input type="checkbox" bind:checked={showOld} onchange={reload} /> {t("Abgelöste Fakten zeigen")}</label>
      </div>
      {#if graph.nodes.length === 0}
        <EmptyState title={t("Noch keine Fakten")} text="Sag im Chat zum Beispiel „Merk dir, dass ich Lehrerin bin“ oder lege rechts einen Fakt an." icon="M8 4a4 4 0 0 0-4 4v8a4 4 0 0 0 4 4h8a4 4 0 0 0 4-4V8a4 4 0 0 0-4-4H8Z" />
      {:else}
        <MemoryGraphView {graph} query={filter} selectedId={selected?.id ?? null} onselect={select} />
        <p data-hint class="v-help">{t("Verknüpfe mit [[Titel]] (erste Zeile eines Fakts).")}</p>
      {/if}
    </section>
    {:else}
    <section class="v-card v-stack" aria-label={t("Gespeicherte Fakten")}>
      <div class="v-fact-filter">
        <input type="search" placeholder={t("Fakten filtern")} bind:value={filter} aria-label={t("Fakten filtern")} />
        <select bind:value={category} aria-label={t("Kategorie")}>
          <option value="all">{t("Alle Kategorien")}</option>
          {#each FACT_CATEGORIES as c (c)}<option value={c}>{t(FACT_CATEGORY_LABELS[c])}</option>{/each}
        </select>
      </div>
      {#if loaded && facts.length === 0}
        <EmptyState title={t("Noch keine Fakten")} text="Sag im Chat zum Beispiel „Merk dir, dass ich Lehrerin bin“ oder lege rechts einen Fakt an." icon="M8 4a4 4 0 0 0-4 4v8a4 4 0 0 0 4 4h8a4 4 0 0 0 4-4V8a4 4 0 0 0-4-4H8Z" />
      {:else if visible.length === 0}
        <p class="v-card-text">{t("Keine Fakten passen zum Filter.")}</p>
      {:else}
        <ul class="v-list">
          {#each visible as fact (fact.id)}
            <li class="v-fact">
              <div class="v-list-main">
                <p class="v-fact-text">{fact.text}</p>
                <span class="v-row">
                  <span class="v-chip">{t(FACT_CATEGORY_LABELS[fact.category])}</span>
                  {#if fact.user_verified}<span class="v-chip accent">{t("Bestätigt")}</span>{:else}<span class="v-help">{t("Automatisch gelernt, {n} % sicher", { n: Math.round(fact.confidence * 100) })}</span>{/if}
                </span>
              </div>
              <button class="v-btn-icon" title={t("Vergessen")} aria-label={"Vergessen: " + fact.text} onclick={() => remove(fact)}>
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3m-9 0 1 13h10l1-13"/></svg>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
    {/if}

    <div class="v-stack">
      {#if view === "graph"}
        {#if selectedFact}
          <form class="v-card v-stack" onsubmit={saveEdit}>
            <h2 class="v-card-title">{t("Fakt bearbeiten")}</h2>
            <label class="v-field"><span class="v-label">{t("Text")}</span><textarea rows="5" bind:value={editText}></textarea></label>
            <label class="v-field"><span class="v-label">{t("Kategorie")}</span>
              <select bind:value={editCategory}>{#each FACT_CATEGORIES as c (c)}<option value={c}>{t(FACT_CATEGORY_LABELS[c])}</option>{/each}</select>
            </label>
            <label class="v-row v-help"><input type="checkbox" bind:checked={editVerified} /> {t("Als bestätigt markieren")}</label>
            <div class="v-row">
              <button type="submit" class="v-btn v-btn-primary" disabled={!editText.trim()}>{t("Speichern")}</button>
              <button type="button" class="v-btn v-btn-ghost" onclick={() => remove(selectedFact)}>{t("Vergessen")}</button>
            </div>
          </form>
        {:else if selected?.kind === "ghost"}
          <div class="v-card v-stack">
            <h2 class="v-card-title">{selected.label}</h2>
            <p class="v-card-text">{t("Ein Link verweist hierher, aber der Fakt fehlt noch.")}</p>
            <button class="v-btn v-btn-primary" onclick={createFromGhost}>{t("Fakt anlegen")}</button>
          </div>
        {:else if selected?.kind === "category"}
          <div class="v-card v-stack"><h2 class="v-card-title">{selected.label}</h2><p class="v-card-text">{t("Sammelknoten dieser Kategorie.")}</p></div>
        {:else}
          <div class="v-card v-stack"><p data-hint class="v-card-text">{t("Wähle einen Knoten im Graph, um den Fakt zu bearbeiten.")}</p></div>
        {/if}
        <div class="v-card v-stack">
          <h2 class="v-card-title">{t("Aus Unterhaltung lernen")}</h2>
          <p data-hint class="v-card-text">{t("Das lokale Modell schlägt Fakten aus den letzten Nachrichten vor. Nur auf deinen Wunsch.")}</p>
          <select bind:value={learnFrom} aria-label={t("Unterhaltung")} disabled={conversations.length === 0}>
            {#each conversations as c (c.id)}<option value={c.id}>{c.title}</option>{/each}
          </select>
          <div class="v-row">
            <button class="v-btn v-btn-ghost" onclick={learn} disabled={!learnFrom || learning}>{learning ? t("Lernt …") : t("Jetzt lernen")}</button>
            {#if learning}<button class="v-btn v-btn-ghost" onclick={() => learnCancel()}>{t("Abbrechen")}</button>{/if}
          </div>
        </div>
      {/if}
      <form class="v-card v-stack" onsubmit={saveFact}>
        <h2 class="v-card-title">{t("Fakt hinzufügen")}</h2>
        <label class="v-field"><span class="v-label">{t("Was soll IAP wissen?")}</span><textarea rows="3" bind:value={newFactText} placeholder={t("Zum Beispiel: Ich unterrichte die 3B in Mathematik.")}></textarea></label>
        <label class="v-field"><span class="v-label">{t("Kategorie")}</span>
          <select bind:value={newFactCategory}>{#each FACT_CATEGORIES as c (c)}<option value={c}>{t(FACT_CATEGORY_LABELS[c])}</option>{/each}</select>
        </label>
        <label class="v-row v-help"><input type="checkbox" bind:checked={newFactVerified} /> {t("Als bestätigt markieren")}</label>
        <button type="submit" class="v-btn v-btn-primary" disabled={!newFactText.trim()}>{t("Speichern")}</button>
      </form>

      <form class="v-card v-stack" onsubmit={search}>
        <h2 class="v-card-title">{t("Wie findet IAP etwas?")}</h2>
        <p data-hint class="v-card-text">{t("Teste, welche Einträge IAP zu einer Frage heranziehen würde.")}</p>
        <input type="search" bind:value={query} placeholder={t("Frage oder Stichwort")} aria-label={t("Suchbegriff")} />
        <button type="submit" class="v-btn v-btn-ghost" disabled={!query.trim()}>{t("Suchen")}</button>
        {#if hits}
          {#if hits.length === 0}<p class="v-help">{t("Nichts Passendes gefunden.")}</p>
          {:else}
            <ul class="v-list">
              {#each hits as hit (hit.item_id)}
                <li><div class="v-list-main"><p class="v-fact-text">{hit.preview}</p><span>{t(hit.item_type === "fact" ? "Fakt" : "Dokumentabschnitt")} · {t("Trefferwert {n}", { n: hit.score.toFixed(2) })}</span></div></li>
              {/each}
            </ul>
          {/if}
        {/if}
      </form>
    </div>
  </div>
</div>

<style>
  .v-view-switch { margin-bottom: var(--v-space-3); }
  .v-fact-filter { display: grid; grid-template-columns: minmax(0, 1fr) 12rem; gap: var(--v-space-2); }
  @media (max-width: 640px) { .v-fact-filter { grid-template-columns: 1fr; } }
  .v-fact { align-items: flex-start !important; }
  .v-fact-text { margin: 0 0 6px; color: var(--v-text-primary); font-size: var(--v-text-sm); line-height: 1.5; overflow-wrap: anywhere; }
</style>
