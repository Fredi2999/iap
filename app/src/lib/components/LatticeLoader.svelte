<script lang="ts">
  type Status = "working" | "done" | "error";
  type Grid = 3 | 4;
  type Shape = "round" | "square";
  type Pattern = {
    cells: Array<number | null>;
    loop?: number;
    scale?: number;
    lit?: number;
  };

  interface Props {
    status?: Status;
    label?: string;
    doneLabel?: string;
    errorLabel?: string;
    pattern?: string | Pattern;
    grid?: Grid;
    shape?: Shape;
    color?: string;
    doneColor?: string;
    errorColor?: string;
    cellSize?: number;
    gap?: number;
    fontSize?: number;
    step?: number;
    idleOpacity?: number;
    glow?: boolean;
    glowColor?: string;
    showTimer?: boolean;
    elapsed?: number;
    class?: string;
  }

  let {
    status = "working",
    label = "Denkt",
    doneLabel = "Fertig in",
    errorLabel = "Fehler nach",
    pattern = "orbit",
    grid = 3,
    shape = "round",
    color = "currentColor",
    doneColor = "#cbd5da",
    errorColor = "#e79090",
    cellSize = 6,
    gap = 2,
    fontSize = 13,
    step = 90,
    idleOpacity = 0.16,
    glow = false,
    glowColor = "",
    showTimer = true,
    elapsed,
    class: className = "",
  }: Props = $props();

  const patterns: Record<string, Partial<Record<Grid, Pattern>>> = {
    arrow: { 3: { cells: [1, 2, 3, 0, 1, 2, 1, 2, 3], loop: 7.2 } },
    dots: { 3: { cells: [0, 1, 2, 0, 1, 2, 0, 1, 2], loop: 3, scale: 2.4 } },
    ripple: { 3: { cells: [2, 1, 2, 1, 0, 1, 2, 1, 2], loop: 4.8, scale: 1.5 } },
    spiral: { 3: { cells: [0, 1, 2, 7, 8, 3, 6, 5, 4], loop: 9, scale: 1.2, lit: 0.35 } },
    orbit: {
      3: { cells: [0, 1, 2, 7, null, 3, 6, 5, 4], loop: 8, scale: 1.2 },
      4: { cells: [0, 1, 2, 3, 11, null, null, 4, 10, null, null, 5, 9, 8, 7, 6], loop: 12, scale: 1.2, lit: 0.45 },
    },
    snake: {
      3: { cells: [0, 1, 2, 5, 4, 3, 6, 7, 8], loop: 9, lit: 0.35 },
      4: { cells: [0, 1, 2, 3, 7, 6, 5, 4, 8, 9, 10, 11, 15, 14, 13, 12], loop: 16, lit: 0.25 },
    },
    sweep: { 4: { cells: [0, 1, 2, 3, 1, 2, 3, 4, 2, 3, 4, 5, 3, 4, 5, 6], loop: 5, lit: 0.45 } },
    spin: { 4: { cells: [0, 0, 1, 1, 0, 0, 1, 1, 3, 3, 2, 2, 3, 3, 2, 2], loop: 4, scale: 1.6, lit: 0.35 } },
    rain: { 4: { cells: [0, 2, 1, 3, 1, 3, 2, 4, 2, 4, 3, 5, 3, 5, 4, 6], loop: 4, scale: 1.2, lit: 0.35 } },
    pulse: { 4: { cells: [2, 1, 1, 2, 1, 0, 0, 1, 1, 0, 0, 1, 2, 1, 1, 2], loop: 2.4, scale: 2.5, lit: 0.45 } },
  };
  const marks: Record<Grid, Record<"done" | "error", number[]>> = {
    3: { done: [2, 3, 5, 7], error: [0, 2, 4, 6, 8] },
    4: { done: [7, 8, 10, 13], error: [0, 3, 5, 6, 9, 10, 12, 15] },
  };

  const activeGrid = $derived(grid === 4 ? 4 : 3);
  const activePattern = $derived.by((): Pattern => {
    const selected = typeof pattern === "string"
      ? patterns[pattern]?.[activeGrid] ?? patterns[activeGrid === 3 ? "orbit" : "sweep"][activeGrid]
      : pattern;
    const cells = Array.from({ length: activeGrid * activeGrid }, (_, i) => selected?.cells[i] ?? null);
    const max = Math.max(0, ...cells.filter((value): value is number => value !== null));
    return { cells, loop: selected?.loop ?? max + 4.2, scale: selected?.scale ?? 1, lit: selected?.lit ?? 0.62 };
  });
  const intervalStep = $derived(step * (activePattern.scale ?? 1));
  const cycle = $derived(Math.round((activePattern.loop ?? 8) * intervalStep));
  let tenths = $state(0);
  const shownTenths = $derived(elapsed === undefined ? tenths : Math.max(0, Math.round(elapsed * 10)));
  const timeText = $derived(shownTenths < 600
    ? `${(shownTenths / 10).toFixed(1)} s`
    : `${Math.floor(shownTenths / 600)} min ${((shownTenths % 600) / 10).toFixed(1)} s`);
  const announce = $derived(status === "working"
    ? `${label}, läuft`
    : `${status === "done" ? doneLabel : errorLabel}${showTimer ? ` ${timeText}` : ""}`);

  $effect(() => {
    if (elapsed !== undefined || status !== "working" || !showTimer) return;
    const started = performance.now();
    tenths = 0;
    const timer = window.setInterval(() => {
      tenths = Math.floor((performance.now() - started) / 100);
    }, 100);
    return () => window.clearInterval(timer);
  });
