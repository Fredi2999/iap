<script lang="ts">
  // Logo mit sanftem Leuchten; ein Klick lässt es kurz heller aufleuchten.
  import { t } from "../i18n/index.svelte";
  interface Props { src: string; size?: number; glowColor?: string; interactive?: boolean; class?: string; }
  let { src, size = 28, glowColor = "#62bfff", interactive = true, class: className = "" }: Props = $props();
  let flash = $state(false);
  function pulse() {
    if (!interactive) return;
    flash = true;
    window.setTimeout(() => { flash = false; }, 500);
  }
</script>

<button type="button" class={`glow-logo ${flash ? "flash" : ""} ${className}`} style={`--logo-size:${size}px;--logo-glow:${glowColor}`} onclick={pulse} aria-label={t("IAP Logo aufladen")}>
  <img {src} alt="" />
</button>

<style>
  .glow-logo { display: inline-grid; place-items: center; flex: 0 0 auto; width: var(--logo-size); height: var(--logo-size); padding: 0; border: 0; background: transparent; cursor: pointer; }
  .glow-logo:focus-visible { outline: 1px solid var(--v-focus-ring); outline-offset: 4px; border-radius: .35rem; }
  img { width: 100%; height: 100%; object-fit: contain; filter: drop-shadow(0 0 5px var(--logo-glow)); animation: logo-breathe 3.4s ease-in-out infinite; transition: filter 200ms ease, transform 200ms ease; }
  .flash img { filter: drop-shadow(0 0 12px var(--logo-glow)); transform: scale(1.07); }
  @keyframes logo-breathe { 50% { filter: drop-shadow(0 0 9px var(--logo-glow)); } }
  @media (prefers-reduced-motion: reduce) { img { animation: none; transition: none; } }
  :global(:root[data-effects="calm"]) img { animation: none; }
</style>
