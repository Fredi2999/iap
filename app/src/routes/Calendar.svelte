<script lang="ts">
  import { t, tk, locale } from "../lib/i18n/index.svelte";
  import { onDestroy, onMount } from "svelte";
  import { schedulePlan, importIcsText, exportIcsText, loadUiState, saveUiState, calendarOverview, calendarEvents, calendarCreateEvent, onCalendarChanged } from "../lib/ipc";
  import type { CalendarOverview, NewCalendarEvent, RemoteEvent, SchedulerEvent, SchedulerTask, SolvedPlan, SchedulerWeekday, ScheduledSlot } from "../lib/types";
  import { planningEvents, remoteToItem, writableTargets } from "../lib/calendarItems";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { addDays, addMonths, mergeBusy, startOfWeek, startOfDay, weekDays } from "../lib/calendar";
  import { appState } from "../lib/stores/app.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import CalendarWeek from "../lib/components/calendar/CalendarWeek.svelte";
  import CalendarMonth from "../lib/components/calendar/CalendarMonth.svelte";
  import EventDialog from "../lib/components/calendar/EventDialog.svelte";
  import TaskDialog from "../lib/components/calendar/TaskDialog.svelte";
  import CalendarSources from "../lib/components/calendar/CalendarSources.svelte";

  interface Props {
    /** Ob der Air Gap eingeschaltet ist; dann nimmt der Kalender keine Verbindung auf. */
    airGap?: boolean;
    airGapBusy?: boolean;
    onToggleAirGap?: () => void;
  }
  let { airGap = true, airGapBusy = false, onToggleAirGap = () => {} }: Props = $props();

  const TASKS_KEY = "ui.scheduler.tasks";
  const EVENTS_KEY = "ui.scheduler.events";
  const SETTINGS_KEY = "ui.scheduler.settings";

  type View = "week" | "month" | "list";
  interface PlannerSettings {
    days: SchedulerWeekday[];
    startMinute: number;
    endMinute: number;
    horizonDays: number;
    breakMinutes: number;
    energyBudget: number;
  }
  const ALL_DAYS: SchedulerWeekday[] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
  const DAY_LABEL: Record<SchedulerWeekday, string> = {
    monday: tk("Mo"), tuesday: tk("Di"), wednesday: tk("Mi"), thursday: tk("Do"), friday: tk("Fr"), saturday: tk("Sa"), sunday: tk("So"),
  };
  const DEFAULTS: PlannerSettings = { days: ALL_DAYS.slice(0, 5), startMinute: 9 * 60, endMinute: 17 * 60, horizonDays: 14, breakMinutes: 10, energyBudget: 6 };
  const PRIORITY_LABEL: Record<number, string> = { 1: tk("Niedrig"), 2: tk("Eher niedrig"), 3: tk("Normal"), 4: tk("Wichtig"), 5: tk("Dringend") };

  let events = $state<SchedulerEvent[]>([]);
  // Termine aus verbundenen Kalendern (Apple, Google): kommen aus dem Zwischenspeicher im Tresor,
  // sind hier schreibgeschützt und werden nie mit den eigenen Terminen vermischt gespeichert.
  let remote = $state<RemoteEvent[]>([]);
  let overview = $state<CalendarOverview | null>(null);
  let unlistenCalendar: UnlistenFn | null = null;
  const displayEvents = $derived<SchedulerEvent[]>([...events, ...remote.map(remoteToItem)]);
  const targets = $derived(writableTargets(overview?.sources ?? []));
  let tasks = $state<SchedulerTask[]>([]);
  let settings = $state<PlannerSettings>({ ...DEFAULTS });
  let plan = $state<SolvedPlan | null>(null);
  let error = $state<unknown>(null);
  let running = $state(false);
  let loaded = $state(false);
  let notice = $state<string | null>(null);

  let view = $state<View>("week");
  let anchor = $state(Date.now());
  let eventDialog = $state<{ event: SchedulerEvent | null; start: number } | null>(null);
  let taskDialog = $state<{ task: SchedulerTask | null } | null>(null);

  async function loadCalendars() {
    try {
      [overview, remote] = await Promise.all([calendarOverview(), calendarEvents()]);
    } catch (reason) {
      // Ohne Kalender-Anbindung (oder vor dem Entsperren) bleibt der Kalender wie er ist.
      console.error(reason);
    }
  }

  async function createRemote(sourceId: string, href: string, event: NewCalendarEvent, tzOffsetMinutes: number) {
    const created = await calendarCreateEvent(sourceId, href, event, tzOffsetMinutes);
    await loadCalendars();
    eventDialog = null;
    notice = t("Termin „{name}“ wurde im Kalender angelegt.", { name: created.title });
  }

  onDestroy(() => unlistenCalendar?.());

  // Die Befehlssuche kann „Neuer Termin“ oder „Neue Aufgabe“ auslösen; sobald die Seite bereit ist,
  // öffnet sich der passende Dialog (einmalig, danach wird die Anforderung gelöscht).
  $effect(() => {
    const action = appState.pendingCalendarAction;
    if (!action || !loaded) return;
    appState.pendingCalendarAction = null;
    if (action === "event") eventDialog = { event: null, start: Math.max(Date.now(), startOfDay(anchor)) };
    else taskDialog = { task: null };
  });

  // Aufgaben, Termine und Einstellungen liegen verschlüsselt im Vault, damit sie
  // einen Neustart überstehen und nicht auf dem Host-Rechner landen.
  onMount(async () => {
    try {
      const [savedTasks, savedEvents, savedSettings] = await Promise.all([loadUiState(TASKS_KEY), loadUiState(EVENTS_KEY), loadUiState(SETTINGS_KEY)]);
      if (savedTasks) tasks = JSON.parse(savedTasks) as SchedulerTask[];
      if (savedEvents) events = JSON.parse(savedEvents) as SchedulerEvent[];
      if (savedSettings) settings = { ...DEFAULTS, ...(JSON.parse(savedSettings) as Partial<PlannerSettings>) };
    } catch (reason) {
      error = reason;
    } finally {
      loaded = true;
    }
    await loadCalendars();
    try { unlistenCalendar = await onCalendarChanged(() => void loadCalendars()); } catch (reason) { console.error(reason); }
  });

  async function persist() {
    try {
      await Promise.all([saveUiState(TASKS_KEY, JSON.stringify(tasks)), saveUiState(EVENTS_KEY, JSON.stringify(events))]);
    } catch (reason) {
      error = reason;
    }
  }
  async function persistSettings() {
    try { await saveUiState(SETTINGS_KEY, JSON.stringify(settings)); } catch (reason) { error = reason; }
  }

  /** Nach jeder Änderung ist ein berechneter Plan überholt. */
  function changed() {
    plan = null;
    notice = null;
    void persist();
  }

  function saveEvent(event: SchedulerEvent) {
    events = events.some((entry) => entry.id === event.id) ? events.map((entry) => (entry.id === event.id ? event : entry)) : [...events, event];
    eventDialog = null;
    changed();
  }
  function deleteEvent(id: string) {
    events = events.filter((entry) => entry.id !== id);
    eventDialog = null;
    changed();
  }
  function saveTask(task: SchedulerTask) {
    tasks = tasks.some((entry) => entry.id === task.id) ? tasks.map((entry) => (entry.id === task.id ? task : entry)) : [...tasks, task];
    taskDialog = null;
    changed();
  }
  function deleteTask(id: string) {
    // Abhängigkeiten auf die gelöschte Aufgabe entfernen, sonst wäre sie nie planbar.
    tasks = tasks.filter((entry) => entry.id !== id).map((entry) => ({ ...entry, depends_on: entry.depends_on.filter((dep) => dep !== id) }));
    taskDialog = null;
    changed();
  }
  function toggleDone(task: SchedulerTask) {
    tasks = tasks.map((entry) => (entry.id === task.id ? { ...entry, status: entry.status === "done" ? "open" : "done" } : entry));
    changed();
  }
  function openTask(id: string) {
    const task = tasks.find((entry) => entry.id === id);
    if (task) taskDialog = { task };
  }

  async function runScheduler() {
    running = true; error = null;
    try {
      const availabilities = settings.days.map((weekday) => ({ weekday, start_minute: settings.startMinute, end_minute: settings.endMinute }));
      plan = await schedulePlan({
        now_unix_ms: Date.now(),
        tz_offset_minutes: -new Date().getTimezoneOffset(),
        horizon_days: settings.horizonDays,
        availabilities,
        // Der Planer verlangt überschneidungsfreie Termine; im Kalender sind Überschneidungen normal.
        events: mergeBusy(planningEvents(displayEvents)),
        tasks,
        break_minutes: settings.breakMinutes,
        day_energy_budget: settings.energyBudget,
      });
    } catch (reason) { error = reason; plan = null; }
    finally { running = false; }
  }

  async function importFile(ev: Event) {
    const input = ev.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    try {
      const result = await importIcsText(await file.text());
      // Ein zweiter Import derselben Datei soll nichts verdoppeln.
      const knownUids = new Set(events.map((entry) => entry.external_uid).filter(Boolean));
      const knownTasks = new Set(tasks.map((entry) => entry.id));
      const newEvents = result.events.filter((entry) => !entry.external_uid || !knownUids.has(entry.external_uid));
      const newTasks = result.tasks.filter((entry) => !knownTasks.has(entry.id));
      events = [...events, ...newEvents];
      tasks = [...tasks, ...newTasks];
      const skipped = result.events.length - newEvents.length + result.tasks.length - newTasks.length;
      changed();
      notice = skipped > 0
        ? t("{n} Einträge importiert, {m} schon vorhanden.", { n: newEvents.length + newTasks.length, m: skipped })
        : t("{n} Einträge importiert.", { n: newEvents.length + newTasks.length });
    } catch (reason) { error = reason; }
    finally { input.value = ""; }
  }

  async function downloadExport() {
    try {
      const text = await exportIcsText(events, tasks);
      const blob = new Blob([text], { type: "text/calendar" });
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = "iap-kalender.ics";
      a.click();
      URL.revokeObjectURL(a.href);
    } catch (reason) { error = reason; }
  }

  function time(ms: number) {
    return new Date(ms).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
  }

  function step(direction: -1 | 1) {
    anchor = view === "month" ? addMonths(anchor, direction) : addDays(anchor, 7 * direction);
  }

  const title = $derived.by(() => {
    if (view === "month") return new Date(anchor).toLocaleDateString(locale(), { month: "long", year: "numeric" });
    const days = weekDays(anchor);
    const from = new Date(days[0]);
    const to = new Date(days[6]);
    return `${from.toLocaleDateString(locale(), { day: "numeric", month: "short" })} – ${to.toLocaleDateString(locale(), { day: "numeric", month: "short", year: "numeric" })}`;
  });

  const plannedTasks = $derived((plan?.slots ?? []).filter((slot): slot is Extract<ScheduledSlot, { kind: "task" }> => slot.kind === "task"));
  const openTasks = $derived(tasks.filter((task) => task.status !== "done" && task.status !== "cancelled"));
  const doneTasks = $derived(tasks.filter((task) => task.status === "done" || task.status === "cancelled"));

  // Listenansicht: der Plan nach Tagen gruppiert.
  let planDays = $derived.by(() => {
    if (!plan) return [];
    const days = new Map<string, ScheduledSlot[]>();
    for (const slot of plan.slots) {
      const label = new Date(slot.start_unix_ms).toLocaleDateString(locale(), { weekday: "long", day: "numeric", month: "long" });
      days.set(label, [...(days.get(label) ?? []), slot]);
    }
    return [...days.entries()];
  });
  let taskTitle = $derived(new Map(tasks.map((task) => [task.id, task.title])));

  const minutesToTime = (minutes: number) => `${String(Math.floor(minutes / 60)).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;
  const timeToMinutes = (value: string) => { const [h, m] = value.split(":").map(Number); return Number.isFinite(h) && Number.isFinite(m) ? h * 60 + m : null; };
  function setTime(key: "startMinute" | "endMinute", value: string) {
    const minutes = timeToMinutes(value);
    if (minutes !== null) { settings[key] = minutes; void persistSettings(); plan = null; }
  }
  function toggleDay(day: SchedulerWeekday) {
    settings.days = settings.days.includes(day) ? settings.days.filter((entry) => entry !== day) : ALL_DAYS.filter((entry) => entry === day || settings.days.includes(entry));
    void persistSettings(); plan = null;
  }
  function setNumber(key: "horizonDays" | "breakMinutes" | "energyBudget", value: string, min: number, max: number) {
    const n = Math.round(Number(value));
    if (Number.isFinite(n)) { settings[key] = Math.min(max, Math.max(min, n)); void persistSettings(); plan = null; }
  }
  const settingsProblem = $derived(settings.days.length === 0 ? "Wähle mindestens einen Arbeitstag." : settings.endMinute <= settings.startMinute ? "Das Arbeitsende muss nach dem Arbeitsbeginn liegen." : null);
</script>

<div class="v-page">
  <PageHeader title={t("Kalender und Aufgaben")} description="Plane Termine und Aufgaben. IAP verteilt offene Aufgaben automatisch auf deine Arbeitszeit.">
    {#snippet actions()}
      <button class="v-btn v-btn-primary" onclick={() => (eventDialog = { event: null, start: Math.max(Date.now(), startOfDay(anchor)) })}>{t("Neuer Termin")}</button>
      <button class="v-btn v-btn-ghost" onclick={() => (taskDialog = { task: null })}>{t("Neue Aufgabe")}</button>
      <label class="v-btn v-btn-ghost v-file-button">{t("Kalender importieren")}<input type="file" accept=".ics" onchange={importFile} /></label>
      <button class="v-btn v-btn-ghost" onclick={downloadExport} disabled={tasks.length === 0 && events.length === 0}>{t("Exportieren")}</button>
    {/snippet}
    {#snippet help()}
      {t("Klicke in die Wochenansicht, um einen Termin anzulegen, oder auf einen Eintrag, um ihn zu ändern. Aufgaben legst du über „Neue Aufgabe“ an.")}
      {t("Importierte Termine aus Outlook, Apple Kalender oder Thunderbird (.ics) werden berücksichtigt. Alles bleibt offline.")}
      {t("Wenn du willst, verbindest du unten einen Apple- oder Google-Kalender. Das ist freiwillig und nur bei ausgeschaltetem Air Gap möglich.")}
    {/snippet}
  </PageHeader>

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}
  {#if notice}<p class="v-help" role="status">{notice}</p>{/if}

  <section class="v-card v-stack">
    <div class="v-row v-cal-toolbar">
      <button class="v-btn v-btn-ghost" onclick={() => step(-1)} disabled={view === "list"} aria-label={t("Zurück")}>‹</button>
      <button class="v-btn v-btn-ghost" onclick={() => (anchor = Date.now())} disabled={view === "list"}>{t("Heute")}</button>
      <button class="v-btn v-btn-ghost" onclick={() => step(1)} disabled={view === "list"} aria-label={t("Weiter")}>›</button>
      <h2 class="v-cal-title">{view === "list" ? t("Wochenplan") : title}</h2>
      <span class="v-cal-spacer"></span>
      <div class="v-segmented" role="group" aria-label={t("Ansicht")}>
        {#each [["week", tk("Woche")], ["month", tk("Monat")], ["list", tk("Liste")]] as [id, label] (id)}
          <button type="button" class:active={view === id} aria-pressed={view === id} onclick={() => (view = id as View)}>{t(label)}</button>
        {/each}
      </div>
    </div>

    {#if view === "week"}
      <CalendarWeek {anchor} events={displayEvents} planned={plannedTasks} onEvent={(event) => (eventDialog = { event, start: event.start_unix_ms })} onTask={openTask} onCreate={(start) => (eventDialog = { event: null, start })} />
    {:else if view === "month"}
      <CalendarMonth {anchor} events={displayEvents} planned={plannedTasks} {tasks} onEvent={(event) => (eventDialog = { event, start: event.start_unix_ms })} onDay={(day) => { anchor = day; view = "week"; }} />
    {:else if !plan}
      <p class="v-card-text">{t("Berechne den Wochenplan, um ihn hier als Liste zu sehen.")}</p>
    {:else}
      {#if plan.slots.length === 0}<p class="v-card-text">{t("Keine freien Zeitfenster gefunden.")}</p>{/if}
      {#each planDays as [day, slots] (day)}
        <div class="v-plan-day">
          <h3>{day}</h3>
          <ul class="v-list">
            {#each slots as slot, i (i)}
              <li class:v-plan-break={slot.kind === "break"}>
                <span class="v-num v-plan-time">{time(slot.start_unix_ms)}</span>
                <div class="v-list-main"><strong>{slot.kind === "break" ? t("Pause") : slot.title}</strong>{#if slot.kind === "event"}<span>{t("Termin")}</span>{/if}</div>
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    {/if}
  </section>

  <CalendarSources {overview} {airGap} {airGapBusy} {onToggleAirGap} onChanged={loadCalendars} />

  <div class="v-grid-aside">
    <section class="v-card">
      <h2 class="v-card-title">{t("Aufgaben")} <small>{t("{n} offen", { n: openTasks.length })}{displayEvents.length ? `, ${t("{n} Termine", { n: displayEvents.length })}` : ""}</small></h2>
      {#if loaded && tasks.length === 0}
        <EmptyState title={t("Noch keine Aufgaben")} text="Lege eine Aufgabe an oder importiere einen Kalender." icon="M9 11l3 3 8-8M20 12v7a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" />
      {:else}
        <ul class="v-list">
          {#each [...openTasks, ...doneTasks] as task (task.id)}
            {@const done = task.status === "done" || task.status === "cancelled"}
            <li class:v-task-done={done}>
              <input type="checkbox" checked={task.status === "done"} disabled={task.status === "cancelled"} aria-label={t("Als erledigt markieren: {name}", { name: task.title })} onchange={() => toggleDone(task)} />
              <button type="button" class="v-list-main v-task-open" onclick={() => (taskDialog = { task })}>
                <strong>{task.title}</strong>
                <span>{t("{n} Minuten", { n: task.duration_minutes })} · {t(PRIORITY_LABEL[task.priority] ?? "Normal")}{task.due_unix_ms ? ` · ${t("fällig")} ${new Date(task.due_unix_ms).toLocaleDateString(locale(), { day: "numeric", month: "short" })}` : ""}{task.project ? ` · ${task.project}` : ""}</span>
              </button>
              <button class="v-btn-icon" aria-label={t("Aufgabe entfernen: {name}", { name: task.title })} title={t("Entfernen")} onclick={() => deleteTask(task.id)}>
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3m-9 0 1 13h10l1-13"/></svg>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <section class="v-card v-stack">
      <h2 class="v-card-title">{t("Planer")}</h2>
      <p data-hint class="v-card-text">{t("Berechnet konfliktfreie Zeitfenster für alle offenen Aufgaben.")}</p>
      <details class="v-cal-settings">
        <summary>{t("Arbeitszeit und Grenzen")}</summary>
        <div class="v-cal-settings-body">
          <div class="v-row">
            {#each ALL_DAYS as day (day)}
              <label class="v-cal-day"><input type="checkbox" checked={settings.days.includes(day)} onchange={() => toggleDay(day)} /> {t(DAY_LABEL[day])}</label>
            {/each}
          </div>
          <div class="v-cal-two">
            <label class="v-field"><span class="v-label">{t("Arbeitsbeginn")}</span><input type="time" value={minutesToTime(settings.startMinute)} onchange={(e) => setTime("startMinute", e.currentTarget.value)} /></label>
            <label class="v-field"><span class="v-label">{t("Arbeitsende")}</span><input type="time" value={minutesToTime(settings.endMinute)} onchange={(e) => setTime("endMinute", e.currentTarget.value)} /></label>
            <label class="v-field"><span class="v-label">{t("Planungszeitraum in Tagen")}</span><input type="number" min="1" max="28" value={settings.horizonDays} onchange={(e) => setNumber("horizonDays", e.currentTarget.value, 1, 28)} /></label>
            <label class="v-field"><span class="v-label">{t("Pause zwischen Aufgaben (Min.)")}</span><input type="number" min="0" max="60" value={settings.breakMinutes} onchange={(e) => setNumber("breakMinutes", e.currentTarget.value, 0, 60)} /></label>
            <label class="v-field"><span class="v-label">{t("Anstrengung pro Tag")}</span><input type="number" min="1" max="10" value={settings.energyBudget} onchange={(e) => setNumber("energyBudget", e.currentTarget.value, 1, 10)} /><span data-hint class="v-help">{t("Leichte Aufgaben zählen 1, mittlere 2, fordernde 3.")}</span></label>
          </div>
          {#if settingsProblem}<p class="v-cal-problem" role="alert">{t(settingsProblem)}</p>{/if}
        </div>
      </details>
      <button class="v-btn v-btn-primary" onclick={runScheduler} disabled={running || openTasks.length === 0 || settingsProblem !== null}>{t(running ? "Plane …" : "Wochenplan berechnen")}</button>
      {#if plan}
        <p class="v-help" role="status">{t("{n} Aufgaben eingeplant.", { n: plannedTasks.length })}</p>
        {#if plan.unscheduled.length > 0}
          <div class="v-notice warn">
            <strong>{t("Nicht untergebracht:")}</strong>
            <ul>{#each plan.unscheduled as u (u.task_id)}<li>{taskTitle.get(u.task_id) ?? u.task_id}: {u.reason}</li>{/each}</ul>
          </div>
        {/if}
      {/if}
    </section>
  </div>
</div>

{#if eventDialog}
  <EventDialog event={eventDialog.event} start={eventDialog.start} {targets} {airGap} onSave={saveEvent} onCreateRemote={createRemote} onDelete={deleteEvent} onClose={() => (eventDialog = null)} />
{/if}
{#if taskDialog}
  <TaskDialog task={taskDialog.task} {tasks} onSave={saveTask} onDelete={deleteTask} onClose={() => (taskDialog = null)} />
{/if}

<style>
  .v-cal-toolbar { gap: var(--v-space-2); }
  .v-cal-title { margin: 0 var(--v-space-2); color: var(--v-text-primary); font-size: var(--v-text-lg); font-weight: 600; }
  .v-cal-spacer { flex: 1; }
  .v-plan-day h3 { margin: var(--v-space-3) 0 var(--v-space-1); color: var(--v-text-primary); font-size: var(--v-text-sm); font-weight: 600; }
  .v-plan-time { width: 3.2rem; flex: 0 0 auto; color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .v-plan-break .v-list-main strong { color: var(--v-text-muted); font-weight: 400; }
  .v-notice ul { margin: var(--v-space-1) 0 0; padding-left: 1.1rem; }
  .v-task-open { flex: 1; border: 0; background: transparent; text-align: left; color: inherit; cursor: pointer; padding: 0; }
  .v-task-done .v-list-main { opacity: .55; }
  .v-task-done strong { text-decoration: line-through; }
  .v-cal-settings summary { cursor: pointer; color: var(--v-text-secondary); font-size: var(--v-text-sm); }
  .v-cal-settings-body { display: flex; flex-direction: column; gap: var(--v-space-3); margin-top: var(--v-space-3); }
  .v-cal-two { display: grid; grid-template-columns: 1fr 1fr; gap: var(--v-space-3); }
  .v-cal-day { display: inline-flex; align-items: center; gap: 4px; color: var(--v-text-secondary); font-size: var(--v-text-sm); }
  .v-cal-problem { margin: 0; color: var(--v-danger); font-size: var(--v-text-sm); }
</style>