</script>

<span
  class={`lattice-loader ${className}`}
  role="status"
  data-status={status}
  data-shape={shape}
  data-glow={glow ? "" : undefined}
  style={`--ll-n:${activeGrid};--ll-cell:${cellSize}px;--ll-gap:${gap}px;--ll-font:${fontSize}px;--ll-color:${color};--ll-mark:${status === "error" ? errorColor : doneColor};--ll-idle:${idleOpacity};--ll-glow:${glowColor || color};--ll-cycle:${cycle}ms`}
>
  <span class="grid" aria-hidden="true">
    <span class="layer run">
      {#each activePattern.cells as unit, i (i)}
        <span
          class="cell"
          data-hole={unit === null ? "" : undefined}
          style={unit === null ? "" : `animation-delay:${Math.round(unit * intervalStep)}ms;--lit:${activePattern.lit ?? 0.62}`}
        ></span>
      {/each}
    </span>
    <span class="layer mark">
      {#each activePattern.cells as _, i (i)}
        <span class="cell" data-on={marks[activeGrid][status === "error" ? "error" : "done"].includes(i) ? "" : undefined}></span>
      {/each}
    </span>
  </span>
  <span class="visible-label" aria-hidden="true">{status === "working" ? label : status === "done" ? doneLabel : errorLabel}</span>
  {#if showTimer}<span class="timer" aria-hidden="true">{timeText}</span>{/if}
  <span class="sr-only">{announce}</span>
</span>

<style>
  .lattice-loader { display: inline-flex; align-items: center; gap: calc(var(--ll-font) * .62); color: var(--ll-color); font-size: var(--ll-font); line-height: 1; white-space: nowrap; }
  .grid { display: grid; flex: none; }
  .layer { grid-area: 1 / 1; display: grid; grid-template-columns: repeat(var(--ll-n), var(--ll-cell)); gap: var(--ll-gap); }
  .cell { width: var(--ll-cell); height: var(--ll-cell); border-radius: max(1px, calc(var(--ll-cell) * .25)); background: currentColor; opacity: var(--ll-idle); }
  .lattice-loader[data-shape="round"] .cell { border-radius: 50%; }
  .run .cell { animation: lattice-on var(--ll-cycle) cubic-bezier(.77,0,.175,1) infinite; }
  .run .cell[data-hole] { animation: none; opacity: calc(var(--ll-idle) * .47); }
  .lattice-loader[data-glow] .run .cell:not([data-hole]) { box-shadow: 0 0 calc(var(--ll-cell) * 1.2) var(--ll-glow); }
  .mark { opacity: 0; transform: scale(.9); transition: opacity 180ms ease, transform 180ms cubic-bezier(.23,1,.32,1); }
  .mark .cell[data-on] { background: var(--ll-mark); opacity: 1; }
  .lattice-loader:not([data-status="working"]) .run { opacity: 0; }
  .lattice-loader:not([data-status="working"]) .run .cell { animation-play-state: paused; }
  .lattice-loader:not([data-status="working"]) .mark { opacity: 1; transform: none; }
  .visible-label { font-weight: 500; }
  .timer { font-family: var(--v-font-mono, monospace); font-size: .875em; font-variant-numeric: tabular-nums; opacity: .7; }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; border: 0; }
  @keyframes lattice-on { 0%, 100% { opacity: var(--ll-idle); } 18%, 42% { opacity: 1; } 62% { opacity: var(--ll-idle); } }
  @media (prefers-reduced-motion: reduce) {
    .run .cell { animation-duration: 1.4s; animation-delay: 0ms !important; }
    .mark { transform: none; }
  }
</style>
