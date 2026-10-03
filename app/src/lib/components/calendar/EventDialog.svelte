<script lang="ts">
  // Termin anlegen oder bearbeiten. Importierte Termine sind genauso bearbeitbar. Termine aus einem
  // verbundenen Kalender (Apple, Google) sind hier schreibgeschützt; neue Termine können wahlweise
  // nur in IAP oder direkt in einem verbundenen Apple-Kalender landen.
  import { untrack } from "svelte";
  import { t, locale } from "../../i18n/index.svelte";
  import { fromLocalInput, newId, toDateInput, toLocalInput } from "../../calendar";
  import { calendarError, localOffsetMinutes, type CalendarTarget } from "../../calendarItems";
  import type { NewCalendarEvent, SchedulerEvent } from "../../types";
  import Modal from "./Modal.svelte";

  interface Props {
    /** `null` = neuer Termin. */
    event: SchedulerEvent | null;
    /** Vorschlag für den Beginn eines neuen Termins (Klick ins Raster). */
    start?: number;
    /** Beschreibbare Kalender verbundener Konten; leer, wenn nichts verbunden ist. */
    targets?: CalendarTarget[];
    airGap?: boolean;
    onSave: (event: SchedulerEvent) => void;
    /** Legt den Termin im verbundenen Kalender an; wirft bei Fehlern (der Dialog zeigt sie an). */
    onCreateRemote?: (sourceId: string, href: string, event: NewCalendarEvent, tzOffsetMinutes: number) => Promise<void>;
    onDelete: (id: string) => void;
    onClose: () => void;
  }
  let { event, start = Date.now(), targets = [], airGap = false, onSave, onCreateRemote, onDelete, onClose }: Props = $props();

  const remote = $derived(event?.remote ?? null);

  // Startwerte bewusst einmalig übernehmen; danach gehört der Entwurf dem Dialog.
  let title = $state(untrack(() => event?.title ?? ""));
  let from = $state(untrack(() => toLocalInput(event?.start_unix_ms ?? start)));
  let to = $state(untrack(() => toLocalInput(event?.end_unix_ms ?? start + 3_600_000)));
  let location = $state(untrack(() => event?.location ?? ""));
  let notes = $state("");
  let target = $state("local");
  let allDay = $state(false);
  let fromDate = $state(untrack(() => toDateInput(event?.start_unix_ms ?? start)));
  let toDate = $state(untrack(() => toDateInput(event?.start_unix_ms ?? start)));
  let busy = $state(false);
  let failure = $state<string | null>(null);

  const chosen = $derived(target === "local" ? null : targets.find((entry) => `${entry.sourceId}::${entry.href}` === target) ?? null);
  const startMs = $derived(fromLocalInput(from));
  const endMs = $derived(fromLocalInput(to));
  const problem = $derived(
    !title.trim() ? "Bitte einen Titel eingeben."
      : chosen && allDay ? (!fromDate || !toDate ? "Bitte Beginn und Ende angeben." : toDate < fromDate ? "Das Ende muss nach dem Beginn liegen." : null)
      : startMs === null || endMs === null ? "Bitte Beginn und Ende angeben."
      : endMs <= startMs ? "Das Ende muss nach dem Beginn liegen."
      : null,
  );

  async function save(submit: SubmitEvent) {
    submit.preventDefault();
    if (problem) return;
    if (chosen && onCreateRemote) {
      busy = true;
      failure = null;
      try {
        await onCreateRemote(chosen.sourceId, chosen.href, {
          title: title.trim(),
          all_day: allDay,
          start_unix_ms: startMs ?? 0,
          end_unix_ms: endMs ?? 0,
          start_date: allDay ? fromDate : null,
          end_date: allDay ? toDate : null,
          location: location.trim() || null,
          notes: notes.trim() || null,
        }, localOffsetMinutes());
      } catch (reason) {
        failure = calendarError(reason);
      } finally {
        busy = false;
      }
      return;
    }
    if (startMs === null || endMs === null) return;
    onSave({
      ...(event ?? { id: newId("e"), project: null, external_uid: null }),
      title: title.trim(),
      start_unix_ms: startMs,
      end_unix_ms: endMs,
      location: location.trim() || null,
    });
  }

  function when(item: SchedulerEvent): string {
    const opts: Intl.DateTimeFormatOptions = { weekday: "short", day: "numeric", month: "long", year: "numeric" };
    if (item.all_day) {
      const first = new Date(item.start_unix_ms);
      const last = new Date(item.end_unix_ms - 1);
      const a = first.toLocaleDateString(locale(), opts);
      return first.toDateString() === last.toDateString() ? a : `${a} – ${last.toLocaleDateString(locale(), opts)}`;
    }
    const time: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit" };
    const day = new Date(item.start_unix_ms).toLocaleDateString(locale(), opts);
    return `${day}, ${new Date(item.start_unix_ms).toLocaleTimeString(locale(), time)} – ${new Date(item.end_unix_ms).toLocaleTimeString(locale(), time)}`;
  }
</script>

