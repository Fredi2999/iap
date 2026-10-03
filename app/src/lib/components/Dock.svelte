<script lang="ts">
  import { t } from "../i18n/index.svelte";
  import FrostedPanel from "./FrostedPanel.svelte";
  import GlowFrame from "./GlowFrame.svelte";

  import { DESTINATIONS, type Destination, type DestinationId } from "../navigation";

  interface Props {
    active: DestinationId;
    onSelect: (id: DestinationId) => void;
  }

  let { active, onSelect }: Props = $props();

  const main = DESTINATIONS.filter((destination) => destination.id !== "settings" && !destination.hidden);
  const pinned = DESTINATIONS.filter((destination) => destination.id === "settings");

  let itemsEl = $state<HTMLElement | null>(null);
  let tip = $state<{ label: string; top: number } | null>(null);

  // Der aktive Eintrag bleibt sichtbar, auch wenn die Leiste gescrollt war.
  $effect(() => {
    void active;
    queueMicrotask(() => itemsEl?.querySelector<HTMLElement>("[aria-current='page']")?.scrollIntoView({ block: "nearest" }));
  });

  // Eigener Tooltip für die schmale Symbolleiste: erscheint sofort statt nach
  // der langen Verzögerung des Browser-Titels.
  function showTip(event: Event, destination: Destination) {
    if (!window.matchMedia("(max-width: 900px)").matches) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    tip = { label: t(destination.label), top: rect.top + rect.height / 2 };
  }
</script>

{#snippet item(destination: Destination)}
  <GlowFrame active={active === destination.id} borderRadius={10} class="iap-dock-glow">
  <button type="button" class:active={active === destination.id} aria-current={active === destination.id ? "page" : undefined}
    aria-label={t(destination.label)} onclick={() => onSelect(destination.id)}
    onpointerenter={(event) => showTip(event, destination)} onpointerleave={() => (tip = null)}
    onfocus={(event) => showTip(event, destination)} onblur={() => (tip = null)}>
    <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={destination.icon}/></svg>
    <span class="iap-dock-text"><span>{t(destination.label)}</span><small>{t(destination.hint)}</small></span>
  </button>
  </GlowFrame>
{/snippet}

<nav class="iap-dock" aria-label={t("Hauptnavigation")}>
  <FrostedPanel blur={10} borderRadius={20} backgroundOpacity={0.7} saturation={1.2} class="iap-dock-glass">
    <div class="iap-dock-inner">
      <div class="iap-dock-items" bind:this={itemsEl}>
        {#each main as destination (destination.id)}{@render item(destination)}{/each}
      </div>
      <div class="iap-dock-pinned">
        {#each pinned as destination (destination.id)}{@render item(destination)}{/each}
      </div>
    </div>
  </FrostedPanel>
</nav>
{#if tip}<div class="iap-dock-tip" role="tooltip" style={"top: " + tip.top + "px"}>{tip.label}</div>{/if}

<style>
  /* flex-shrink: 0 ist Absicht: ohne das presst breiter Seiteninhalt (z. B. die Kachelliste unter
     „Werkzeuge“) die Navigation schmaler, weil beide Geschwister im Flex-Layout sonst gleich stark
     schrumpfen. Die Navigation bleibt immer 14rem breit; der Inhalt scrollt bei Bedarf selbst. */
  .iap-dock { position: relative; z-index: 12; width: 14rem; flex-shrink: 0; min-height: 0; height: 100%; }
  :global(.iap-dock-glass) { height: 100% !important; border-radius: 1.25rem; }
  .iap-dock-inner { height: 100%; display: flex; flex-direction: column; padding: var(--v-space-2); }
  /* Platz für den Leuchtrand an den Seiten; waagerecht nie scrollen. */
  .iap-dock-items { flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; gap: 3px; margin: -4px; padding: 4px; overflow-x: hidden; overflow-y: auto; }
  .iap-dock-pinned { overflow: visible; }
  .iap-dock-pinned { flex: 0 0 auto; margin-top: var(--v-space-2); padding-top: var(--v-space-2); border-top: 1px solid var(--v-line); }
  button { width: 100%; min-height: 3rem; display: flex; align-items: center; gap: var(--v-space-3); flex: 0 0 auto; border: 0; border-radius: var(--v-radius-control); padding: var(--v-space-2) var(--v-space-3); color: var(--v-text-secondary); background: transparent; text-align: left; transition: background-color 160ms ease, color 160ms ease, transform 120ms var(--v-ease-out-strong); }
  button svg { flex: 0 0 auto; color: var(--v-text-muted); transition: color 160ms ease; }
  :global(.iap-dock-glow) { flex: 0 0 auto; width: 100%; }
  .iap-dock-text { display: flex; flex-direction: column; min-width: 0; }
  .iap-dock-text span { font-size: var(--v-text-md); font-weight: 550; line-height: 1.25; }
  .iap-dock-text small { overflow: hidden; color: var(--v-text-muted); font-size: var(--v-text-xs); line-height: 1.3; text-overflow: ellipsis; white-space: nowrap; }
  @media (hover: hover) and (pointer: fine) { button:hover { color: var(--v-text-primary); background: rgb(var(--v-tint) / .07); } }
  button:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: -2px; }
  button:active { transform: scale(.98); }
  button.active { color: var(--v-text-primary); background: rgb(var(--v-tint) / .13); box-shadow: inset 0 1px rgb(var(--v-tint) / .1); }
  button.active svg { color: var(--v-accent-blue); }
  .iap-dock-tip { position: fixed; z-index: 90; right: 5.1rem; transform: translateY(-50%); padding: 6px 10px; border: 1px solid var(--v-line-strong); border-radius: 8px; background: var(--v-surface-solid); color: var(--v-text-primary); font-size: var(--v-text-xs); white-space: nowrap; pointer-events: none; box-shadow: var(--v-shadow-md); }
  @media (max-width: 900px) {
    .iap-dock { width: 4.1rem; }
    .iap-dock-inner { padding: 6px; }
    button { justify-content: center; min-height: 2.75rem; padding: 0; }
    .iap-dock-text { display: none; }
  }
  @media (prefers-reduced-motion: reduce) { button, button svg { transition: none; } }
</style>
