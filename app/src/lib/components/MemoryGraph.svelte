<script lang="ts">
  import { t } from "../i18n/index.svelte";
  import type { MemoryGraph, GraphNode } from "../types";
  import { layoutGraph, neighborhood } from "../graphLayout";

  interface Props {
    graph: MemoryGraph;
    /** Suchtext: passende Knoten und ihre Nachbarn bleiben hell, der Rest wird gedimmt. */
    query?: string;
    selectedId?: string | null;
    onselect?: (node: GraphNode | null) => void;
  }
  let { graph, query = "", selectedId = null, onselect }: Props = $props();

  const W = 900;
  const H = 600;
  let positions = $derived(layoutGraph(graph.nodes.map((n) => ({ id: n.id })), graph.edges, { width: W, height: H, margin: 70 }));

  // Ansicht: Verschieben und Zoomen über einen eigenen viewBox-Ausschnitt.
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let drag: { x: number; y: number; px: number; py: number } | null = null;
  let svg: SVGSVGElement | undefined = $state();

  let matches = $derived.by(() => {
    const q = query.trim().toLocaleLowerCase();
    if (!q) return null;
    const ids = new Set(graph.nodes.filter((n) => n.label.toLocaleLowerCase().includes(q)).map((n) => n.id));
    return neighborhood(graph.edges, ids);
  });

  function radius(node: GraphNode): number {
    if (node.kind === "category") return 15;
    if (node.kind === "ghost") return 8;
    return 8 + Math.min(8, Math.sqrt(node.access_count) * 2);
  }

  function clamp(value: number, lo: number, hi: number) { return Math.min(hi, Math.max(lo, value)); }
  function zoomBy(factor: number) { zoom = clamp(zoom * factor, 0.4, 4); }
  function reset() { zoom = 1; panX = 0; panY = 0; }

  function onWheel(event: WheelEvent) {
    event.preventDefault();
    zoomBy(event.deltaY < 0 ? 1.12 : 1 / 1.12);
  }
  function down(event: PointerEvent) {
    if ((event.target as Element).closest("[data-node]")) return;
    drag = { x: event.clientX, y: event.clientY, px: panX, py: panY };
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
  }
  function move(event: PointerEvent) {
    if (!drag || !svg) return;
    const scale = (W / zoom) / svg.getBoundingClientRect().width;
    panX = drag.px - (event.clientX - drag.x) * scale;
    panY = drag.py - (event.clientY - drag.y) * scale;
  }
  function up() { drag = null; }

  function pick(node: GraphNode) { onselect?.(node.id === selectedId ? null : node); }
  function key(event: KeyboardEvent, node: GraphNode) {
    if (event.key === "Enter" || event.key === " ") { event.preventDefault(); pick(node); }
  }
  function onSvgKey(event: KeyboardEvent) {
    const step = 40 / zoom;
    if (event.key === "ArrowLeft") panX -= step;
    else if (event.key === "ArrowRight") panX += step;
    else if (event.key === "ArrowUp") panY -= step;
    else if (event.key === "ArrowDown") panY += step;
    else if (event.key === "+" || event.key === "=") zoomBy(1.2);
    else if (event.key === "-") zoomBy(1 / 1.2);
    else if (event.key === "0") reset();
    else return;
    event.preventDefault();
  }

  function shortLabel(label: string): string {
    return label.length > 26 ? label.slice(0, 25) + "…" : label;
  }
  let viewBox = $derived(`${panX + (W - W / zoom) / 2} ${panY + (H - H / zoom) / 2} ${W / zoom} ${H / zoom}`);
</script>

