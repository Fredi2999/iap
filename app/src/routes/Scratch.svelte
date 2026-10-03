<script lang="ts">
  import { t } from "../lib/i18n/index.svelte";
  import { upsertFact } from "../lib/ipc";
  import type { FactCategory } from "../lib/types";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import { FACT_CATEGORIES, FACT_CATEGORY_LABELS } from "../lib/labels";
  import { notify } from "../lib/notifications";

  // Bewusst flüchtig: Notizen leben nur im Arbeitsspeicher dieser Sitzung.
  // Dauerhaft wird nur, was ausdrücklich ins Gedächtnis übernommen wird.
  let content = $state<string>("");
  let category = $state<FactCategory>("preference");
  let saving = $state(false);
  let error = $state<unknown>(null);

  let words = $derived(content.split(/\s+/).filter(Boolean).length);

  async function saveToMemory() {
    if (!content.trim()) return;
    saving = true;
    error = null;
    try {
      await upsertFact({ id: null, text: content.trim(), category, user_verified: true });
      notify("Ins Gedächtnis übernommen", "IAP kennt diese Notiz ab jetzt.", "success");
      content = "";
    } catch (reason) {
      error = reason;
    } finally {
      saving = false;
    }
  }
</script>

<div class="v-page">
  <PageHeader title={t("Notizen")} description="Schnell etwas festhalten. Notizen verschwinden beim Schließen, außer du übernimmst sie ins Gedächtnis.">
    {#snippet actions()}<span class="v-chip warn">{t("Nur für diese Sitzung")}</span>{/snippet}
  </PageHeader>

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  <section class="v-card v-stack">
    <label class="v-field">
      <span class="sr-only">{t("Notiz")}</span>
      <textarea bind:value={content} rows="14" placeholder={t("Gedanken, Ideen, Stichpunkte …")} class="v-scratch-text"></textarea>
    </label>
    <div class="v-scratch-bar">
      <span class="v-help v-num">{t("{n} Zeichen", { n: content.length })} · {t("{n} Wörter", { n: words })}</span>
      <div class="v-row">
        <label class="v-row v-help">{t("Merken als")}
          <select bind:value={category} aria-label={t("Kategorie")}>
            {#each FACT_CATEGORIES as c (c)}<option value={c}>{t(FACT_CATEGORY_LABELS[c])}</option>{/each}
          </select>
        </label>
        <button class="v-btn v-btn-ghost" onclick={() => (content = "")} disabled={!content}>{t("Leeren")}</button>
        <button class="v-btn v-btn-primary" onclick={saveToMemory} disabled={!content.trim() || saving}>{t(saving ? "Übernehme …" : "Ins Gedächtnis übernehmen")}</button>
      </div>
    </div>
  </section>
</div>

<style>
  .v-scratch-text { min-height: 16rem; font-size: var(--v-text-md); line-height: 1.6; resize: vertical; }
  .v-scratch-bar { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--v-space-3); }
  .v-scratch-bar select { padding: .4rem .6rem; font-size: var(--v-text-sm); }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; }
</style>
