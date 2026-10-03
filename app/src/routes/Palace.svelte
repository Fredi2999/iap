<script lang="ts">
  import { t, locale } from "../lib/i18n/index.svelte";
  import { onMount } from "svelte";
  import { listActiveFacts } from "../lib/ipc";
  import type { Fact, FactCategory } from "../lib/types";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import { FACT_CATEGORY_LABELS } from "../lib/labels";

  interface Node { fact: Fact; x: number; y: number; r: number; cluster: number; }
  interface Edge { a: number; b: number; strength: number; }

  const CATEGORY_COLORS: Record<FactCategory, string> = {
    preference: "#2dd4bf",
    project: "#a78bfa",
    person: "#f472b6",
    skill: "#fbbf24",
    constraint: "#f87171",
    other: "#94a3b8",
  };

  let facts = $state<Fact[]>([]);
  let error = $state<unknown>(null);
  let loaded = $state(false);
  let selected = $state<Node | null>(null);
  let filter = $state<FactCategory | "all">("all");

  const nodes = $derived.by<Node[]>(() => {
    const filtered = filter === "all" ? facts : facts.filter((f) => f.category === filter);
    if (filtered.length === 0) return [];
    const clusters = new Map<FactCategory, Fact[]>();
    for (const f of filtered) {
      const arr = clusters.get(f.category) ?? [];
      arr.push(f);
      clusters.set(f.category, arr);
    }
    const centerX = 400;
    const centerY = 320;
    const outerRadius = 240;
    const clusterList = Array.from(clusters.entries());
    const result: Node[] = [];
    clusterList.forEach(([cat, list], ci) => {
      const angle = (ci / clusterList.length) * Math.PI * 2 - Math.PI / 2;
      const cx = centerX + Math.cos(angle) * outerRadius * 0.55;
      const cy = centerY + Math.sin(angle) * outerRadius * 0.55;
      const innerRadius = Math.min(80, 20 + list.length * 8);
      list.forEach((fact, i) => {
        const a = list.length === 1 ? 0 : (i / list.length) * Math.PI * 2;
        result.push({
          fact,
          x: cx + Math.cos(a) * innerRadius,
          y: cy + Math.sin(a) * innerRadius,
          r: 10 + Math.min(fact.access_count, 5) * 1.5,
          cluster: ci,
        });
      });
    });
    return result;
  });

  const edges = $derived.by<Edge[]>(() => {
    const list: Edge[] = [];
    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        if (nodes[i].fact.category === nodes[j].fact.category) {
          list.push({ a: i, b: j, strength: 0.15 });
        }
      }
    }
    return list;
  });

  const categoryCounts = $derived(
    facts.reduce((acc, f) => {
      acc[f.category] = (acc[f.category] ?? 0) + 1;
      return acc;
    }, {} as Record<FactCategory, number>)
  );

  onMount(async () => {
    try {
      facts = await listActiveFacts();
    } catch (e) { error = e; }
    finally { loaded = true; }
  });
</script>

<div class="v-page">
  <PageHeader title={t("Wissensnetz")} description={t("Deine {n} Fakten als Netz, gruppiert nach Kategorie. Klicke auf einen Punkt für Details.", { n: facts.length })} />

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  {#if loaded && facts.length === 0}
    <EmptyState title={t("Das Netz ist noch leer")} text="Sag IAP im Chat „Merk dir …“ oder übernimm Notizen ins Gedächtnis." icon="M12 4l8 4v8l-8 4-8-4V8l8-4ZM12 12v8M12 12l8-4M12 12L4 8" />
  {:else if facts.length > 0}
    <div class="v-row" role="group" aria-label={t("Kategorie filtern")}>
      <button class="v-chip" class:accent={filter === "all"} onclick={() => (filter = "all")}>{t("Alle ({n})", { n: facts.length })}</button>
      {#each Object.entries(categoryCounts) as [cat, count] (cat)}
        <button class="v-chip" class:accent={filter === cat} onclick={() => (filter = cat as FactCategory)}>
          <span class="v-dot" style={`background: ${CATEGORY_COLORS[cat as FactCategory]}`}></span>{t(FACT_CATEGORY_LABELS[cat as FactCategory])} ({count})
        </button>
      {/each}
    </div>

    <div class="v-card v-palace">
      <svg viewBox="0 0 800 640" role="img" aria-label="Wissensnetz">
        {#each edges as edge, i (i)}
          <line x1={nodes[edge.a]?.x} y1={nodes[edge.a]?.y} x2={nodes[edge.b]?.x} y2={nodes[edge.b]?.y} stroke="var(--v-line-strong)" stroke-width="0.7" opacity={edge.strength * 2} />
        {/each}
        {#each nodes as node (node.fact.id)}
          <g class="v-node" onclick={() => (selected = node)} role="button" tabindex="0" aria-label={node.fact.text}
            onkeydown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); selected = node; } }}>
            <circle cx={node.x} cy={node.y} r={node.r + 6} fill={CATEGORY_COLORS[node.fact.category]} opacity={selected?.fact.id === node.fact.id ? 0.28 : 0} />
            <circle cx={node.x} cy={node.y} r={node.r} fill={CATEGORY_COLORS[node.fact.category]} fill-opacity="0.9" stroke="var(--v-surface-solid)" stroke-width="1.5" />
            <text x={node.x} y={node.y + node.r + 14} text-anchor="middle" fill="var(--v-text-secondary)" font-size="12">{node.fact.text.length > 20 ? node.fact.text.substring(0, 18) + "…" : node.fact.text}</text>
          </g>
        {/each}
      </svg>

      {#if selected}
        <aside class="v-palace-detail" aria-label={t("Fakt")}>
          <div class="v-row">
            <span class="v-dot" style={`background: ${CATEGORY_COLORS[selected.fact.category]}`}></span>
            <span class="v-label">{t(FACT_CATEGORY_LABELS[selected.fact.category])}</span>
            <button class="v-btn-icon v-palace-close" onclick={() => (selected = null)} aria-label={t("Details schließen")}>
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg>
            </button>
          </div>
          <p>{selected.fact.text}</p>
          <ul class="v-help">
            <li>{selected.fact.user_verified ? t("Von dir bestätigt") : t("Automatisch gelernt, {n} % sicher", { n: Math.round(selected.fact.confidence * 100) })}</li>
            <li>{t("{n} Mal in Antworten genutzt", { n: selected.fact.access_count })}</li>
            <li>{t("Seit {date}", { date: new Date(selected.fact.valid_from_unix_ms).toLocaleDateString(locale()) })}</li>
          </ul>
        </aside>
      {/if}
    </div>
  {/if}
</div>

<style>
  .v-dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; }
  button.v-chip { cursor: pointer; background: transparent; }
  .v-palace { position: relative; padding: 0; overflow: hidden; }
  .v-palace svg { display: block; width: 100%; height: min(640px, 64vh); }
  .v-node { cursor: pointer; outline: none; }
  .v-node:focus-visible circle:last-of-type { stroke: var(--v-focus-ring); stroke-width: 3; }
  .v-node text { pointer-events: none; user-select: none; }
  .v-palace-detail { position: absolute; top: var(--v-space-4); right: var(--v-space-4); width: min(18rem, calc(100% - 2rem)); padding: var(--v-space-4); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: var(--v-shadow-lg); }
  .v-palace-close { margin-left: auto; }
  .v-palace-detail p { margin: var(--v-space-2) 0; color: var(--v-text-primary); font-size: var(--v-text-sm); line-height: 1.55; }
  .v-palace-detail ul { margin: 0; padding-left: 1rem; }
</style>
