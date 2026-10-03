<script lang="ts">
  import { t } from "../i18n/index.svelte";
  import { onMount } from "svelte";

  interface Props {
    title: string;
    badge?: string;
    description?: string;
    features?: string[];
    tip?: string;
    /** Ausrichtung; ohne Angabe wählt der Baustein die Seite mit genug Platz. */
    align?: "left" | "right" | "auto";
  }

  let {
    title,
    badge = "100 % offline",
    description = "",
    features = [],
    tip,
    align = "auto",
  }: Props = $props();

  let open = $state(false);
  let containerRef = $state<HTMLDivElement | null>(null);

  // Seite, zu der das Popover aufgeht: nach rechts, solange daneben Platz ist,
  // sonst nach links. So wird es am Rand einer Fläche nicht abgeschnitten.
  let side = $state<"left" | "right">("left");
  function toggle() {
    if (!open && containerRef) {
      const rect = containerRef.getBoundingClientRect();
      side = align !== "auto" ? (align === "right" ? "right" : "left") : window.innerWidth - rect.left >= 300 ? "left" : "right";
    }
    open = !open;
  }

  function handleKey(e: KeyboardEvent) {
    if (open && e.key === "Escape") {
      open = false;
    }
  }

  function handleClickOutside(e: MouseEvent) {
    if (open && containerRef && !containerRef.contains(e.target as Node)) {
      open = false;
    }
  }

  onMount(() => {
    document.addEventListener("click", handleClickOutside);
    document.addEventListener("keydown", handleKey);
    return () => {
      document.removeEventListener("click", handleClickOutside);
      document.removeEventListener("keydown", handleKey);
    };
  });
</script>

<div class="v-info-wrap" bind:this={containerRef}>
  <button type="button" class="v-info-trigger" aria-expanded={open} onclick={toggle}
    aria-label={t("Informationen zu {name}", { name: t(title) })} title={t("Informationen zu {name}", { name: t(title) })}>
    {t("i")}
  </button>

  {#if open}
    <!-- Klick-Popover: wächst vom Auslöser aus (links oder rechts oben), ohne Verzögerung. -->
    <div class="v-info-pop" class:right={side === "right"} role="tooltip">
      <div class="v-info-head">
        <h4>{t(title)}</h4>
        {#if badge}<span class="v-info-badge">{t(badge)}</span>{/if}
      </div>
      {#if description}<p class="v-info-text">{t(description)}</p>{/if}
      {#if features.length > 0}
        <div class="v-info-features">
          <span class="v-info-label">{t("Funktionen:")}</span>
          <ul>{#each features as feature}<li>{t(feature)}</li>{/each}</ul>
        </div>
      {/if}
      {#if tip}
        <div class="v-info-tip">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/></svg>
          <span>{t(tip ?? "")}</span>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .v-info-wrap { position: relative; display: inline-flex; align-items: center; }
  .v-info-trigger { display: flex; align-items: center; justify-content: center; width: 1.25rem; height: 1.25rem; padding: 0; border: 1px solid var(--v-line-strong); border-radius: 999px; background: rgb(var(--v-tint) / .06); color: var(--v-text-muted); font-size: var(--v-text-xs); font-weight: 650; cursor: pointer; transition: color 150ms ease, border-color 150ms ease, background-color 150ms ease, transform 120ms var(--v-ease-out-strong); }
  @media (hover: hover) and (pointer: fine) { .v-info-trigger:hover { color: var(--v-text-primary); border-color: var(--v-text-muted); } }
  .v-info-trigger:active { transform: scale(.94); }
  .v-info-trigger[aria-expanded="true"] { color: var(--v-text-primary); background: rgb(var(--v-tint) / .12); }
  .v-info-trigger:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: 2px; }
  .v-info-pop { position: absolute; z-index: 50; text-align: left; font-weight: 400; top: calc(100% + .5rem); left: 0; width: min(17rem, calc(100vw - 6rem)); max-height: min(28rem, 70dvh); overflow-y: auto; padding: .85rem; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-field); background: var(--v-surface-solid); color: var(--v-text-secondary); box-shadow: var(--v-shadow-lg); transform-origin: top left; opacity: 1; transform: scale(1); transition: opacity 150ms var(--v-ease-out-strong), transform 150ms var(--v-ease-out-strong); }
  .v-info-pop.right { left: auto; right: 0; transform-origin: top right; }
  @starting-style { .v-info-pop { opacity: 0; transform: scale(.96); } }
  .v-info-head { display: flex; align-items: center; justify-content: space-between; gap: .5rem; margin-bottom: .5rem; padding-bottom: .5rem; border-bottom: 1px solid var(--v-line); }
  h4 { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-sm); font-weight: 600; }
  .v-info-badge { flex: 0 0 auto; padding: 1px 7px; border: 1px solid var(--v-line); border-radius: 999px; color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .v-info-text { margin: 0 0 .75rem; font-size: var(--v-text-sm); line-height: 1.5; }
  .v-info-features { margin-bottom: .75rem; }
  .v-info-label { display: block; margin-bottom: .3rem; color: var(--v-text-muted); font-size: var(--v-text-xs); font-weight: 600; }
  ul { margin: 0; padding-left: 1rem; list-style: disc; font-size: var(--v-text-sm); line-height: 1.45; }
  li + li { margin-top: .2rem; }
  .v-info-tip { display: flex; align-items: flex-start; gap: .5rem; padding: .6rem; border: 1px solid var(--v-line); border-radius: var(--v-radius-control); background: rgb(var(--v-tint) / .04); color: var(--v-text-muted); font-size: var(--v-text-xs); line-height: 1.4; }
  .v-info-tip svg { flex: 0 0 auto; margin-top: 1px; color: var(--v-accent-blue); }
  @media (prefers-reduced-motion: reduce) { .v-info-pop { transition: opacity 120ms ease; transform: none; } }
</style>
