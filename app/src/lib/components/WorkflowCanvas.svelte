<script lang="ts">
  import { t } from "../i18n/index.svelte";
  import type { GraphProblem, WorkflowGraph, WorkflowNode } from "../types";
  import {
    HEADER_HEIGHT, NODE_GROUPS, NODE_TYPES, NODE_WIDTH, ROW_HEIGHT, BODY_PADDING,
    connectProblem, inputPorts, newId, newNode, nodeHeight, nodeSummary, nodeTitle, outputPorts, portY,
    type NodeType,
  } from "../workflow";

  // Zeichenfläche des Workflow-Builders. Alles läuft über eigene Pointer-Events
  // (wie RecordButton und Toast), ohne zusätzliche Bibliothek:
  //  - Knoten aus der Palette ziehen oder anklicken, Knoten am Kopf verschieben;
  //  - Anschlüsse verbinden: vom Ausgang (rechts) zum Eingang (links) ziehen;
  //  - Tastatur: Knoten fokussieren, Pfeiltasten verschieben, Entf löscht,
  //    Verbindungen lassen sich zusätzlich im Inspektor per Auswahllisten anlegen.
  // Die maßgebliche Prüfung macht das Backend; hier wird nur Offensichtliches verhindert.
  export type Selection = { kind: "node" | "edge"; id: string } | null;

  interface Props {
    graph: WorkflowGraph;
    selection: Selection;
    problems: GraphProblem[];
    /** Knoten, der gerade läuft. */
    activeNode: string | null;
    /** Knoten, die im laufenden Lauf schon fertig sind. */
    doneNodes: string[];
    /** Während eines Laufs ist die Fläche gesperrt. */
    locked: boolean;
    onChange: () => void;
    onMessage: (text: string) => void;
  }
  let { graph = $bindable(), selection = $bindable(), problems, activeNode, doneNodes, locked, onChange, onMessage }: Props = $props();

  let surface = $state<HTMLDivElement | null>(null);
  let zoom = $state(1);
  // Hintergrund ziehen = Ansicht verschieben.
  let pan: { x: number; y: number; left: number; top: number; moved: boolean } | null = null;
  let scroller = $state<HTMLDivElement | null>(null);
  let paletteDrag = $state<{ type: NodeType; x: number; y: number; moved: boolean; startX: number; startY: number } | null>(null);
  let nodeDrag: { id: string; dx: number; dy: number } | null = null;
  let draft = $state<{ from: string; fromPort: string; x: number; y: number } | null>(null);
  // Nach einem Ziehen aus der Palette folgt noch ein click; er darf keinen zweiten Knoten anlegen.
  let justDragged = false;

  const width = $derived(Math.max(1300, ...graph.nodes.map((n) => n.x + NODE_WIDTH + 80)));
  const height = $derived(Math.max(560, ...graph.nodes.map((n) => n.y + nodeHeight(n) + 80)));
  // Die Fläche wächst mit dem Inhalt, bleibt aber in einem sinnvollen Rahmen.
  const viewHeight = $derived(Math.round(Math.min(680, Math.max(300, height * zoom))));
  const problemNodes = $derived(new Set(problems.map((p) => p.node_id).filter((id): id is string => !!id)));
  const problemEdges = $derived(new Set(problems.map((p) => p.edge_id).filter((id): id is string => !!id)));

  // Der Rechteck-Abstand kommt skaliert an; Knotenpositionen sind unskaliert.
  function local(event: PointerEvent): { x: number; y: number } {
    const rect = surface?.getBoundingClientRect();
    return { x: (event.clientX - (rect?.left ?? 0)) / zoom, y: (event.clientY - (rect?.top ?? 0)) / zoom };
  }

  const clampZoom = (value: number) => Math.min(1.6, Math.max(0.3, Math.round(value * 100) / 100));

  export function setZoom(value: number) {
    zoom = clampZoom(value);
  }

  /** Passt die Ansicht so an, dass alle Knoten sichtbar sind. */
  export function fit() {
    if (!scroller || graph.nodes.length === 0) { zoom = 1; return; }
    const right = Math.max(...graph.nodes.map((n) => n.x + NODE_WIDTH)) + 40;
    const bottom = Math.max(...graph.nodes.map((n) => n.y + nodeHeight(n))) + 40;
    zoom = clampZoom(Math.min(1, scroller.clientWidth / right, scroller.clientHeight / bottom));
    scroller.scrollTo({ left: 0, top: 0 });
  }

  function wheel(event: WheelEvent) {
    if (!event.ctrlKey) return;
    event.preventDefault();
    zoom = clampZoom(zoom * (event.deltaY < 0 ? 1.1 : 1 / 1.1));
  }

  function backgroundDown(event: PointerEvent) {
    const target = event.target as HTMLElement;
    if (event.button !== 0 || !scroller || target.closest(".v-wf-node, .v-wf-edge-hit")) return;
    pan = { x: event.clientX, y: event.clientY, left: scroller.scrollLeft, top: scroller.scrollTop, moved: false };
    scroller.setPointerCapture(event.pointerId);
  }
  function backgroundMove(event: PointerEvent) {
    if (!pan || !scroller) return;
    const dx = event.clientX - pan.x;
    const dy = event.clientY - pan.y;
    if (Math.abs(dx) + Math.abs(dy) > 3) pan.moved = true;
    scroller.scrollLeft = pan.left - dx;
    scroller.scrollTop = pan.top - dy;
  }
  function backgroundUp() {
    if (pan && !pan.moved) selection = null;
    pan = null;
  }

  // --- Palette ---
  function freeSpot(): { x: number; y: number } {
    let x = 30;
    let y = 30;
    while (graph.nodes.some((n) => Math.abs(n.x - x) < 40 && Math.abs(n.y - y) < 40)) {
      x += 36;
      y += 36;
    }
    return { x, y };
  }

  function addNode(type: NodeType, x?: number, y?: number) {
    if (locked) return;
    if (type === "manual_start" && graph.nodes.some((n) => n.kind.type === "manual_start")) {
      onMessage(t("Es darf nur einen manuellen Start geben."));
      return;
    }
    const spot = x === undefined || y === undefined ? freeSpot() : { x: x - NODE_WIDTH / 2, y: y - HEADER_HEIGHT / 2 };
    const node = newNode(type, spot.x, spot.y);
    graph.nodes.push(node);
    selection = { kind: "node", id: node.id };
    onChange();
  }

  function paletteDown(event: PointerEvent, type: NodeType) {
    if (event.button !== 0 || locked) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    justDragged = false;
    paletteDrag = { type, x: event.clientX, y: event.clientY, moved: false, startX: event.clientX, startY: event.clientY };
  }
  function paletteMove(event: PointerEvent) {
    if (!paletteDrag) return;
    paletteDrag.x = event.clientX;
    paletteDrag.y = event.clientY;
    if (Math.hypot(event.clientX - paletteDrag.startX, event.clientY - paletteDrag.startY) > 6) paletteDrag.moved = true;
  }
  function paletteUp(event: PointerEvent) {
    if (!paletteDrag) return;
    const { type, moved } = paletteDrag;
    paletteDrag = null;
    if (!moved) return; // Ein Klick wird über onclick behandelt (auch per Tastatur).
    justDragged = true;
    const area = scroller?.getBoundingClientRect();
    if (area && event.clientX >= area.left && event.clientX <= area.right && event.clientY >= area.top && event.clientY <= area.bottom) {
      const point = local(event);
      addNode(type, point.x, point.y);
    }
  }

  // --- Knoten verschieben ---
  function nodeDown(event: PointerEvent, node: WorkflowNode) {
    if (event.button !== 0) return;
    selection = { kind: "node", id: node.id };
    if (locked) return;
    const point = local(event);
    nodeDrag = { id: node.id, dx: point.x - node.x, dy: point.y - node.y };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }
  function nodeMove(event: PointerEvent) {
    if (!nodeDrag) return;
    const node = graph.nodes.find((n) => n.id === nodeDrag?.id);
    if (!node) return;
    const point = local(event);
    node.x = Math.max(0, Math.round(point.x - nodeDrag.dx));
    node.y = Math.max(0, Math.round(point.y - nodeDrag.dy));
  }
  function nodeUp() {
    if (nodeDrag) onChange();
    nodeDrag = null;
  }

  // --- Verbinden ---
  function outDown(event: PointerEvent, node: WorkflowNode, port: string) {
    if (event.button !== 0 || locked) return;
    event.stopPropagation();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    const point = local(event);
    draft = { from: node.id, fromPort: port, x: point.x, y: point.y };
  }
  function outMove(event: PointerEvent) {
    if (!draft) return;
    const point = local(event);
    draft.x = point.x;
    draft.y = point.y;
  }
  function outUp(event: PointerEvent) {
    if (!draft) return;
    const start = draft;
    draft = null;
    const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>("[data-in-port]");
    if (!target) return;
    connect(start.from, start.fromPort, target.dataset.node ?? "", target.dataset.inPort ?? "");
  }

  export function connect(from: string, fromPort: string, to: string, toPort: string) {
    const problem = connectProblem(graph, from, fromPort, to, toPort);
    if (problem) {
      onMessage(t(problem));
      return;
    }
    const edge = { id: newId("e"), from, from_port: fromPort, to, to_port: toPort };
    graph.edges.push(edge);
    selection = { kind: "edge", id: edge.id };
    onChange();
  }

  export function removeSelection() {
    if (locked || !selection) return;
    if (selection.kind === "node") {
      const id = selection.id;
      graph.nodes = graph.nodes.filter((n) => n.id !== id);
      graph.edges = graph.edges.filter((e) => e.from !== id && e.to !== id);
    } else {
      const id = selection.id;
      graph.edges = graph.edges.filter((e) => e.id !== id);
    }
    selection = null;
    onChange();
  }

  function keyDown(event: KeyboardEvent) {
    const target = event.target as HTMLElement;
    if (target.closest("input, textarea, select")) return;
    if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      removeSelection();
      return;
    }
    if (selection?.kind === "node" && !locked && event.key.startsWith("Arrow")) {
      const node = graph.nodes.find((n) => n.id === selection?.id);
      if (!node) return;
      event.preventDefault();
      const step = event.shiftKey ? 40 : 10;
      if (event.key === "ArrowLeft") node.x = Math.max(0, node.x - step);
      if (event.key === "ArrowRight") node.x += step;
      if (event.key === "ArrowUp") node.y = Math.max(0, node.y - step);
      if (event.key === "ArrowDown") node.y += step;
      onChange();
    }
  }

  /** Verbindet Punkte mit rechten Winkeln und abgerundeten Ecken. */
  function rounded(points: [number, number][], radius = 10): string {
    let d = `M ${points[0][0]} ${points[0][1]}`;
    for (let i = 1; i < points.length - 1; i++) {
      const [px, py] = points[i - 1];
      const [cx, cy] = points[i];
      const [nx, ny] = points[i + 1];
      const r = Math.min(radius, Math.hypot(cx - px, cy - py) / 2, Math.hypot(nx - cx, ny - cy) / 2);
      const ux = Math.sign(cx - px);
      const uy = Math.sign(cy - py);
      const vx = Math.sign(nx - cx);
      const vy = Math.sign(ny - cy);
      d += ` L ${cx - ux * r} ${cy - uy * r} Q ${cx} ${cy} ${cx + vx * r} ${cy + vy * r}`;
    }
    const last = points[points.length - 1];
    return `${d} L ${last[0]} ${last[1]}`;
  }

  function edgePath(edge: { from: string; from_port: string; to: string; to_port: string }): string {
    const a = graph.nodes.find((n) => n.id === edge.from);
    const b = graph.nodes.find((n) => n.id === edge.to);
    if (!a || !b) return "";
    const x1 = a.x + NODE_WIDTH;
    const y1 = portY(a, "out", edge.from_port);
    const x2 = b.x;
    const y2 = portY(b, "in", edge.to_port);
    if (x2 < x1 + 30) {
      // Rückwärts oder in derselben Spalte: rechtwinklig in der Lücke zwischen den Knoten
      // oder, wenn es keine gibt, unter allen beteiligten Knoten herum.
      const aBottom = a.y + nodeHeight(a);
      const bBottom = b.y + nodeHeight(b);
      let lane: number;
      if (b.y >= aBottom + 12) lane = (aBottom + b.y) / 2;
      else if (a.y >= bBottom + 12) lane = (bBottom + a.y) / 2;
      else lane = Math.max(aBottom, bBottom) + 36;
      return rounded([[x1, y1], [x1 + 26, y1], [x1 + 26, lane], [x2 - 26, lane], [x2 - 26, y2], [x2, y2]]);
    }
    const bend = Math.max(60, Math.abs(x2 - x1) / 2);
    return `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`;
  }

  function draftPath(): string {
    if (!draft) return "";
    const a = graph.nodes.find((n) => n.id === draft?.from);
    if (!a) return "";
    const x1 = a.x + NODE_WIDTH;
    const y1 = portY(a, "out", draft.fromPort);
    const bend = Math.max(60, Math.abs(draft.x - x1) / 2);
    return `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${draft.x - bend} ${draft.y}, ${draft.x} ${draft.y}`;
  }

  function acceptable(node: WorkflowNode, port: string): boolean {
    return !!draft && connectProblem(graph, draft.from, draft.fromPort, node.id, port) === null;
  }
