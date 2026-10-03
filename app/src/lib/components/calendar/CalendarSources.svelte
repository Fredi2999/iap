<script lang="ts">
  // Verbundene Kalender (Apple, Google): verbinden, aktualisieren, trennen. Optional – ohne Verbindung
  // arbeitet der Kalender wie bisher offline. IAP fragt nur auf Klick ab, nie im Hintergrund,
  // und bei eingeschaltetem Air Gap gar nicht.
  import { t, locale } from "../../i18n/index.svelte";
  import { calendarRemoveSource, calendarSync } from "../../ipc";
  import { calendarError, localOffsetMinutes } from "../../calendarItems";
  import type { CalendarOverview, CalendarSource } from "../../types";
  import CalendarConnectDialog from "./CalendarConnectDialog.svelte";

  interface Props {
    overview: CalendarOverview | null;
    airGap: boolean;
    airGapBusy: boolean;
    onToggleAirGap: () => void;
    /** Nach jeder Änderung: Quellen und Termine neu laden. */
    onChanged: () => Promise<void> | void;
  }
  let { overview, airGap, airGapBusy, onToggleAirGap, onChanged }: Props = $props();

  let connecting = $state(false);
  let syncing = $state<string | "all" | null>(null);
  let confirmRemove = $state<string | null>(null);
  let failure = $state<string | null>(null);
  let notice = $state<string | null>(null);

  const sources = $derived(overview?.sources ?? []);
  const supported = $derived(overview?.supported ?? true);

  function stamp(ms: number): string {
    return new Date(ms).toLocaleString(locale(), { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
  }

  async function refresh(sourceId: string | null) {
    syncing = sourceId ?? "all";
    failure = null;
    notice = null;
    try {
      const reports = await calendarSync(sourceId, localOffsetMinutes());
      const failed = reports.filter((report) => !report.ok);
      const total = reports.reduce((sum, report) => sum + report.count, 0);
      if (failed.length === 0) notice = t("{n} Termine aktualisiert.", { n: total });
      await onChanged();
    } catch (reason) {
      failure = calendarError(reason);
    } finally {
      syncing = null;
    }
  }

  async function remove(source: CalendarSource) {
    failure = null;
    try {
      await calendarRemoveSource(source.id);
      confirmRemove = null;
      await onChanged();
    } catch (reason) {
      failure = calendarError(reason);
    }
  }

  const kindLabel = (source: CalendarSource) => (source.kind === "icloud" ? "Apple" : "Google");
</script>

<section class="v-card v-stack" aria-label={t("Verbundene Kalender")}>
  <h2 class="v-card-title">{t("Verbundene Kalender")}</h2>
  <p class="v-card-text">{t("Optional. IAP verbindet sich nur auf deinen Klick.")}</p>
  <details class="v-more">
    <summary>{t("Details")}</summary>
    <p class="v-help">{t("Hole Termine aus deinem Apple- oder Google-Kalender und trage neue Termine in Apple Kalender ein. IAP verbindet sich nur, wenn du auf „Aktualisieren“ klickst oder einen Termin dorthin speicherst.")}</p>
  </details>

  {#if !supported}
    <p class="v-notice warn">{t("Die Kalender-Anbindung ist auf diesem System noch nicht eingerichtet.")}</p>
  {:else}
    {#if airGap}
      <div class="v-notice warn v-cal-air" role="status">
        <span>{t("Air Gap ist an: keine Verbindung zum Kalender.")}</span>
        <button type="button" class="v-btn v-btn-ghost" onclick={onToggleAirGap} disabled={airGapBusy}>{t("Air Gap ausschalten")}</button>
      </div>
    {/if}

    {#if failure}<p class="v-cal-problem" role="alert">{failure}</p>{/if}
    {#if notice}<p class="v-help" role="status">{notice}</p>{/if}

    {#if sources.length === 0}
      <p class="v-card-text">{t("Noch kein Kalender verbunden.")}</p>
    {:else}
      <ul class="v-list">
        {#each sources as source (source.id)}
          <li class="v-cal-source">
            <div class="v-cal-lines">
              <strong>{source.label} <span class="v-chip">{kindLabel(source)}{source.kind === "google_ics" ? ` · ${t("nur Lesen")}` : ""}</span></strong>
              <span>{source.calendars.map((calendar) => calendar.name).join(", ")}</span>
              <span class="v-cal-meta">{source.last_sync_unix_ms ? t("Zuletzt aktualisiert: {zeit}", { zeit: stamp(source.last_sync_unix_ms) }) : t("Noch nicht aktualisiert")}</span>
              {#if source.last_error}<span class="v-cal-problem" role="alert">{source.last_error}</span>{/if}
              {#if source.unsupported_rules > 0}<span class="v-cal-meta">{t("{n} Serien konnten nicht vollständig gelesen werden; es erscheint nur der erste Termin.", { n: source.unsupported_rules })}</span>{/if}
            </div>
            <div class="v-row v-cal-source-actions">
              <button type="button" class="v-btn v-btn-ghost" onclick={() => refresh(source.id)} disabled={airGap || syncing !== null}>{t(syncing === source.id ? "Aktualisiere …" : "Aktualisieren")}</button>
              {#if confirmRemove === source.id}
                <button type="button" class="v-btn v-btn-ghost v-cal-danger" onclick={() => remove(source)}>{t("Ja, trennen")}</button>
                <button type="button" class="v-btn v-btn-ghost" onclick={() => (confirmRemove = null)}>{t("Abbrechen")}</button>
              {:else}
                <button type="button" class="v-btn v-btn-ghost" onclick={() => (confirmRemove = source.id)}>{t("Trennen")}</button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}

    <div class="v-row v-cal-footer">
      <button type="button" class="v-btn v-btn-primary" onclick={() => (connecting = true)}>{t("Kalender verbinden")}</button>
      {#if sources.length > 1}
        <button type="button" class="v-btn v-btn-ghost" onclick={() => refresh(null)} disabled={airGap || syncing !== null}>{t(syncing === "all" ? "Aktualisiere …" : "Alle aktualisieren")}</button>
      {/if}
    </div>
    {#if confirmRemove}<p class="v-help">{t("Entfernt Zugang und Termine aus IAP. Dein Kalender bei Apple oder Google bleibt unverändert.")}</p>{/if}
  {/if}
</section>

{#if connecting}
  <CalendarConnectDialog {airGap} onClose={() => (connecting = false)} onDone={async () => { connecting = false; await onChanged(); }} />
{/if}

<style>
  .v-more summary { cursor: pointer; width: fit-content; color: var(--v-text-muted); font-size: var(--v-text-sm); }
  .v-more p { margin: var(--v-space-2) 0 0; }
  .v-cal-air { display: flex; align-items: center; justify-content: space-between; gap: var(--v-space-3); flex-wrap: wrap; }
  .v-cal-problem { margin: 0; color: var(--v-danger); font-size: var(--v-text-sm); }
  .v-cal-source { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: start; gap: var(--v-space-3); }
  .v-cal-lines { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .v-cal-lines > span { display: block; color: var(--v-text-secondary); font-size: var(--v-text-sm); overflow-wrap: anywhere; }
  .v-cal-source-actions { gap: var(--v-space-2); flex-wrap: wrap; justify-content: flex-end; }
  @media (max-width: 640px) { .v-cal-source { grid-template-columns: 1fr; } .v-cal-source-actions { justify-content: flex-start; } }
  .v-cal-lines > .v-cal-meta { color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .v-cal-lines > .v-cal-problem { color: var(--v-danger); }
  .v-cal-danger { color: var(--v-danger); }
  .v-cal-footer { gap: var(--v-space-2); flex-wrap: wrap; }
</style>
