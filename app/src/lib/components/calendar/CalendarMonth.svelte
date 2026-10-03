<script lang="ts">
  // Monatsraster mit Terminen, geplanten Aufgaben und Fälligkeiten als kurze Zeilen je Tag.
  import { t, locale } from "../../i18n/index.svelte";
  import { isSameDay, monthGrid, spansOnDay, startOfDay } from "../../calendar";
  import type { ScheduledSlot, SchedulerEvent, SchedulerTask } from "../../types";

  type TaskSlot = Extract<ScheduledSlot, { kind: "task" }>;
  interface Props {
    /** Ein beliebiger Zeitpunkt im anzuzeigenden Monat. */
    anchor: number;
    events: SchedulerEvent[];
    planned: TaskSlot[];
    tasks: SchedulerTask[];
    onDay: (dayMs: number) => void;
    onEvent: (event: SchedulerEvent) => void;
  }
  let { anchor, events, planned, tasks, onDay, onEvent }: Props = $props();

  const MAX_ROWS = 3;
  const grid = $derived(monthGrid(anchor));
  const month = $derived(new Date(anchor).getMonth());
  const today = Date.now();
  const weekdayNames = $derived(grid.slice(0, 7).map((day) => new Date(day).toLocaleDateString(locale(), { weekday: "short" })));

  interface Row { key: string; label: string; kind: "event" | "task" | "due"; event?: SchedulerEvent }

  function rowsFor(day: number): Row[] {
    const rows: Row[] = [
      ...spansOnDay(events, day).map((event): Row => ({ key: "e" + event.id + event.start_unix_ms, label: event.title, kind: "event", event })),
      ...spansOnDay(planned, day).map((slot): Row => ({ key: "p" + slot.task_id, label: slot.title, kind: "task" })),
      ...tasks
        .filter((task) => task.due_unix_ms && task.status !== "done" && task.status !== "cancelled" && startOfDay(task.due_unix_ms) === day)
        .map((task): Row => ({ key: "d" + task.id, label: task.title, kind: "due" })),
    ];
    return rows;
  }
</script>

<div class="v-month" role="grid" aria-label={t("Monatsansicht")}>
  <div class="v-month-head" role="row">
    {#each weekdayNames as name (name)}<span role="columnheader">{name}</span>{/each}
  </div>
  <div class="v-month-grid">
    {#each grid as day (day)}
      {@const rows = rowsFor(day)}
      <div class="v-month-cell" class:other={new Date(day).getMonth() !== month} class:today={isSameDay(day, today)} role="gridcell">
        <button type="button" class="v-month-num" onclick={() => onDay(day)} aria-label={new Date(day).toLocaleDateString(locale(), { weekday: "long", day: "numeric", month: "long" })}>{new Date(day).getDate()}</button>
        {#each rows.slice(0, MAX_ROWS) as row (row.key)}
          {#if row.event}
            {@const event = row.event}
            <button type="button" class="v-month-row {row.kind}" class:remote={!!event.remote} style:border-left-color={event.remote?.color ?? null} onclick={() => onEvent(event)} title={row.label}>{row.label}</button>
          {:else}
            <span class="v-month-row {row.kind}" title={row.label}>{row.kind === "due" ? t("Fällig:") + " " : ""}{row.label}</span>
          {/if}
        {/each}
        {#if rows.length > MAX_ROWS}<button type="button" class="v-month-more" onclick={() => onDay(day)}>{t("+{n} weitere", { n: rows.length - MAX_ROWS })}</button>{/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .v-month { border: 1px solid var(--v-line); border-radius: var(--v-radius-card); overflow: hidden; }
  .v-month-head, .v-month-grid { display: grid; grid-template-columns: repeat(7, minmax(0, 1fr)); }
  .v-month-head { background: var(--v-surface-1); border-bottom: 1px solid var(--v-line); }
  .v-month-head span { padding: var(--v-space-2); text-align: center; color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .v-month-cell { display: flex; flex-direction: column; gap: 2px; min-height: 5.6rem; min-width: 0; padding: 4px; border-top: 1px solid var(--v-line); border-left: 1px solid var(--v-line); }
  .v-month-cell:nth-child(7n + 1) { border-left: 0; }
  .v-month-cell:nth-child(-n + 7) { border-top: 0; }
  .v-month-cell.other { opacity: .5; }
  .v-month-cell.today { background: var(--v-accent-blue-soft); }
  .v-month-num { align-self: flex-start; min-width: 1.6rem; padding: 0 4px; border: 0; background: transparent; color: var(--v-text-secondary); font-size: var(--v-text-sm); cursor: pointer; }
  .v-month-cell.today .v-month-num { border-radius: 999px; background: var(--v-accent-blue); color: var(--v-cta-text); }
  .v-month-row { display: block; width: 100%; box-sizing: border-box; padding: 1px 5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; border: 0; border-left: 3px solid transparent; border-radius: 4px; text-align: left; font-size: var(--v-text-xs); line-height: 1.35; color: var(--v-text-primary); }
  button.v-month-row { cursor: pointer; }
  .v-month-row.event { background: var(--v-accent-blue-soft); border-left-color: var(--v-accent-blue); }
  .v-month-row.event.remote { background: var(--v-surface-3); }
  .v-month-row.task { background: var(--v-surface-3); border-left-color: var(--v-success); }
  .v-month-row.due { border-left-color: var(--v-warning); color: var(--v-text-secondary); }
  .v-month-more { align-self: flex-start; padding: 0 5px; border: 0; background: transparent; color: var(--v-text-muted); font-size: var(--v-text-xs); cursor: pointer; }
  @media (max-width: 720px) { .v-month-cell { min-height: 4rem; } }
</style>
