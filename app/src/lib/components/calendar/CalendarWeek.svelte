<script lang="ts">
  // Wochenraster: sieben Tage, Stundenzeilen, Termine und vom Planer eingeplante Aufgaben als Blöcke.
  import { t, locale } from "../../i18n/index.svelte";
  import { addDays, isSameDay, layoutOverlaps, spansOnDay, weekDays } from "../../calendar";
  import type { ScheduledSlot, SchedulerEvent } from "../../types";

  type TaskSlot = Extract<ScheduledSlot, { kind: "task" }>;
  interface Props {
    /** Ein beliebiger Zeitpunkt in der anzuzeigenden Woche. */
    anchor: number;
    events: SchedulerEvent[];
    planned: TaskSlot[];
    onEvent: (event: SchedulerEvent) => void;
    onTask: (taskId: string) => void;
    /** Klick in leere Fläche: Beginn des neuen Termins. */
    onCreate: (startMs: number) => void;
  }
  let { anchor, events, planned, onEvent, onTask, onCreate }: Props = $props();

  const FIRST_HOUR = 6;
  const LAST_HOUR = 22;
  const HOUR_PX = 48;
  const hours = Array.from({ length: LAST_HOUR - FIRST_HOUR }, (_, i) => FIRST_HOUR + i);
  const days = $derived(weekDays(anchor));
  const now = $state({ ms: Date.now() });
  $effect(() => {
    const timer = setInterval(() => { now.ms = Date.now(); }, 60_000);
    return () => clearInterval(timer);
  });

  type Block =
    | { kind: "event"; id: string; title: string; start_unix_ms: number; end_unix_ms: number; event: SchedulerEvent }
    | { kind: "task"; id: string; title: string; start_unix_ms: number; end_unix_ms: number };

  function blocksFor(day: number): { item: Block; column: number; columns: number }[] {
    const all: Block[] = [
      ...spansOnDay(events.filter((event) => !event.all_day), day).map((event): Block => ({ kind: "event", id: event.id, title: event.title, start_unix_ms: event.start_unix_ms, end_unix_ms: event.end_unix_ms, event })),
      ...spansOnDay(planned, day).map((slot): Block => ({ kind: "task", id: slot.task_id, title: slot.title, start_unix_ms: slot.start_unix_ms, end_unix_ms: slot.end_unix_ms })),
    ];
    return layoutOverlaps(all);
  }

  /** Ganztagstermine (aus verbundenen Kalendern) eines Tages; sie stehen über dem Raster. */
  const allDayFor = (day: number) => spansOnDay(events.filter((event) => event.all_day), day);
  const hasAllDay = $derived(days.some((day) => allDayFor(day).length > 0));

  /** Minuten seit Tagesbeginn, auf die sichtbare Spanne begrenzt. */
  function minutesIn(day: number, ms: number): number {
    const from = FIRST_HOUR * 60;
    const to = LAST_HOUR * 60;
    const raw = (ms - day) / 60_000;
    return Math.min(to, Math.max(from, raw)) - from;
  }

  function top(day: number, block: Block): number {
    return (minutesIn(day, block.start_unix_ms) / 60) * HOUR_PX;
  }
  function height(day: number, block: Block): number {
    const end = Math.min(block.end_unix_ms, addDays(day, 1));
    return Math.max(18, ((minutesIn(day, end) - minutesIn(day, block.start_unix_ms)) / 60) * HOUR_PX);
  }

  function clock(ms: number): string {
    return new Date(ms).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
  }

  function createAt(day: number, event: MouseEvent) {
    if (event.target !== event.currentTarget) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const minutes = FIRST_HOUR * 60 + Math.floor(((event.clientY - rect.top) / HOUR_PX) * 2) * 30;
    const start = new Date(day);
    start.setHours(0, minutes, 0, 0);
    onCreate(start.getTime());
  }

  const nowTop = $derived((minutesIn(new Date(now.ms).setHours(0, 0, 0, 0), now.ms) / 60) * HOUR_PX);
  const nowVisible = $derived(
    days.some((day) => isSameDay(day, now.ms)) && new Date(now.ms).getHours() >= FIRST_HOUR && new Date(now.ms).getHours() < LAST_HOUR,
  );
</script>

