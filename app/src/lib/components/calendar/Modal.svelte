<script lang="ts">
  // Gemeinsamer Rahmen der Kalenderdialoge: Abdunklung, Escape zum Schließen, Fokus ins erste Feld.
  import { onMount, type Snippet } from "svelte";
  import { t } from "../../i18n/index.svelte";

  interface Props {
    title: string;
    onClose: () => void;
    children: Snippet;
  }
  let { title, onClose, children }: Props = $props();
  let box = $state<HTMLDivElement | null>(null);

  onMount(() => {
    box?.querySelector<HTMLElement>("input, select, textarea")?.focus();
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<div class="v-cal-layer">
  <button type="button" class="v-cal-shade" aria-label={t("Schließen")} onclick={onClose}></button>
  <div class="v-cal-modal" role="dialog" aria-modal="true" aria-label={title} bind:this={box}>
    <h2>{title}</h2>
    {@render children()}
  </div>
</div>

<style>
  .v-cal-layer { position: fixed; inset: 0; z-index: 85; display: flex; align-items: center; justify-content: center; padding: 1rem; }
  .v-cal-shade { position: absolute; inset: 0; border: 0; border-radius: 0; background: rgb(var(--v-shade) / .5); cursor: default; }
  .v-cal-modal { position: relative; width: min(34rem, 100%); max-height: 100%; overflow: auto; display: flex; flex-direction: column; gap: var(--v-space-4); padding: var(--v-space-6); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: 0 25px 70px rgb(var(--v-shade) / .42); }
  h2 { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-lg); font-weight: 600; }
  .v-cal-modal { transition: opacity 200ms var(--v-ease-out-strong), transform 200ms var(--v-ease-out-strong); }
  .v-cal-shade { transition: opacity 200ms ease; }
  @starting-style { .v-cal-modal { opacity: 0; transform: scale(.96); } .v-cal-shade { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .v-cal-modal { transition: opacity 120ms ease; } @starting-style { .v-cal-modal { transform: none; } } }
</style>
