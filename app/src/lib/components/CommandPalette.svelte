<script lang="ts">
  import { t } from "../i18n/index.svelte";
  import { onMount } from "svelte";
  import { listActiveFacts, listConversations } from "../ipc";

  export interface PaletteCommand {
    id: string;
    label: string;
    group: string;
    hint?: string;
    icon: string;
    keywords?: string;
    run: () => void;
  }

  interface Props {
    isOpen: boolean;
    commands: PaletteCommand[];
    onClose: () => void;
    onOpenConversation: (id: string) => void;
    onOpenFacts: () => void;
  }

  let { isOpen = $bindable(false), commands, onClose, onOpenConversation, onOpenFacts }: Props = $props();
  let query = $state("");
  let selectedIndex = $state(0);
  let inputEl = $state<HTMLInputElement | null>(null);
  let dynamic = $state<PaletteCommand[]>([]);

  const CHAT_ICON = "M4 5.5A2.5 2.5 0 0 1 6.5 3h11A2.5 2.5 0 0 1 20 5.5v9A2.5 2.5 0 0 1 17.5 17H9l-5 4v-4.5a2.5 2.5 0 0 1 0-1V5.5Z";
  const FACT_ICON = "M8 4a4 4 0 0 0-4 4v8a4 4 0 0 0 4 4h8a4 4 0 0 0 4-4V8a4 4 0 0 0-4-4H8Zm2 4h4v2h-4V8Zm-2 4h8v2H8v-2Z";

  function normalize(text: string) {
    return text.toLocaleLowerCase("de-AT").normalize("NFD").replace(/\p{Diacritic}/gu, "");
  }

  // Unterhaltungen und Fakten werden nur bei geöffneter Suche geladen und nur
  // angezeigt, sobald getippt wird; ohne Suchbegriff bleibt die Liste kurz.
  async function loadDynamic() {
    const next: PaletteCommand[] = [];
    try {
      for (const conversation of await listConversations()) {
        next.push({ id: `conv-${conversation.id}`, label: conversation.title, group: t("Unterhaltung"), icon: CHAT_ICON, run: () => onOpenConversation(conversation.id) });
      }
    } catch { /* Tresor gesperrt oder Kern nicht erreichbar: nur statische Befehle */ }
    try {
      for (const fact of await listActiveFacts()) {
        next.push({ id: `fact-${fact.id}`, label: fact.text, group: t("Fakt"), icon: FACT_ICON, run: onOpenFacts });
      }
    } catch { /* siehe oben */ }
    dynamic = next;
  }

  // Zuletzt gewählte Befehle stehen ohne Suchbegriff oben. Nur die Kennungen liegen im Browserspeicher
  // dieses Fensters (kein Tresor, keine Inhalte); ist er gesperrt oder leer, gibt es keine Liste.
  const RECENT_KEY = "iap.palette.recent";
  const RECENT_MAX = 4;
  function readRecent(): string[] {
    try {
      const parsed = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
      return Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === "string").slice(0, RECENT_MAX) : [];
    } catch { return []; }
  }
  function rememberRecent(id: string) {
    try {
      const next = [id, ...readRecent().filter((entry) => entry !== id)].slice(0, RECENT_MAX);
      localStorage.setItem(RECENT_KEY, JSON.stringify(next));
    } catch { /* Speicher gesperrt: dann eben ohne Merkliste */ }
  }
  let recentIds = $state<string[]>([]);

  const filtered = $derived.by(() => {
    // Jedes eingegebene Wort muss vorkommen, in beliebiger Reihenfolge („termin neu“ findet „Neuer Termin“).
    const words = normalize(query.trim()).split(/\s+/).filter(Boolean);
    if (words.length === 0) {
      const recent = recentIds
        .map((id) => commands.find((item) => item.id === id))
        .filter((item): item is PaletteCommand => item !== undefined)
        .map((item) => ({ ...item, id: `recent-${item.id}`, group: t("Zuletzt benutzt") }));
      return [...recent, ...commands];
    }
    return [...commands, ...dynamic]
      .filter((item) => {
        const haystack = normalize(`${item.label} ${item.group} ${item.hint ?? ""} ${item.keywords ?? ""}`);
        return words.every((word) => haystack.includes(word));
      })
      .slice(0, 40);
  });

  $effect(() => {
    if (isOpen) {
      query = "";
      selectedIndex = 0;
      recentIds = readRecent();
      void loadDynamic();
      queueMicrotask(() => inputEl?.focus());
    }
  });

  $effect(() => { void query; selectedIndex = 0; });

  function choose(item: PaletteCommand) {
    // Nur feste Befehle merken, keine einzelnen Unterhaltungen oder Fakten (die haben private Titel).
    if (!item.id.startsWith("conv-") && !item.id.startsWith("fact-")) rememberRecent(item.id.replace(/^recent-/, ""));
    onClose();
    item.run();
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (!isOpen) return;
    if (event.key === "Escape") { event.preventDefault(); onClose(); }
    else if (event.key === "ArrowDown") { event.preventDefault(); selectedIndex = filtered.length ? (selectedIndex + 1) % filtered.length : 0; scrollSelected(); }
    else if (event.key === "ArrowUp") { event.preventDefault(); selectedIndex = filtered.length ? (selectedIndex - 1 + filtered.length) % filtered.length : 0; scrollSelected(); }
    else if (event.key === "Enter" && filtered[selectedIndex]) { event.preventDefault(); choose(filtered[selectedIndex]); }
  }

  function scrollSelected() {
    queueMicrotask(() => document.querySelector(".v-palette-results [aria-selected='true']")?.scrollIntoView({ block: "nearest" }));
  }

  onMount(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  });
