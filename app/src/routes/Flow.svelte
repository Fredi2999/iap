<script lang="ts">
  import { t } from "../lib/i18n/index.svelte";
  import Workflows from "./Workflows.svelte";
  import AgentFlow from "./AgentFlow.svelte";

  // Flow Version: ein Modus im selben Fenster mit zwei Bereichen, Agent Flow
  // und Workflows. „Zurück“ führt zur vorherigen Ansicht (auch zur Startseite,
  // wenn das die letzte war); ein eigener Startseite-Knopf daneben brachte
  // nichts zusätzliches und ist deshalb weg. Sitzung, Entwürfe und erlaubte
  // Aufgaben bleiben beim Wechsel erhalten.
  interface Props {
    tab: "agent" | "workflows";
    canGoBack: boolean;
    onTab: (tab: "agent" | "workflows") => void;
    onBack: () => void;
    onOpenConnectors: () => void;
  }
  let { tab, canGoBack, onTab, onBack, onOpenConnectors }: Props = $props();
</script>

<section class="v-flow" aria-labelledby="v-flow-title">
  <header class="v-flow-head">
    <div class="v-flow-nav">
      <button type="button" class="v-btn v-btn-ghost" onclick={onBack} disabled={!canGoBack}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 6l-6 6 6 6"/></svg>
        <span>{t("Zurück")}</span>
      </button>
    </div>
    <h1 id="v-flow-title">{t("Flow Version")}</h1>
    <div class="v-segmented" role="tablist" aria-label={t("Bereich der Flow Version")}>
      <button type="button" role="tab" aria-selected={tab === "agent"} class:active={tab === "agent"} onclick={() => onTab("agent")}>{t("Agent Flow")}</button>
      <button type="button" role="tab" aria-selected={tab === "workflows"} class:active={tab === "workflows"} onclick={() => onTab("workflows")}>{t("Workflows")}</button>
    </div>
  </header>

  <!-- Beide Bereiche bleiben eingehängt, damit Entwürfe und laufende Aufgaben
       beim Wechsel des Tabs erhalten bleiben. -->
  <div class="v-flow-body" role="tabpanel" hidden={tab !== "agent"}>
    <AgentFlow active={tab === "agent"} />
  </div>
  <div class="v-flow-body" role="tabpanel" hidden={tab !== "workflows"}>
    <Workflows {onOpenConnectors} />
  </div>
</section>

<style>
  .v-flow { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--v-space-5); width: min(72rem, 100%); margin: 0 auto; }
  .v-flow-head { display: flex; flex-wrap: wrap; align-items: center; gap: var(--v-space-3) var(--v-space-5); }
  .v-flow-nav { display: flex; gap: var(--v-space-2); }
  .v-flow-nav .v-btn { gap: .4rem; }
  h1 { margin: 0; margin-right: auto; color: var(--v-text-primary); font-size: var(--v-text-2xl); font-weight: 600; letter-spacing: -.02em; }
</style>
