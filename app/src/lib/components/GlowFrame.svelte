<script lang="ts">
  // Rahmen mit Leuchten am Rand. Die Farbe leuchtet auf, wenn der Zeiger darüber ist, die Taste
  // fokussiert wird oder der Eintrag aktiv ist; Position und Stärke steuert ausschließlich CSS.
  import type { Snippet } from "svelte";
  interface Props { children?: Snippet; borderRadius?: number; active?: boolean; color?: string; class?: string; }
  let { children, borderRadius = 10, active = false, color = "var(--v-accent-blue)", class: className = "" }: Props = $props();
</script>

<div class={`glow-frame ${active ? "active" : ""} ${className}`} style={`--g-radius:${borderRadius}px;--g-color:${color}`}>
  {@render children?.()}
</div>

<style>
  .glow-frame { position: relative; border-radius: var(--g-radius); transition: box-shadow 180ms ease; }
  .glow-frame::after { content: ""; position: absolute; inset: 0; border-radius: inherit; box-shadow: 0 0 0 1px color-mix(in oklab, var(--g-color) 55%, transparent), 0 0 14px color-mix(in oklab, var(--g-color) 35%, transparent); opacity: 0; pointer-events: none; transition: opacity 180ms ease; }
  .glow-frame:hover::after, .glow-frame:focus-within::after { opacity: .75; }
  .glow-frame.active::after { opacity: .5; }
  @media (prefers-reduced-motion: reduce) { .glow-frame::after { transition: none; } }
  :global(:root[data-effects="calm"]) .glow-frame::after { box-shadow: 0 0 0 1px color-mix(in oklab, var(--g-color) 55%, transparent); }
</style>
