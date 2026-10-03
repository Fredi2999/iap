<script lang="ts">
  // Übersicht auf der Startseite, direkt unter dem Pet: anstehende Termine (eigene und aus
  // verbundenen Kalendern), offene Aufgaben und – wenn Gmail eingerichtet ist – der Stand der
  // Auto-Antwort mit bereitliegenden Entwürfen. Alles kommt aus dem Tresor bzw. dem lokalen
  // Status; es wird dafür nichts ins Netz geladen (Kalender werden nur im Kalender-Tab abgerufen).
  import { onMount, onDestroy } from "svelte";
  import { t, locale } from "../i18n/index.svelte";
  import { loadUiState, mailStatus, mailLog, onMailChanged, calendarEvents, onCalendarChanged } from "../ipc";
  import { remoteToItem, upcomingEvents } from "../calendarItems";
  import type { SchedulerTask, SchedulerEvent, MailStatus, MailLogEntry } from "../types";

  interface Props {
    /** Beim Wechsel zu einer Route; die Übersicht ist nur eine Abkürzung. */
    onOpen: (route: "calendar" | "connectors") => void;
  }
  let { onOpen }: Props = $props();

  let tasks = $state<SchedulerTask[]>([]);
  let mail = $state<MailStatus | null>(null);
  let log = $state<MailLogEntry[]>([]);
  let ownEvents = $state<SchedulerEvent[]>([]);
  let linkedEvents = $state<SchedulerEvent[]>([]);
  let unlisten: (() => void) | null = null;
  let unlistenCalendar: (() => void) | null = null;

  const openTasks = $derived(
    tasks
      .filter((task) => task.status !== "done" && task.status !== "cancelled")
      .sort((a, b) => (a.due_unix_ms ?? Infinity) - (b.due_unix_ms ?? Infinity))
      .slice(0, 4),
  );
  // Die nächsten Termine der kommenden sieben Tage; zählt auch, was schon läuft.
  const upcoming = $derived(upcomingEvents([...ownEvents, ...linkedEvents], Date.now(), 7, 4));
  // Entwürfe, die auf Freigabe warten: das ist echtes To-do, nicht nur eine Meldung.
  const pendingDrafts = $derived(log.filter((entry) => entry.kind === "drafted").length);

  async function loadTasks() {
    try {
      const saved = await loadUiState("ui.scheduler.tasks");
      if (saved) tasks = JSON.parse(saved) as SchedulerTask[];
    } catch (reason) { console.error(reason); }
  }

  async function loadEvents() {
    try {
      const saved = await loadUiState("ui.scheduler.events");
      ownEvents = saved ? (JSON.parse(saved) as SchedulerEvent[]) : [];
    } catch (reason) { console.error(reason); }
    try {
      linkedEvents = (await calendarEvents()).map(remoteToItem);
    } catch (reason) {
      // Ohne Kalender-Anbindung gibt es hier nichts anzuzeigen.
      linkedEvents = [];
    }
  }

  async function loadMail() {
    try {
      mail = await mailStatus();
      if (mail?.enabled) log = await mailLog();
    } catch (reason) {
      // Vor dem Entsperren oder ohne Gmail ist das in Ordnung.
      mail = null;
    }
  }

  onMount(async () => {
    await loadTasks();
    await loadEvents();
    await loadMail();
    try { unlistenCalendar = await onCalendarChanged(() => void loadEvents()); } catch (reason) { console.error(reason); }
    try { unlisten = await onMailChanged(() => void loadMail()); } catch (reason) { console.error(reason); }
  });
  onDestroy(() => { unlisten?.(); unlistenCalendar?.(); });

  function whenLabel(event: SchedulerEvent): string {
    const day = new Date(event.start_unix_ms);
    const now = Date.now();
    const days = Math.round((new Date(day).setHours(0, 0, 0, 0) - new Date(now).setHours(0, 0, 0, 0)) / 86_400_000);
    const running = event.start_unix_ms <= now && event.end_unix_ms > now;
    const dayText = running || days <= 0 ? t("heute") : days === 1 ? t("morgen") : day.toLocaleDateString(locale(), { weekday: "short", day: "2-digit", month: "2-digit" });
    if (event.all_day) return `${dayText} · ${t("ganztägig")}`;
    return running ? t("jetzt") : `${dayText} · ${day.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" })}`;
  }

  function dueLabel(task: SchedulerTask): string {
    if (!task.due_unix_ms) return "";
    const days = Math.round((task.due_unix_ms - Date.now()) / 86_400_000);
    if (days < 0) return t("überfällig");
    if (days === 0) return t("heute");
    if (days === 1) return t("morgen");
    return new Date(task.due_unix_ms).toLocaleDateString(locale(), { day: "2-digit", month: "2-digit" });
  }
  const overdue = (task: SchedulerTask) => task.due_unix_ms != null && task.due_unix_ms < Date.now();
