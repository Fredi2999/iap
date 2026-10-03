<script lang="ts">
  import { t, locale } from "../lib/i18n/index.svelte";
  import { onMount, untrack } from "svelte";
  import { listAudit } from "../lib/ipc";
  import type { AuditEntryView } from "../lib/types";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import { AUDIT_ACTION_LABELS, AUDIT_OUTCOME_LABELS } from "../lib/labels";

  let entries = $state<AuditEntryView[]>([]);
  let action = $state<string>("");
  let outcome = $state<string>("");
  let contains = $state<string>("");
  let error = $state<unknown>(null);
  let loaded = $state(false);

  onMount(() => { untrack(() => refresh()); });

  async function refresh(event?: SubmitEvent) {
    event?.preventDefault();
    error = null;
    try {
      entries = await listAudit({ actions: action ? [action] : [], outcomes: outcome ? [outcome] : [], contains: contains || null, since_unix_ms: null, limit: 500 });
    } catch (reason) {
      error = reason;
    } finally {
      loaded = true;
    }
  }

  function formatTime(unix_ms: number): string {
    return new Date(unix_ms).toLocaleString(locale(), { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }
</script>

<div class="v-page">
  <PageHeader title={t("Protokoll")} description="Jede Aktion, die IAP ausführt oder ablehnt, wird fälschungssicher festgehalten.">
    {#snippet help()}{t("Die Einträge sind über Prüfsummen miteinander verkettet. Wird ein Eintrag nachträglich verändert, fällt das auf.")}{/snippet}
  </PageHeader>

  {#if error}<ErrorNotice {error} onRetry={() => refresh()} onDismiss={() => (error = null)} />{/if}

  <form class="v-card v-log-filter" onsubmit={refresh}>
    <label class="v-field"><span class="v-label">{t("Aktion")}</span>
      <select bind:value={action}><option value="">{t("Alle")}</option>{#each Object.entries(AUDIT_ACTION_LABELS) as [value, label] (value)}<option {value}>{t(label)}</option>{/each}</select>
    </label>
    <label class="v-field"><span class="v-label">{t("Ergebnis")}</span>
      <select bind:value={outcome}><option value="">{t("Alle")}</option>{#each Object.entries(AUDIT_OUTCOME_LABELS) as [value, label] (value)}<option {value}>{t(label)}</option>{/each}</select>
    </label>
    <label class="v-field v-log-search"><span class="v-label">{t("Enthält")}</span><input type="text" placeholder={t("Text im Grund oder Ziel")} bind:value={contains} /></label>
    <button type="submit" class="v-btn v-btn-primary">{t("Filtern")}</button>
  </form>

  {#if loaded && entries.length === 0}
    <EmptyState title={t("Keine passenden Einträge")} text="Ändere die Filter oder arbeite mit IAP, dann erscheinen hier Einträge." icon="M5 4h14v16H5V4Zm2 3h10M7 11h10M7 15h7" />
  {:else if entries.length > 0}
    <div class="v-card v-log-table-wrap">
      <table class="v-log-table">
        <thead><tr><th>{t("Zeit")}</th><th>{t("Aktion")}</th><th>{t("Ziel")}</th><th>{t("Ergebnis")}</th><th>{t("Grund")}</th></tr></thead>
        <tbody>
          {#each entries as entry (entry.id)}
            <tr>
              <td class="v-num">{formatTime(entry.created_unix_ms)}</td>
              <td>{t(AUDIT_ACTION_LABELS[entry.action] ?? entry.action)}</td>
              <td class="v-num v-log-target" title={entry.target ?? ""}>{entry.target ?? ""}</td>
              <td><span class="v-chip" class:accent={entry.outcome === "allow"} class:danger={entry.outcome === "deny"} class:warn={entry.outcome === "prompt"}>{t(AUDIT_OUTCOME_LABELS[entry.outcome] ?? entry.outcome)}</span></td>
              <td>{entry.reason}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .v-log-filter { display: grid; grid-template-columns: 12rem 10rem minmax(0, 1fr) auto; gap: var(--v-space-3); align-items: end; }
  @media (max-width: 900px) { .v-log-filter { grid-template-columns: 1fr 1fr; } .v-log-search { grid-column: 1 / -1; } }
  .v-log-table-wrap { padding: 0; overflow-x: auto; }
  .v-log-table { width: 100%; border-collapse: collapse; font-size: var(--v-text-sm); }
  .v-log-table th { padding: var(--v-space-3) var(--v-space-4); border-bottom: 1px solid var(--v-line); color: var(--v-text-muted); font-size: var(--v-text-xs); font-weight: 600; text-align: left; white-space: nowrap; }
  .v-log-table td { padding: var(--v-space-2) var(--v-space-4); border-bottom: 1px solid var(--v-line); color: var(--v-text-secondary); vertical-align: top; }
  .v-log-table tr:last-child td { border-bottom: 0; }
  .v-log-table td.v-num { font-size: var(--v-text-xs); white-space: nowrap; }
  .v-log-target { max-width: 16rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