</script>

<div class="v-wf">
  <div class="v-wf-palette" role="toolbar" aria-label={t("Knoten hinzufügen")}>
    {#each NODE_GROUPS as group (group.id)}
      <div class="v-wf-group" role="group" aria-label={t(group.title)}>
        <span class="v-wf-palette-label">{t(group.title)}</span>
        {#each NODE_TYPES.filter((entry) => entry.group === group.id) as entry (entry.type)}
          <button
            type="button"
            class="v-wf-chip"
            disabled={locked}
            title={t(entry.hint)}
            onpointerdown={(e) => paletteDown(e, entry.type)}
            onpointermove={paletteMove}
            onpointerup={paletteUp}
            onpointercancel={() => { paletteDrag = null; }}
            onclick={() => { if (justDragged) { justDragged = false; return; } addNode(entry.type); }}
          >{t(entry.title)}</button>
        {/each}
      </div>
    {/each}
    <span class="v-wf-palette-hint">{t("Ziehen oder anklicken. Anschlüsse: vom rechten Kreis zum linken Kreis ziehen.")}</span>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div class="v-wf-frame">
  <div class="v-wf-scroll" bind:this={scroller} onkeydown={keyDown} onwheel={wheel} onpointerdown={backgroundDown} onpointermove={backgroundMove} onpointerup={backgroundUp} onpointercancel={backgroundUp} role="application" aria-label={t("Zeichenfläche des Workflows")} tabindex="0" style={`height:${viewHeight}px`}>
   <div class="v-wf-sizer" style={`width:${width * zoom}px;height:${height * zoom}px`}>
    <div class="v-wf-surface" bind:this={surface} style={`width:${width}px;height:${height}px;transform:scale(${zoom})`}>
      <svg class="v-wf-edges" {width} {height} aria-hidden="true">
        {#each graph.edges as edge (edge.id)}
          <path d={edgePath(edge)} class="v-wf-edge" class:selected={selection?.kind === "edge" && selection.id === edge.id} class:problem={problemEdges.has(edge.id)} />
          <path d={edgePath(edge)} class="v-wf-edge-hit" role="presentation" onclick={() => { selection = { kind: "edge", id: edge.id }; }} />
        {/each}
        {#if draft}<path d={draftPath()} class="v-wf-edge draft" />{/if}
      </svg>

      {#each graph.nodes as node (node.id)}
        {@const ins = inputPorts(node.kind)}
        {@const outs = outputPorts(node.kind)}
        <div
          class="v-wf-node"
          class:selected={selection?.kind === "node" && selection.id === node.id}
          class:active={activeNode === node.id}
          class:done={doneNodes.includes(node.id)}
          class:problem={problemNodes.has(node.id)}
          style={`left:${node.x}px;top:${node.y}px;width:${NODE_WIDTH}px;height:${nodeHeight(node)}px`}
          role="button"
          tabindex="0"
          aria-label={`${t(nodeTitle(node.kind))}`}
          aria-pressed={selection?.kind === "node" && selection.id === node.id}
          onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); selection = { kind: "node", id: node.id }; } }}
        >
          <div class="v-wf-head" style={`height:${HEADER_HEIGHT}px`} role="presentation" onpointerdown={(e) => nodeDown(e, node)} onpointermove={nodeMove} onpointerup={nodeUp} onpointercancel={nodeUp}>
            <span>{t(nodeTitle(node.kind))}</span>
            {#if doneNodes.includes(node.id) && activeNode !== node.id}<span class="v-wf-tick" aria-label={t("fertig")}>✓</span>{/if}
          </div>
          <div class="v-wf-body" style={`padding:${BODY_PADDING}px 0`}>
            {#each Array.from({ length: Math.max(ins.length, outs.length, 1) }) as _row, index}
              <div class="v-wf-row" style={`height:${ROW_HEIGHT}px`}>
                {#if ins[index]}
                  <button type="button" class="v-wf-port in" class:ok={acceptable(node, ins[index].name)} data-in-port={ins[index].name} data-node={node.id} title={`${t(ins[index].label)} (${ins[index].kind})`} aria-label={`${t("Eingang")}: ${t(ins[index].label)}`} tabindex="-1"></button>
                  <span class="v-wf-port-label">{t(ins[index].label)}</span>
                {:else}<span class="v-wf-port-label"></span>{/if}
                {#if outs[index]}
                  <span class="v-wf-port-label out">{t(outs[index].label)}</span>
                  <button type="button" class="v-wf-port out" title={`${t(outs[index].label)} (${outs[index].kind})`} aria-label={`${t("Ausgang")}: ${t(outs[index].label)}`} tabindex="-1"
                    onpointerdown={(e) => outDown(e, node, outs[index].name)} onpointermove={outMove} onpointerup={outUp} onpointercancel={() => { draft = null; }}></button>
                {/if}
              </div>
            {/each}
          </div>
          <div class="v-wf-foot">{nodeSummary(node)}</div>
        </div>
      {/each}
    </div>
   </div>
  </div>
  <div class="v-wf-zoom" role="group" aria-label={t("Ansicht")}>
    <button type="button" class="v-wf-zbtn" onclick={() => setZoom(zoom / 1.2)} aria-label={t("Verkleinern")}>−</button>
    <output class="v-num" aria-live="polite">{Math.round(zoom * 100)} %</output>
    <button type="button" class="v-wf-zbtn" onclick={() => setZoom(zoom * 1.2)} aria-label={t("Vergrößern")}>+</button>
    <button type="button" class="v-wf-zbtn wide" onclick={fit}>{t("Alles zeigen")}</button>
  </div>
</div>

  {#if paletteDrag?.moved}
    <div class="v-wf-ghost" style={`left:${paletteDrag.x + 8}px;top:${paletteDrag.y + 8}px`}>{t(NODE_TYPES.find((n) => n.type === paletteDrag?.type)?.title ?? "")}</div>
  {/if}
</div>

<style>
  .v-wf { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--v-space-2); min-width: 0; }
  .v-wf-palette { display: flex; flex-wrap: wrap; align-items: center; gap: var(--v-space-2); }
  .v-wf-group { display: flex; flex-wrap: wrap; align-items: center; gap: var(--v-space-1); padding-right: var(--v-space-3); }
  .v-wf-palette-label { color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .v-wf-palette-hint { color: var(--v-text-muted); font-size: var(--v-text-xs); flex-basis: 100%; }
  .v-wf-chip { min-height: 2rem; padding: 0 .75rem; border: 1px solid var(--v-line); border-radius: 9999px; background: var(--v-surface-2); color: var(--v-text-secondary); font: inherit; font-size: var(--v-text-sm); cursor: grab; touch-action: none; user-select: none; }
  .v-wf-chip:hover:not(:disabled) { border-color: var(--v-line-strong); color: var(--v-text-primary); }
  .v-wf-chip:focus-visible { outline: none; box-shadow: var(--v-shadow-focus); }
  .v-wf-chip:disabled { opacity: .45; cursor: not-allowed; }
  .v-wf-frame { position: relative; min-width: 0; }
  .v-wf-sizer { position: relative; }
  .v-wf-zoom { position: absolute; right: 12px; bottom: 12px; display: inline-flex; align-items: center; gap: 4px; padding: 4px; border: 1px solid var(--v-line); border-radius: 9999px; background: var(--v-surface-2); box-shadow: 0 2px 10px rgb(0 0 0 / .25); }
  .v-wf-zoom output { min-width: 3rem; text-align: center; color: var(--v-text-secondary); font-size: var(--v-text-xs); }
  .v-wf-zbtn { min-width: 1.9rem; height: 1.9rem; padding: 0 .55rem; border: 0; border-radius: 9999px; background: transparent; color: var(--v-text-secondary); font: inherit; font-size: var(--v-text-sm); cursor: pointer; }
  .v-wf-zbtn.wide { font-size: var(--v-text-xs); }
  .v-wf-zbtn:hover { background: rgb(var(--v-tint) / .1); color: var(--v-text-primary); }
  .v-wf-zbtn:focus-visible { outline: none; box-shadow: var(--v-shadow-focus); }
  .v-wf-scroll { overflow: auto; cursor: grab; border: 1px solid var(--v-line); border-radius: var(--v-radius-card); background: rgb(var(--v-tint) / .03); background-image: radial-gradient(rgb(var(--v-tint) / .08) 1px, transparent 1px); background-size: 24px 24px; }
  .v-wf-scroll:focus-visible { outline: none; box-shadow: var(--v-shadow-focus); }
  .v-wf-surface { position: absolute; left: 0; top: 0; transform-origin: 0 0; }
  .v-wf-edges { position: absolute; inset: 0; pointer-events: none; }
  .v-wf-edge { fill: none; stroke: var(--v-text-muted); stroke-width: 2; }
  .v-wf-edge.selected { stroke: var(--v-accent-blue); stroke-width: 3; }
  .v-wf-edge.problem { stroke: var(--v-danger); }
  .v-wf-edge.draft { stroke: var(--v-accent-blue); stroke-dasharray: 5 4; }
  .v-wf-edge-hit { fill: none; stroke: transparent; stroke-width: 16; pointer-events: stroke; cursor: pointer; }
  .v-wf-node { position: absolute; display: flex; flex-direction: column; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-field); background: var(--v-surface-2); box-shadow: 0 2px 10px rgb(0 0 0 / .18); }
  .v-wf-node:focus-visible { outline: none; box-shadow: var(--v-shadow-focus); }
  .v-wf-node.selected { border-color: var(--v-accent-blue); }
  .v-wf-node.problem { border-color: var(--v-danger); }
  .v-wf-node.active { border-color: var(--v-accent-blue); box-shadow: 0 0 0 3px color-mix(in oklab, var(--v-accent-blue) 30%, transparent); }
  .v-wf-head { display: flex; align-items: center; justify-content: space-between; padding: 0 var(--v-space-3); border-bottom: 1px solid var(--v-line); color: var(--v-text-primary); font-size: var(--v-text-sm); font-weight: 600; cursor: grab; touch-action: none; user-select: none; }
  .v-wf-tick { color: var(--v-accent-blue); }
  .v-wf-body { flex: none; }
  .v-wf-row { display: flex; align-items: center; justify-content: space-between; gap: 6px; position: relative; }
  .v-wf-port-label { flex: 1; min-width: 0; padding: 0 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--v-text-secondary); font-size: var(--v-text-xs); }
  .v-wf-port-label.out { text-align: right; }
  .v-wf-port { position: absolute; width: 14px; height: 14px; padding: 0; border: 2px solid var(--v-text-muted); border-radius: 50%; background: var(--v-surface-2); cursor: crosshair; touch-action: none; }
  .v-wf-port.in { left: -8px; }
  .v-wf-port.out { right: -8px; }
  .v-wf-port:hover, .v-wf-port.ok { border-color: var(--v-accent-blue); background: var(--v-accent-blue); }
  .v-wf-foot { margin-top: auto; padding: 0 var(--v-space-3) 4px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .v-wf-ghost { position: fixed; z-index: 50; pointer-events: none; padding: 4px 10px; border: 1px solid var(--v-accent-blue); border-radius: 9999px; background: var(--v-surface-2); color: var(--v-text-primary); font-size: var(--v-text-sm); }
  @media (prefers-reduced-motion: reduce) { .v-wf-node, .v-wf-chip { transition: none; } }
</style>
