<script lang="ts">
  import { t, locale } from "../lib/i18n/index.svelte";
  import { onDestroy, onMount } from "svelte";
  import {
    listConversations,
    openConversation,
    sendMessage,
    subscribeToStream,
    upsertFact,
  } from "../lib/ipc";
  import type { Conversation, StreamEvent } from "../lib/types";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import Markdown from "../lib/components/Markdown.svelte";

  const DIGEST_PROMPT = `Fasse die folgende Unterhaltung als Session Digest zusammen. Nutze exakt dieses Format:

**Worum ging es**
(1–2 Sätze, thematisch)

**Entscheidungen**
- (jede knapp, kein Zusatzblabla)

**Offene Fragen / Nächste Schritte**
- (klar handlungsorientiert)

Halte alles unter 200 Wörtern. Keine Höflichkeitsfloskeln.`;

  let conversations = $state<Conversation[]>([]);
  let selected = $state<string | null>(null);
  let digest = $state<string>("");
  let generating = $state(false);
  let error = $state<unknown>(null);
  let status = $state<string>("");
  let unlisten: (() => void) | null = null;

  onMount(async () => {
    try {
      conversations = await listConversations();
    } catch (e) { error = e; }
  });

  onDestroy(() => { if (unlisten) unlisten(); });

  async function generate() {
    if (!selected) return;
    generating = true;
    digest = "";
    error = null;
    status = "";
    try {
      const detail = await openConversation(selected);
      if (detail.messages.length === 0) {
        error = t("Diese Unterhaltung ist leer.");
        generating = false;
        return;
      }
      const transcript = detail.messages
        .filter((m) => m.role !== "system")
        .map((m) => `${m.role.toUpperCase()}: ${m.content}`)
        .join("\n\n");

      if (unlisten) unlisten();
      unlisten = await subscribeToStream((ev: StreamEvent) => {
        if (ev.kind === "delta") digest += ev.text;
        if (ev.kind === "finished") generating = false;
        if (ev.kind === "failed") { error = ev.message; generating = false; }
      });

      // Wir schicken den Digest-Prompt an eine ephemere Konversation.
      await sendMessage({
        conversation_id: null,
        content: `${DIGEST_PROMPT}\n\n---\n\n${transcript}`,
      });
    } catch (e) {
      error = e;
      generating = false;
    }
  }

  async function keep() {
    if (!digest.trim() || !selected) return;
    const title = conversations.find((c) => c.id === selected)?.title ?? "Digest";
    try {
      await upsertFact({
        id: null,
        text: `[Digest: ${title}] ${digest.trim()}`,
        category: "project",
        user_verified: true,
      });
      status = t("Zusammenfassung im Gedächtnis gespeichert.");
    } catch (e) { error = e; }
  }
</script>


<div class="v-page">
  <PageHeader title={t("Zusammenfassungen")} description="Verdichtet eine lange Unterhaltung auf das Wichtigste: Thema, Entscheidungen und nächste Schritte." />

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  {#if conversations.length === 0}
    <EmptyState title={t("Noch keine Unterhaltungen")} text="Sobald du mit IAP gechattet hast, kannst du Unterhaltungen hier zusammenfassen." icon="M4 6h16M4 12h12M4 18h8" />
  {:else}
    <div class="v-digest">
      <section class="v-card v-digest-list" aria-label={t("Unterhaltungen")}>
        <h2 class="v-card-title">{t("Unterhaltung wählen")}</h2>
        <ul>
          {#each conversations as c (c.id)}
            <li><button type="button" class:active={selected === c.id} onclick={() => { selected = c.id; digest = ""; status = ""; }}>{c.title}</button></li>
          {/each}
        </ul>
      </section>

      <section class="v-card v-stack v-digest-result">
        {#if !selected}
          <EmptyState title={t("Unterhaltung wählen")} text="Wähle links, welche Unterhaltung IAP zusammenfassen soll." />
        {:else}
          <div class="v-card-title">
            <span>{conversations.find((c) => c.id === selected)?.title}</span>
            <div class="v-row">
              {#if digest && !generating}<button class="v-btn v-btn-ghost" onclick={keep}>{t("Im Gedächtnis speichern")}</button>{/if}
              <button class="v-btn v-btn-primary" onclick={generate} disabled={generating}>{t(generating ? "Fasse zusammen …" : digest ? "Neu erstellen" : "Zusammenfassen")}</button>
            </div>
          </div>
          {#if digest}
            <div class="v-digest-text"><Markdown content={digest} isStreaming={generating} /></div>
            {#if status}<p class="v-help">{status}</p>{/if}
          {:else if generating}
            <p class="v-card-text">{t("IAP liest die Unterhaltung …")}</p>
          {:else}
            <p data-hint class="v-card-text">{t("Klicke auf „Zusammenfassen“. Das Ergebnis kannst du danach ins Gedächtnis übernehmen.")}</p>
          {/if}
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .v-digest { display: grid; grid-template-columns: minmax(14rem, 18rem) minmax(0, 1fr); gap: var(--v-space-4); align-items: start; }
  @media (max-width: 900px) { .v-digest { grid-template-columns: 1fr; } }
  .v-digest-list ul { margin: 0; padding: 0; list-style: none; max-height: 60vh; overflow-y: auto; }
  .v-digest-list button { width: 100%; min-height: 2.4rem; padding: 0 var(--v-space-3); overflow: hidden; border: 0; border-radius: var(--v-radius-control); background: transparent; color: var(--v-text-secondary); font-size: var(--v-text-sm); text-align: left; text-overflow: ellipsis; white-space: nowrap; }
  .v-digest-list button:hover { background: rgb(var(--v-tint) / .06); }
  .v-digest-list button.active { background: rgb(var(--v-tint) / .12); color: var(--v-text-primary); font-weight: 550; }
  .v-digest-result { min-height: 20rem; }
  .v-digest-text { color: var(--v-text-secondary); font-size: var(--v-text-md); line-height: 1.6; }
</style>
