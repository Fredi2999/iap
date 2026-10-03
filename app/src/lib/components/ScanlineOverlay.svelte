<script lang="ts">
  // Mittelblauer Hintergrund mit feinen Zeilen und weichem Rand für Start- und Entsperrbildschirm.
  // Statt eines Shaders genügt ein CSS-Verlauf; `paused` hält das leise Flackern an.
  interface Props { scanlineStrength?: number; paused?: boolean; class?: string; }
  let { scanlineStrength = 0.035, paused = false, class: className = "" }: Props = $props();
</script>

<div class={`scanlines ${paused ? "paused" : ""} ${className}`} style={`--scan:${scanlineStrength}`} aria-hidden="true"></div>

<style>
  .scanlines { position: absolute; inset: 0; overflow: hidden; pointer-events: none; background: radial-gradient(ellipse at 55% 40%, #4a6d82, #2c4859 55%, #07121d 100%); }
  .scanlines::before { content: ""; position: absolute; inset: 0; background: repeating-linear-gradient(0deg, rgb(0 0 0 / calc(var(--scan) * 8)) 0 1px, transparent 1px 3px); animation: scan-drift 9s linear infinite; }
  .scanlines::after { content: ""; position: absolute; inset: 0; background: radial-gradient(ellipse at center, transparent 60%, rgb(0 0 0 / .3)); }
  .scanlines.paused::before { animation: none; }
  @keyframes scan-drift { to { transform: translateY(3px); } }
  @media (prefers-reduced-motion: reduce) { .scanlines::before { animation: none; } }
  :global(:root[data-effects="calm"]) .scanlines::before { animation: none; }
</style>
