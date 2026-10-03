// Kraftlayout für den Gedächtnis-Graphen, von Hand geschrieben (keine Abhängigkeit).
// Bewusst deterministisch und ohne Animation: Startpositionen kommen aus dem Index, nicht aus
// Math.random, und es läuft eine feste Zahl Schritte. Das Ergebnis ist reproduzierbar, testbar
// und braucht keine laufende Schleife (schont CPU auf 8-GB-Rechnern, respektiert reduzierte Bewegung).

export interface LayoutNode { id: string; weight?: number }
export interface LayoutEdge { from: string; to: string }
export interface Point { x: number; y: number }

export interface LayoutOptions {
  width: number;
  height: number;
  iterations?: number;
  /** Abstand zum Rand, damit Knoten nicht unter Bedienelementen oder Legende landen. */
  margin?: number;
}

/** Berechnet Positionen in [0,width] x [0,height]. Unbekannte Kantenenden werden ignoriert. */
export function layoutGraph(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  { width, height, iterations = 220, margin = 12 }: LayoutOptions,
): Map<string, Point> {
  const n = nodes.length;
  const result = new Map<string, Point>();
  if (n === 0) return result;
  const cx = width / 2;
  const cy = height / 2;
  if (n === 1) { result.set(nodes[0].id, { x: cx, y: cy }); return result; }

  const index = new Map(nodes.map((node, i) => [node.id, i] as const));
  const links: [number, number][] = [];
  for (const edge of edges) {
    const a = index.get(edge.from);
    const b = index.get(edge.to);
    if (a !== undefined && b !== undefined && a !== b) links.push([a, b]);
  }

  const x = new Float64Array(n);
  const y = new Float64Array(n);
  const radius = Math.min(width, height) * 0.35;
  for (let i = 0; i < n; i++) {
    // Goldener Winkel verteilt die Startpunkte gleichmäßig ohne Zufall.
    const angle = i * 2.399963229728653;
    const r = radius * Math.sqrt((i + 0.5) / n);
    x[i] = cx + r * Math.cos(angle);
    y[i] = cy + r * Math.sin(angle);
  }

  const area = width * height;
  const ideal = Math.sqrt(area / n) * 0.5;
  let temperature = Math.min(width, height) / 8;
  const dx = new Float64Array(n);
  const dy = new Float64Array(n);

  for (let step = 0; step < iterations; step++) {
    dx.fill(0); dy.fill(0);
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        let vx = x[i] - x[j];
        let vy = y[i] - y[j];
        let dist = Math.hypot(vx, vy);
        if (dist < 0.01) { vx = 0.01 * ((i % 2) * 2 - 1); vy = 0.01; dist = Math.hypot(vx, vy); }
        // Abstoßung nur auf kurze Distanz, sonst treiben getrennte Gruppen in die Ecken.
        if (dist > ideal * 3) continue;
        const force = (ideal * ideal) / dist;
        dx[i] += (vx / dist) * force; dy[i] += (vy / dist) * force;
        dx[j] -= (vx / dist) * force; dy[j] -= (vy / dist) * force;
      }
    }
    for (const [a, b] of links) {
      const vx = x[a] - x[b];
      const vy = y[a] - y[b];
      const dist = Math.max(Math.hypot(vx, vy), 0.01);
      const force = (dist * dist) / ideal;
      dx[a] -= (vx / dist) * force; dy[a] -= (vy / dist) * force;
      dx[b] += (vx / dist) * force; dy[b] += (vy / dist) * force;
    }
    for (let i = 0; i < n; i++) {
      // Leichte Anziehung zur Mitte hält getrennte Inseln im Bild.
      dx[i] += (cx - x[i]) * 0.12;
      dy[i] += (cy - y[i]) * 0.12;
      const len = Math.max(Math.hypot(dx[i], dy[i]), 0.01);
      const move = Math.min(len, temperature);
      x[i] += (dx[i] / len) * move;
      y[i] += (dy[i] / len) * move;
      x[i] = Math.min(width - margin, Math.max(margin, x[i]));
      y[i] = Math.min(height - margin, Math.max(margin, y[i]));
    }
    temperature *= 0.975;
  }

  nodes.forEach((node, i) => result.set(node.id, { x: x[i], y: y[i] }));
  return result;
}

/** Knoten, die zu einer Suche passen, plus ihre direkten Nachbarn (für das Hervorheben). */
export function neighborhood(edges: LayoutEdge[], ids: Set<string>): Set<string> {
  const out = new Set(ids);
  for (const edge of edges) {
    if (ids.has(edge.from)) out.add(edge.to);
    if (ids.has(edge.to)) out.add(edge.from);
  }
  return out;
}
