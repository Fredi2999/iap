<script lang="ts">
  // Mattierte Fläche: halbtransparenter Hintergrund mit Unschärfe dahinter. Das genügt für die
  // Seitenleiste und kostet weniger als ein Verzerrungsfilter.
  import type { Snippet } from "svelte";
  interface Props { children?: Snippet; borderRadius?: number; blur?: number; backgroundOpacity?: number; saturation?: number; class?: string; }
  let { children, borderRadius = 20, blur = 10, backgroundOpacity = 0.7, saturation = 1.2, class: className = "" }: Props = $props();
</script>

<div class={`frosted ${className}`} style={`--f-radius:${borderRadius}px;--f-blur:${blur}px;--f-alpha:${backgroundOpacity};--f-sat:${saturation}`}>
  {@render children?.()}
</div>

<style>
  .frosted { position: relative; border-radius: var(--f-radius); border: 1px solid var(--v-line); background: color-mix(in oklab, var(--v-surface-solid) calc(var(--f-alpha) * 100%), transparent); backdrop-filter: blur(var(--f-blur)) saturate(var(--f-sat)); -webkit-backdrop-filter: blur(var(--f-blur)) saturate(var(--f-sat)); box-shadow: inset 0 1px rgb(var(--v-tint) / .08), 0 10px 28px rgb(var(--v-shade) / .22); overflow: hidden; }
</style>
