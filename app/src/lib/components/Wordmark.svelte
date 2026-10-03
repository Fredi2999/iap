<script lang="ts">
  // Wortmarke aus einzelnen Buchstaben: sie steigen beim Erscheinen nacheinander auf und heben
  // sich beim Darüberfahren leicht an. Reines CSS, kein Canvas, damit sie keine Rechenzeit kostet.
  interface Props { text?: string; compact?: boolean; class?: string; }
  let { text = "IAP", compact = false, class: className = "" }: Props = $props();
</script>

<div class={`wordmark ${compact ? "compact" : ""} ${className}`} role="img" aria-label={text}>
  {#each [...text] as letter, index (index)}<span aria-hidden="true" style={`--i:${index}`}>{letter}</span>{/each}
</div>

<style>
  .wordmark { display: flex; align-items: center; justify-content: center; width: 100%; height: 100%; color: var(--v-text-primary); font-size: clamp(3.7rem, 7.5vw, 5.3rem); font-weight: 590; letter-spacing: .08em; line-height: 1; user-select: none; }
  .wordmark.compact { font-size: 1.28rem; }
  span { display: inline-block; animation: wordmark-rise 800ms cubic-bezier(.2, .8, .2, 1) both; animation-delay: calc(var(--i) * 110ms); transition: transform 220ms ease, color 220ms ease; }
  @media (hover: hover) and (pointer: fine) { span:hover { transform: translateY(-.07em); color: var(--v-accent-blue); } }
  @keyframes wordmark-rise { from { opacity: 0; transform: translateY(.3em); filter: blur(6px); } }
  @media (prefers-reduced-motion: reduce) { span { animation: none; transition: none; } }
  :global(:root[data-effects="calm"]) span { animation: none; }
</style>