<div class="v-graph">
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <svg bind:this={svg} {viewBox} role="group" tabindex="0" aria-label={t("Gedächtnis-Graph. Pfeiltasten verschieben, Plus und Minus zoomen, Null setzt zurück.")}
    onwheel={onWheel} onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onkeydown={onSvgKey}>
    {#each graph.edges as edge, i (i)}
      {@const a = positions.get(edge.from)}
      {@const b = positions.get(edge.to)}
      {#if a && b}
        <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} class="edge {edge.kind}" class:dim={matches && !(matches.has(edge.from) && matches.has(edge.to))} />
      {/if}
    {/each}
    {#each graph.nodes as node (node.id)}
      {@const p = positions.get(node.id)}
      {#if p}
        {@const r = radius(node)}
        <g data-node transform={`translate(${p.x} ${p.y})`} class="node {node.kind}" class:selected={node.id === selectedId}
          class:dim={matches && !matches.has(node.id)} class:old={node.superseded} class:verified={node.verified}
          role="button" tabindex="0" aria-pressed={node.id === selectedId} aria-label={node.label}
          onclick={() => pick(node)} onkeydown={(e) => key(e, node)}>
          {#if node.kind === "category"}
            <rect x={-r} y={-r} width={r * 2} height={r * 2} rx="4" />
          {:else}
            <circle {r} />
          {/if}
          <text y={r + 13} text-anchor="middle">{shortLabel(node.label)}</text>
        </g>
      {/if}
    {/each}
  </svg>
  <div class="v-graph-tools">
    <button type="button" class="v-btn-icon" onclick={() => zoomBy(1.2)} aria-label={t("Vergrößern")} title={t("Vergrößern")}>+</button>
    <button type="button" class="v-btn-icon" onclick={() => zoomBy(1 / 1.2)} aria-label={t("Verkleinern")} title={t("Verkleinern")}>−</button>
    <button type="button" class="v-btn-icon" onclick={reset} aria-label={t("Ansicht zurücksetzen")} title={t("Ansicht zurücksetzen")}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 12a8 8 0 1 0 3-6.2M4 4v5h5"/></svg>
    </button>
  </div>
  <ul class="v-graph-legend v-help" aria-label={t("Legende")}>
    <li><span class="sw fact"></span>{t("Fakt")}</li>
    <li><span class="sw verified"></span>{t("Bestätigt")}</li>
    <li><span class="sw learned"></span>{t("Automatisch gelernt")}</li>
    <li><span class="sw category"></span>{t("Kategorie")}</li>
    <li><span class="sw ghost"></span>{t("Link ohne Fakt")}</li>
  </ul>
</div>

<style>
  .v-graph { position: relative; border: 1px solid var(--v-line); border-radius: var(--v-radius-control); background: var(--v-surface-solid); overflow: hidden; }
  .v-graph > svg { display: block; width: 100%; height: min(60vh, 560px); min-height: 320px; touch-action: none; cursor: grab; }
  .v-graph > svg:active { cursor: grabbing; }
  .v-graph > svg:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: -2px; }
  .edge { stroke: var(--v-line-strong); stroke-width: 1.2; }
  .edge.category { stroke-dasharray: 2 4; opacity: .6; }
  .edge.supersedes { stroke: var(--v-warning); stroke-dasharray: 6 4; }
  .edge.link { stroke: var(--v-accent-blue); stroke-width: 1.6; }
  .dim { opacity: .14; }
  .node { cursor: pointer; }
  .node circle, .node rect { fill: var(--v-accent-blue-soft); stroke: var(--v-accent-blue); stroke-width: 1.6; }
  .node.verified circle { fill: var(--v-accent-blue); }
  .node.fact:not(.verified) circle { stroke: var(--v-warning); fill: transparent; }
  .node.category rect { fill: var(--v-line); stroke: var(--v-text-muted); }
  .node.ghost circle { fill: transparent; stroke: var(--v-text-muted); stroke-dasharray: 3 3; }
  .node.old { opacity: .5; }
  .node.selected circle, .node.selected rect { stroke: var(--v-text-primary); stroke-width: 3; }
  .node:focus-visible { outline: none; }
  .node:focus-visible circle, .node:focus-visible rect { stroke: var(--v-focus-ring); stroke-width: 3.5; }
  text { fill: var(--v-text-secondary); font-size: 11px; pointer-events: none; paint-order: stroke; stroke: var(--v-surface-solid); stroke-width: 3px; }
  .v-graph-tools { position: absolute; top: var(--v-space-2); right: var(--v-space-2); display: flex; flex-direction: column; gap: 4px; }
  .v-graph-legend { position: absolute; left: var(--v-space-2); bottom: var(--v-space-2); display: flex; flex-wrap: wrap; gap: 4px var(--v-space-3); margin: 0; padding: 4px 8px; list-style: none; border-radius: var(--v-radius-control); background: color-mix(in oklab, var(--v-surface-solid) 85%, transparent); }
  .v-graph-legend li { display: inline-flex; align-items: center; gap: 5px; }
  .sw { width: 10px; height: 10px; border: 1.5px solid var(--v-accent-blue); border-radius: 50%; display: inline-block; }
  .sw.verified { background: var(--v-accent-blue); }
  .sw.learned { border-color: var(--v-warning); }
  .sw.category { border-color: var(--v-text-muted); border-radius: 2px; background: var(--v-line); }
  .sw.ghost { border-color: var(--v-text-muted); border-style: dashed; }
</style>
