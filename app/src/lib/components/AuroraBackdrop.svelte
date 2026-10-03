<script lang="ts">
  // Ruhiger Hintergrund aus drei weichen Farbflächen, die langsam gegeneinander driften.
  // Die Farben kommen aus dem gewählten Farbschema; `speed` skaliert die Dauer der Bewegung.
  interface Props { horizonColor?: string; waveColor?: string; crestColor?: string; baseColor?: string; speed?: number; }
  let { horizonColor = "#12344c", waveColor = "#175676", crestColor = "#69b8e0", baseColor = "#07131d", speed = .28 }: Props = $props();
  const seconds = $derived(Math.round(30 / Math.max(.05, speed)));
</script>

<div class="aurora" style={`--a-base:${baseColor};--a-horizon:${horizonColor};--a-wave:${waveColor};--a-crest:${crestColor};--a-time:${seconds}s`} aria-hidden="true">
  <i class="one"></i><i class="two"></i><i class="three"></i>
</div>

<style>
  .aurora { position: absolute; inset: 3.5rem 0 0; z-index: 0; overflow: hidden; pointer-events: none; background: linear-gradient(180deg, var(--a-horizon), var(--a-base) 70%); }
  i { position: absolute; width: 70%; height: 90%; border-radius: 50%; filter: blur(70px); opacity: .55; animation: aurora-drift var(--a-time) ease-in-out infinite alternate; }
  .one { left: -15%; bottom: -40%; background: var(--a-wave); }
  .two { right: -20%; bottom: -55%; background: var(--a-crest); opacity: .28; animation-delay: calc(var(--a-time) * -.4); }
  .three { left: 25%; bottom: -70%; background: var(--a-horizon); animation-delay: calc(var(--a-time) * -.7); }
  @keyframes aurora-drift { to { transform: translate3d(8%, -6%, 0) scale(1.12); } }
  @media (prefers-reduced-motion: reduce) { i { animation: none; } }
  :global(:root[data-effects="calm"]) i { animation: none; }
</style>