<div class="v-week" role="grid" aria-label={t("Wochenansicht")}>
  <div class="v-week-head" role="row">
    <span class="v-week-corner"></span>
    {#each days as day (day)}
      <span class="v-week-day" class:today={isSameDay(day, now.ms)} role="columnheader">
        <small>{new Date(day).toLocaleDateString(locale(), { weekday: "short" })}</small>
        <strong>{new Date(day).getDate()}</strong>
      </span>
    {/each}
  </div>
  {#if hasAllDay}
    <div class="v-week-allday" role="row" aria-label={t("Ganztägig")}>
      <span class="v-week-corner"><small>{t("Ganztägig")}</small></span>
      {#each days as day (day)}
        <span class="v-week-allday-cell" role="gridcell">
          {#each allDayFor(day) as event (event.id)}
            <button type="button" class="v-week-chip" style:border-left-color={event.remote?.color ?? null} title={event.title} onclick={() => onEvent(event)}>{event.title}</button>
          {/each}
        </span>
      {/each}
    </div>
  {/if}
  <div class="v-week-scroll">
    <div class="v-week-body" style:height="{hours.length * HOUR_PX}px">
      <div class="v-week-hours" aria-hidden="true">
        {#each hours as hour (hour)}<span style:height="{HOUR_PX}px">{String(hour).padStart(2, "0")}:00</span>{/each}
      </div>
      {#each days as day (day)}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="v-week-col" class:today={isSameDay(day, now.ms)} role="gridcell" tabindex="-1" onclick={(event) => createAt(day, event)}>
          {#each blocksFor(day) as placed (placed.item.kind + placed.item.id + placed.item.start_unix_ms)}
            {@const block = placed.item}
            <button
              type="button"
              class="v-week-block {block.kind}"
              class:remote={block.kind === "event" && !!block.event.remote}
              style:border-left-color={block.kind === "event" ? (block.event.remote?.color ?? null) : null}
              style:top="{top(day, block)}px"
              style:height="{height(day, block)}px"
              style:left="{(placed.column / placed.columns) * 100}%"
              style:width="{100 / placed.columns}%"
              title="{block.title} · {clock(block.start_unix_ms)} – {clock(block.end_unix_ms)}"
              onclick={() => (block.kind === "event" ? onEvent(block.event) : onTask(block.id))}
            >
              <strong>{block.title}</strong>
              <small>{clock(block.start_unix_ms)}</small>
            </button>
          {/each}
          {#if nowVisible && isSameDay(day, now.ms)}<span class="v-week-now" style:top="{nowTop}px" aria-hidden="true"></span>{/if}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .v-week { display: flex; flex-direction: column; min-height: 0; border: 1px solid var(--v-line); border-radius: var(--v-radius-card); overflow: hidden; }
  .v-week-head, .v-week-body { display: grid; grid-template-columns: 3.2rem repeat(7, minmax(0, 1fr)); }
  .v-week-head { border-bottom: 1px solid var(--v-line); background: var(--v-surface-1); }
  .v-week-day { display: flex; flex-direction: column; align-items: center; padding: var(--v-space-2) 0; color: var(--v-text-muted); }
  .v-week-day strong { color: var(--v-text-primary); font-size: var(--v-text-base); }
  .v-week-day.today strong { display: grid; place-items: center; width: 1.7rem; height: 1.7rem; border-radius: 50%; background: var(--v-accent-blue); color: var(--v-cta-text); }
  .v-week-allday { display: grid; grid-template-columns: 3.2rem repeat(7, minmax(0, 1fr)); border-bottom: 1px solid var(--v-line); background: var(--v-surface-1); }
  .v-week-allday .v-week-corner { display: flex; align-items: center; justify-content: flex-end; min-width: 0; padding-right: 4px; overflow: hidden; color: var(--v-text-muted); }
  .v-week-allday .v-week-corner small { font-size: 10px; letter-spacing: -.01em; white-space: nowrap; }
  .v-week-allday-cell { display: flex; flex-direction: column; gap: 2px; min-width: 0; padding: 3px 2px; border-left: 1px solid var(--v-line); }
  .v-week-chip { box-sizing: border-box; width: 100%; padding: 1px 5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; border: 0; border-left: 3px solid var(--v-accent-blue); border-radius: 4px; background: var(--v-surface-3); color: var(--v-text-primary); font-size: var(--v-text-xs); text-align: left; cursor: pointer; }
  .v-week-scroll { overflow: auto; max-height: min(60vh, 40rem); }
  .v-week-body { position: relative; }
  .v-week-hours { display: flex; flex-direction: column; }
  .v-week-hours span { box-sizing: border-box; padding: 2px 6px 0 0; text-align: right; color: var(--v-text-muted); font-size: var(--v-text-xs); border-top: 1px solid var(--v-line); }
  .v-week-col { position: relative; border-left: 1px solid var(--v-line); background-image: linear-gradient(to bottom, var(--v-line) 1px, transparent 1px); background-size: 100% 48px; cursor: cell; }
  .v-week-col.today { background-color: var(--v-accent-blue-soft); }
  .v-week-block { position: absolute; box-sizing: border-box; display: flex; flex-direction: column; align-items: flex-start; gap: 0; padding: 2px 5px; overflow: hidden; border: 1px solid var(--v-line-strong); border-left-width: 3px; border-radius: 6px; text-align: left; cursor: pointer; font-size: var(--v-text-xs); line-height: 1.25; }
  .v-week-block strong { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .v-week-block small { color: inherit; opacity: .8; }
  .v-week-block.event { background: var(--v-accent-blue-soft); border-color: var(--v-accent-blue); color: var(--v-text-primary); }
  .v-week-block.remote { background: var(--v-surface-3); }
  .v-week-block.task { background: var(--v-surface-3); border-color: var(--v-success); color: var(--v-text-primary); }
  .v-week-now { position: absolute; left: 0; right: 0; height: 2px; background: var(--v-danger); pointer-events: none; }
  @media (max-width: 720px) { .v-week-head, .v-week-body, .v-week-allday { grid-template-columns: 2.4rem repeat(7, minmax(0, 1fr)); } .v-week-block small { display: none; } }
</style>
