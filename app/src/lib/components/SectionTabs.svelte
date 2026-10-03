<script lang="ts" generics="T extends string">
  interface Item { id: T; label: string }
  interface Props {
    items: Item[];
    active: T;
    label: string;
    onSelect: (id: T) => void;
  }

  let { items, active, label, onSelect }: Props = $props();

  // Pfeiltasten bewegen den Fokus wie bei nativen Reitern.
  function keydown(event: KeyboardEvent, index: number) {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const next = items[(index + step + items.length) % items.length];
    onSelect(next.id);
    queueMicrotask(() => (document.querySelector(`[data-tab-id="${next.id}"]`) as HTMLElement | null)?.focus());
  }
</script>

<div class="v-section-tabs" role="tablist" aria-label={label}>
  {#each items as item, index (item.id)}
    <button type="button" role="tab" data-tab-id={item.id} aria-selected={item.id === active} tabindex={item.id === active ? 0 : -1}
      class:active={item.id === active} onclick={() => onSelect(item.id)} onkeydown={(event) => keydown(event, index)}>
      {item.label}
    </button>
  {/each}
</div>

<style>
  .v-section-tabs { display: flex; gap: 2px; margin-bottom: var(--v-space-5); padding-bottom: 1px; overflow-x: auto; border-bottom: 1px solid var(--v-line); scrollbar-width: none; }
  button { position: relative; flex: 0 0 auto; min-height: 2.5rem; padding: 0 var(--v-space-4); border: 0; background: transparent; color: var(--v-text-muted); font-size: var(--v-text-sm); font-weight: 550; white-space: nowrap; border-radius: var(--v-radius-control) var(--v-radius-control) 0 0; transition: color 160ms ease, background-color 160ms ease; }
  button::after { content: ""; position: absolute; left: var(--v-space-3); right: var(--v-space-3); bottom: -2px; height: 2px; border-radius: 2px; background: var(--v-accent-blue); transform: scaleX(0); transition: transform 200ms var(--v-ease-out-strong); }
  @media (hover: hover) and (pointer: fine) { button:hover { color: var(--v-text-primary); background: rgb(var(--v-tint) / .04); } }
  button.active { color: var(--v-text-primary); }
  button.active::after { transform: scaleX(1); }
  button:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: -2px; }
  @media (prefers-reduced-motion: reduce) { button::after { transition: none; } }
</style>
