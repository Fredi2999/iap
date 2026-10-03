<script lang="ts">
  import { onMount, tick } from "svelte";
  import { t } from "../i18n/index.svelte";

  // Rechtsklick-Menü des Avatars (Hauptseite und Pet). Bedienbar per Tastatur:
  // Pfeiltasten, Pos1/Ende, Eingabe, Escape. Jeder Eintrag ist eine bewusste
  // Nutzeraktion; das Menü selbst startet nichts.
  export interface MenuItem {
    id: string;
    label: string;
    /** Wenn gesetzt, ist der Eintrag deaktiviert und zeigt den Grund. */
    disabledReason?: string | null;
    danger?: boolean;
    /** Trennlinie davor. */
    separated?: boolean;
    run: () => void;
  }

  interface Props {
    x: number;
    y: number;
    items: MenuItem[];
    onClose: () => void;
  }
  let { x, y, items, onClose }: Props = $props();

  let menu = $state<HTMLElement | null>(null);
  let position = $state({ left: 0, top: 0 });
  let active = $state(0);

  onMount(() => {
    position = { left: x, top: y };
    void (async () => {
      await tick();
      // Im sichtbaren Bereich halten (Pet-Fenster und Ränder).
      if (menu) {
        const box = menu.getBoundingClientRect();
        position = {
          left: Math.max(4, Math.min(x, window.innerWidth - box.width - 4)),
          top: Math.max(4, Math.min(y, window.innerHeight - box.height - 4)),
        };
      }
      active = Math.max(0, items.findIndex((item) => !item.disabledReason));
      focusItem();
    })();
    const outside = (event: PointerEvent) => { if (menu && !menu.contains(event.target as Node)) onClose(); };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("blur", onClose);
    };
  });

  function focusItem() {
    menu?.querySelectorAll<HTMLElement>("[role='menuitem']")[active]?.focus();
  }

  function move(step: number) {
    const count = items.length;
    for (let i = 1; i <= count; i++) {
      const next = (active + step * i + count * 2) % count;
      if (!items[next].disabledReason) { active = next; break; }
    }
    focusItem();
  }

  function choose(item: MenuItem) {
    if (item.disabledReason) return;
    onClose();
    item.run();
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") { event.preventDefault(); move(1); }
    else if (event.key === "ArrowUp") { event.preventDefault(); move(-1); }
    else if (event.key === "Home") { event.preventDefault(); active = -1; move(1); }
    else if (event.key === "End") { event.preventDefault(); active = 0; move(-1); }
    else if (event.key === "Escape" || event.key === "Tab") { event.preventDefault(); onClose(); }
  }
</script>

<div class="v-avatar-menu" role="menu" tabindex="-1" aria-label={t("Avatar-Menü")} bind:this={menu} style={`left:${position.left}px;top:${position.top}px`} onkeydown={keydown}>
  {#each items as item, index (item.id)}
    {#if item.separated}<div class="v-avatar-menu-sep" role="separator"></div>{/if}
    <button type="button" role="menuitem" class:danger={item.danger} aria-disabled={item.disabledReason ? "true" : undefined} title={item.disabledReason ?? undefined}
      tabindex={index === active ? 0 : -1} onclick={() => choose(item)} onpointerenter={() => { if (!item.disabledReason) { active = index; focusItem(); } }}>
      <span>{t(item.label)}</span>
      {#if item.disabledReason}<small>{t(item.disabledReason)}</small>{/if}
    </button>
  {/each}
</div>

<style>
  .v-avatar-menu { position: fixed; z-index: 120; min-width: 15rem; max-width: min(22rem, calc(100vw - 8px)); padding: .3rem; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-field); background: var(--v-surface-solid); box-shadow: var(--v-shadow-lg); transform-origin: top left; transition: opacity 130ms var(--v-ease-out-strong), transform 130ms var(--v-ease-out-strong); }
  @starting-style { .v-avatar-menu { opacity: 0; transform: scale(.97); } }
  button { display: grid; gap: .1rem; width: 100%; padding: .55rem .7rem; border: 0; border-radius: .5rem; background: transparent; color: var(--v-text-primary); font: inherit; font-size: var(--v-text-sm); text-align: left; cursor: pointer; }
  button small { color: var(--v-text-muted); font-size: var(--v-text-xs); line-height: 1.35; }
  button:focus-visible, button:hover:not([aria-disabled]) { background: rgb(var(--v-tint) / .1); outline: none; }
  button[aria-disabled] { color: var(--v-text-muted); cursor: not-allowed; }
  button.danger { color: var(--v-danger); }
  .v-avatar-menu-sep { height: 1px; margin: .25rem .3rem; background: var(--v-line); }
  @media (prefers-reduced-motion: reduce) { .v-avatar-menu { transition: opacity 100ms ease; } @starting-style { .v-avatar-menu { transform: none; } } }
</style>
