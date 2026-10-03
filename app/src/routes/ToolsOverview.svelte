<script lang="ts">
  import { t } from "../lib/i18n/index.svelte";
  import type { Route } from "../lib/types";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { DESTINATIONS, ROUTE_HINTS, ROUTE_ICONS, ROUTE_LABELS } from "../lib/navigation";

  interface Props { onOpen: (route: Route) => void; }
  let { onOpen }: Props = $props();

  const tools = DESTINATIONS.find((destination) => destination.id === "tools")?.routes ?? [];
</script>

<PageHeader title={t("Werkzeuge")} description="Spezialisierte Helfer für Aufgaben, die über einen normalen Chat hinausgehen." />

<div class="v-tool-grid">
  {#each tools as route (route)}
    <button type="button" class="v-tool-tile" onclick={() => onOpen(route)}>
      <span class="v-tool-icon" aria-hidden="true"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d={ROUTE_ICONS[route]}/></svg></span>
      <strong>{t(ROUTE_LABELS[route])}</strong>
      <span class="v-tool-hint">{t(ROUTE_HINTS[route])}</span>
    </button>
  {/each}
</div>

<style>
  .v-tool-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr)); gap: var(--v-space-3); }
  .v-tool-tile { display: flex; flex-direction: column; align-items: flex-start; gap: var(--v-space-2); min-height: 8.5rem; padding: var(--v-space-4); border: 1px solid var(--v-line); border-radius: var(--v-radius-card); background: var(--v-surface-2); color: var(--v-text-primary); text-align: left; transition: border-color 160ms ease, background-color 160ms ease, transform 120ms var(--v-ease-out-strong); }
  @media (hover: hover) and (pointer: fine) { .v-tool-tile:hover { border-color: var(--v-line-strong); background: var(--v-surface-3); } }
  .v-tool-tile:active { transform: scale(.985); }
  .v-tool-tile:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: 2px; }
  .v-tool-icon { display: grid; place-items: center; width: 2.5rem; height: 2.5rem; border-radius: var(--v-radius-control); background: var(--v-accent-blue-soft); color: var(--v-accent-blue); }
  strong { margin-top: var(--v-space-1); font-size: var(--v-text-lg); font-weight: 600; }
  .v-tool-hint { color: var(--v-text-muted); font-size: var(--v-text-sm); line-height: 1.45; }
</style>