{#if remote && event}
  <Modal title={event.title} {onClose}>
    <dl class="v-cal-detail">
      <dt>{t("Wann")}</dt><dd>{when(event)}</dd>
      <dt>{t("Kalender")}</dt><dd>{remote.source_label} · {remote.calendar_name}</dd>
      {#if remote.location}<dt>{t("Ort")}</dt><dd>{remote.location}</dd>{/if}
      {#if remote.recurring}<dt>{t("Serie")}</dt><dd>{t("Wiederholt sich")}</dd>{/if}
      {#if remote.notes}<dt>{t("Notiz")}</dt><dd class="v-cal-notes">{remote.notes}</dd>{/if}
    </dl>
    <p class="v-help">{t("Dieser Termin kommt aus einem verbundenen Kalender und ist in IAP schreibgeschützt. Ändere ihn dort, wo er angelegt wurde, und aktualisiere dann hier.")}</p>
    <div class="v-row v-cal-actions"><span class="v-cal-spacer"></span><button type="button" class="v-btn v-btn-primary" onclick={onClose}>{t("Schließen")}</button></div>
  </Modal>
{:else}
  <Modal title={t(event ? "Termin bearbeiten" : "Neuer Termin")} {onClose}>
    <form class="v-cal-form" onsubmit={save}>
      <label class="v-field"><span class="v-label">{t("Titel")}</span><input bind:value={title} maxlength="120" placeholder={t("Zum Beispiel: Zahnarzt")} /></label>
      {#if !event && targets.length > 0}
        <label class="v-field">
          <span class="v-label">{t("Speichern in")}</span>
          <select bind:value={target}>
            <option value="local">{t("Nur in IAP (bleibt auf dem Stick)")}</option>
            {#each targets as entry (entry.sourceId + entry.href)}<option value={`${entry.sourceId}::${entry.href}`}>{entry.label}</option>{/each}
          </select>
        </label>
      {/if}
      {#if chosen}
        <label class="v-cal-check"><input type="checkbox" bind:checked={allDay} /> {t("Ganztägig")}</label>
      {/if}
      {#if chosen && allDay}
        <div class="v-cal-two">
          <label class="v-field"><span class="v-label">{t("Erster Tag")}</span><input type="date" bind:value={fromDate} /></label>
          <label class="v-field"><span class="v-label">{t("Letzter Tag")}</span><input type="date" bind:value={toDate} /></label>
        </div>
      {:else}
        <div class="v-cal-two">
          <label class="v-field"><span class="v-label">{t("Beginn")}</span><input type="datetime-local" bind:value={from} /></label>
          <label class="v-field"><span class="v-label">{t("Ende")}</span><input type="datetime-local" bind:value={to} /></label>
        </div>
      {/if}
      <label class="v-field"><span class="v-label">{t("Ort")}</span><input bind:value={location} maxlength="120" /></label>
      {#if chosen}
        <label class="v-field"><span class="v-label">{t("Notiz")}</span><textarea bind:value={notes} rows="3" maxlength="2000"></textarea></label>
        <p class="v-help">{t("Beim Speichern verbindet sich IAP mit iCloud und legt den Termin dort an. Titel, Zeit, Ort und Notiz gehen dabei an Apple.")}</p>
        {#if airGap}<p class="v-notice warn" role="status">{t("Air Gap ist eingeschaltet. Schalte ihn aus, um in diesen Kalender zu speichern.")}</p>{/if}
      {/if}
      {#if failure}<p class="v-cal-problem" role="alert">{failure}</p>{/if}
      {#if problem && (title || event)}<p class="v-cal-problem" role="alert">{t(problem)}</p>{/if}
      <div class="v-row v-cal-actions">
        {#if event}<button type="button" class="v-btn v-btn-ghost v-cal-delete" onclick={() => onDelete(event.id)}>{t("Löschen")}</button>{/if}
        <span class="v-cal-spacer"></span>
        <button type="button" class="v-btn v-btn-ghost" onclick={onClose}>{t("Abbrechen")}</button>
        <button type="submit" class="v-btn v-btn-primary" disabled={problem !== null || busy || (chosen !== null && airGap)}>{t(busy ? "Speichere …" : "Speichern")}</button>
      </div>
    </form>
  </Modal>
{/if}

<style>
  .v-cal-form { display: flex; flex-direction: column; gap: var(--v-space-4); }
  .v-cal-two { display: grid; grid-template-columns: 1fr 1fr; gap: var(--v-space-3); }
  @media (max-width: 480px) { .v-cal-two { grid-template-columns: 1fr; } }
  .v-cal-actions { gap: var(--v-space-2); }
  .v-cal-spacer { flex: 1; }
  .v-cal-delete { color: var(--v-danger); }
  .v-cal-problem { margin: 0; color: var(--v-danger); font-size: var(--v-text-sm); }
  .v-cal-check { display: inline-flex; align-items: center; gap: var(--v-space-2); color: var(--v-text-secondary); font-size: var(--v-text-sm); }
  .v-cal-detail { display: grid; grid-template-columns: max-content 1fr; gap: var(--v-space-2) var(--v-space-4); margin: 0; }
  .v-cal-detail dt { color: var(--v-text-muted); font-size: var(--v-text-sm); }
  .v-cal-detail dd { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-sm); }
  .v-cal-notes { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
