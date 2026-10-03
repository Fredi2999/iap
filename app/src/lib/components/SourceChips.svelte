<script lang="ts">
  // Quellen einer Antwort. Ein Klick zeigt die zitierte Stelle, damit man
  // die Aussage prüfen kann, ohne das Dokument zu öffnen.
  import { t } from "../i18n/index.svelte";
  import type { MessageSource } from "../types";

  interface Props { sources: MessageSource[]; }
  let { sources }: Props = $props();
  let openNumber = $state<number | null>(null);
  let open = $derived(sources.find((source) => source.number === openNumber) ?? null);
</script>

<div class="v-sources" aria-label={t("Quellen")}>
  <span class="v-sources-label">{t("Quellen")}</span>
  {#each sources as source (source.number)}
    <button type="button" class="v-source" class:active={openNumber === source.number} aria-expanded={openNumber === source.number}
      onclick={() => (openNumber = openNumber === source.number ? null : source.number)}>
      <strong>[{source.number}]</strong> {source.document_name} · {t("Abschnitt {n}", { n: source.chunk_ordinal + 1 })}
    </button>
  {/each}
  {#if open}
    <blockquote class="v-source-excerpt">{open.excerpt}{open.excerpt.length >= 280 ? " …" : ""}</blockquote>
  {/if}
</div>

<style>
  .v-sources { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: var(--v-space-3); }
  .v-sources-label { color: var(--v-text-muted); font-size: var(--v-text-xs); font-weight: 600; margin-right: 2px; }
  .v-source { max-width: 18rem; overflow: hidden; padding: 3px 9px; border: 1px solid var(--v-line); border-radius: 999px; background: rgb(var(--v-tint) / .04); color: var(--v-text-secondary); font-size: var(--v-text-xs); text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
  .v-source strong { color: var(--v-accent-blue); font-weight: 600; }
  .v-source:hover, .v-source.active { border-color: var(--v-line-strong); color: var(--v-text-primary); }
  .v-source-excerpt { flex-basis: 100%; margin: var(--v-space-1) 0 0; padding: var(--v-space-2) var(--v-space-3); border-left: 2px solid var(--v-accent-blue); background: rgb(var(--v-tint) / .04); color: var(--v-text-secondary); font-size: var(--v-text-sm); line-height: 1.5; white-space: pre-wrap; }
</style>