</script>

{#if isOpen}
  <div class="v-palette-layer">
    <button type="button" class="v-palette-dismiss" onclick={onClose} aria-label={t("Suche schließen")}></button>
    <div class="v-palette" role="dialog" aria-modal="true" aria-label={t("Suchen und Befehle")}>
      <div class="v-palette-input">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></svg>
        <input bind:this={inputEl} bind:value={query} aria-label={t("Suchen")} placeholder={t("Bereich, Aktion, Unterhaltung oder Fakt suchen")} />
        <kbd>{t("Esc")}</kbd>
      </div>
      <div class="v-palette-results" role="listbox" aria-label={t("Ergebnisse")}>
        {#each filtered as item, index (item.id)}
          {#if index === 0 || filtered[index - 1].group !== item.group}<div class="v-palette-group">{item.group}</div>{/if}
          <button type="button" role="option" aria-selected={index === selectedIndex} class:selected={index === selectedIndex} onmouseenter={() => (selectedIndex = index)} onclick={() => choose(item)}>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={item.icon}/></svg>
            <span class="v-palette-label">{item.label}</span>
            {#if item.hint}<small>{item.hint}</small>{/if}
          </button>
        {:else}
          <p>{t("Nichts gefunden.")}</p>
        {/each}
      </div>
      <footer><span><kbd>↑</kbd><kbd>↓</kbd> {t("wählen")}</span><span><kbd>{t("Enter")}</kbd> {t("öffnen")}</span><span><kbd>{t("Strg")}</kbd><kbd>{t("K")}</kbd> {t("jederzeit")}</span></footer>
    </div>
  </div>
{/if}

<style>
  .v-palette-layer { position: fixed; inset: 0; z-index: 80; display: flex; justify-content: center; align-items: flex-start; padding: min(14vh, 7rem) 1rem 1rem; }
  .v-palette-dismiss { position: absolute; inset: 0; border: 0; border-radius: 0; background: rgb(var(--v-shade) / .5); cursor: default; }
  .v-palette { position: relative; width: min(38rem, 100%); max-height: min(36rem, 76vh); display: flex; flex-direction: column; overflow: hidden; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: 0 25px 70px rgb(var(--v-shade) / .42); }
  .v-palette-input { display: flex; align-items: center; gap: var(--v-space-3); min-height: 3.5rem; padding: var(--v-space-2) var(--v-space-4); border-bottom: 1px solid var(--v-line); color: var(--v-text-muted); }
  .v-palette-input input { flex: 1; min-width: 0; padding: .2rem 0; border: 0; background: transparent; box-shadow: none; font-size: 1rem; }
  .v-palette-input input:focus { border: 0; box-shadow: none; }
  kbd { display: inline-block; min-width: 1.3rem; margin-right: 2px; padding: 1px 5px; border: 1px solid var(--v-line-strong); border-radius: 4px; color: var(--v-text-muted); font-family: inherit; font-size: var(--v-text-xs); text-align: center; }
  .v-palette-results { overflow-y: auto; padding: var(--v-space-2); }
  .v-palette-group { padding: var(--v-space-3) var(--v-space-3) var(--v-space-1); color: var(--v-text-muted); font-size: var(--v-text-xs); font-weight: 600; }
  .v-palette-results button { width: 100%; min-height: 2.75rem; display: flex; align-items: center; gap: var(--v-space-3); padding: var(--v-space-2) var(--v-space-3); border: 0; border-radius: var(--v-radius-control); background: transparent; color: var(--v-text-secondary); text-align: left; font-size: var(--v-text-md); }
  .v-palette-results button svg { flex: 0 0 auto; color: var(--v-text-muted); }
  .v-palette-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .v-palette-results button small { flex: 0 1 auto; overflow: hidden; color: var(--v-text-muted); font-size: var(--v-text-xs); text-overflow: ellipsis; white-space: nowrap; max-width: 45%; }
  .v-palette-results button.selected { background: rgb(var(--v-tint) / .09); color: var(--v-text-primary); }
  .v-palette-results button.selected svg { color: var(--v-accent-blue); }
  .v-palette-results p { margin: var(--v-space-4); color: var(--v-text-muted); font-size: var(--v-text-sm); }
  footer { display: flex; gap: var(--v-space-4); padding: var(--v-space-2) var(--v-space-4); border-top: 1px solid var(--v-line); color: var(--v-text-muted); font-size: var(--v-text-xs); }
  @media (max-width: 520px) { footer span:last-child { display: none; } }
</style>
