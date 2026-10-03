<script lang="ts">
  // Laufband für kurze Texte. Zwei gleiche Hälften laufen per CSS nahtlos durch; die Zahl
  // `velocity` ist die Dauer eines Durchlaufs in Sekunden je 100 Zeichen Inhalt.
  interface Props { texts: string[]; velocity?: number; }
  let { texts, velocity = 18 }: Props = $props();
  const seconds = $derived(Math.max(8, Math.round((texts.join("").length / 100) * velocity) + 8));
</script>

<div class="marquee" aria-hidden="true" style={`--m-time:${seconds}s`}>
  <div class="track">
    {#each [0, 1] as copy (copy)}
      <div class="half">{#each texts as item (item)}<span>{item}</span>{/each}</div>
    {/each}
  </div>
</div>

<style>
  .marquee { overflow: hidden; width: 100%; border-block: 1px solid var(--v-line); padding: .65rem 0; color: var(--v-text-muted); }
  .track { display: flex; width: max-content; animation: marquee-run var(--m-time) linear infinite; }
  .half { display: flex; align-items: center; gap: 1.6rem; padding-right: 1.6rem; }
  span { font-size: .69rem; font-weight: 590; letter-spacing: .12em; text-transform: uppercase; white-space: nowrap; }
  span::after { content: "·"; margin-left: 1.6rem; color: var(--v-text-disabled); }
  @keyframes marquee-run { to { transform: translateX(-50%); } }
  @media (prefers-reduced-motion: reduce) { .track { animation: none; } }
  :global(:root[data-effects="calm"]) .track { animation: none; }
</style>