</script>

{#if openTasks.length > 0 || upcoming.length > 0 || mail?.enabled}
  <div class="v-overview">
    {#if upcoming.length > 0}
      <section class="v-ov-card" aria-label={t("Anstehende Termine")}>
        <header>
          <span class="v-ov-title">{t("Anstehende Termine")}</span>
          <button type="button" class="v-ov-link" onclick={() => onOpen("calendar")}>{t("Alle")}</button>
        </header>
        <ul>
          {#each upcoming as event (event.id + event.start_unix_ms)}
            <li>
              <span class="v-ov-dot" style:background={event.remote?.color ?? null} aria-hidden="true"></span>
              <span class="v-ov-task">{event.title}</span>
              <span class="v-ov-due">{whenLabel(event)}</span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if openTasks.length > 0}
      <section class="v-ov-card" aria-label={t("Anstehende Aufgaben")}>
        <header>
          <span class="v-ov-title">{t("Anstehende Aufgaben")}</span>
          <button type="button" class="v-ov-link" onclick={() => onOpen("calendar")}>{t("Alle")}</button>
        </header>
        <ul>
          {#each openTasks as task (task.id)}
            <li>
              <span class="v-ov-dot" class:urgent={overdue(task)} aria-hidden="true"></span>
              <span class="v-ov-task">{task.title}</span>
              {#if dueLabel(task)}<span class="v-ov-due" class:urgent={overdue(task)}>{dueLabel(task)}</span>{/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if mail?.enabled}
      <section class="v-ov-card" aria-label={t("Gmail")}>
        <header>
          <span class="v-ov-title">{t("Gmail")}</span>
          <button type="button" class="v-ov-link" onclick={() => onOpen("connectors")}>{t("Öffnen")}</button>
        </header>
        <div class="v-ov-mail">
          <span class="v-chip" class:accent={mail.polling}>{t(mail.polling ? (mail.running ? "Prüft gerade" : "Läuft") : "Aus")}</span>
          {#if pendingDrafts > 0}
            <span class="v-ov-draft">{t("{n} Entwürfe warten auf Freigabe", { n: pendingDrafts })}</span>
          {:else}
            <span class="v-ov-muted">{t("Heute {n} Antworten", { n: mail.sent_today })}</span>
          {/if}
        </div>
        {#if mail.blocked_reason && !mail.polling}
          <p class="v-ov-muted">{t("Nicht aktiv. Öffne Gmail, um den Grund zu sehen.")}</p>
        {/if}
      </section>
    {/if}
  </div>
{/if}

<style>
  .v-overview { display: grid; grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr)); gap: var(--v-space-3); width: min(100%, 44rem); margin-top: var(--v-space-2); }
  .v-ov-card { display: grid; gap: var(--v-space-2); padding: var(--v-space-3) var(--v-space-4); border: 1px solid var(--v-line); border-radius: var(--v-radius-card); background: var(--v-surface-1); text-align: left; }
  .v-ov-card header { display: flex; align-items: center; justify-content: space-between; gap: var(--v-space-2); }
  .v-ov-title { color: var(--v-text-secondary); font-size: var(--v-text-xs); font-weight: 600; letter-spacing: .02em; }
  .v-ov-link { border: 0; background: transparent; color: var(--v-accent-blue); font-size: var(--v-text-xs); cursor: pointer; }
  .v-ov-link:hover { text-decoration: underline; }
  .v-ov-card ul { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; }
  .v-ov-card li { display: flex; align-items: center; gap: var(--v-space-2); min-width: 0; }
  .v-ov-dot { flex: 0 0 auto; width: 6px; height: 6px; border-radius: 50%; background: var(--v-accent-blue); }
  .v-ov-dot.urgent { background: var(--v-danger); }
  .v-ov-task { flex: 1 1 auto; min-width: 0; overflow: hidden; color: var(--v-text-primary); font-size: var(--v-text-sm); text-overflow: ellipsis; white-space: nowrap; }
  .v-ov-due { flex: 0 0 auto; color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .v-ov-due.urgent { color: var(--v-danger); }
  .v-ov-mail { display: flex; flex-wrap: wrap; align-items: center; gap: var(--v-space-2); }
  .v-ov-draft { color: var(--v-text-primary); font-size: var(--v-text-sm); }
  .v-ov-muted { color: var(--v-text-muted); font-size: var(--v-text-sm); margin: 0; }
</style>
