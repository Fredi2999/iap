<script lang="ts">
  import { t, tk } from "../lib/i18n/index.svelte";
  import { onMount } from "svelte";
  import {
    getConnectorConfig,
    updateConnectorConfig,
  } from "../lib/ipc";
  import type { ConnectorConfig } from "../lib/types";
  import ConnectorCard from "../lib/components/ConnectorCard.svelte";
  import GmailCard from "../lib/components/GmailCard.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { notify } from "../lib/notifications";

  interface Props {
    configSnapshot: ConnectorConfig | null;
    onConfigChanged: (config: ConnectorConfig) => void;
    onToggleAirGap: (draft?: ConnectorConfig) => Promise<void>;
    airGapBusy: boolean;
  }
  let { configSnapshot, onConfigChanged, onToggleAirGap, airGapBusy }: Props = $props();
  let appliedSnapshot: ConnectorConfig | null = null;

  // Konfiguration
  let config: ConnectorConfig = $state({
    offline_mode: true,
    exa_enabled: false,
    exa_api_key: "",
    wikipedia_enabled: false,
    open_meteo_enabled: false,
    brave_enabled: false,
    brave_api_key: "",
    gmail_enabled: false,
    gmail_address: "",
    gmail_target_email: "",
    gmail_check_interval_minutes: 5,
    gmail_reply_mode: "draft",
    gmail_send_acknowledged: false,
    gmail_max_per_hour: 6,
    gmail_max_per_day: 30,
    gmail_instruction: "",
    gmail_allow_read: false,
  });

  let loading = $state(true);
  let saving = $state(false);

  $effect(() => {
    if (configSnapshot && configSnapshot !== appliedSnapshot) {
      // Lokale Formulareingaben bleiben bei einem Air-Gap-Wechsel in der Kopfzeile erhalten.
      const previousSnapshot = appliedSnapshot;
      const editedFields = previousSnapshot
        ? Object.fromEntries(
            (Object.keys(configSnapshot) as Array<keyof ConnectorConfig>)
              .filter((key) => key !== "offline_mode" && config[key] !== previousSnapshot[key])
              .map((key) => [key, config[key]]),
          )
        : {};
      config = { ...configSnapshot, ...editedFields };
      appliedSnapshot = configSnapshot;
    }
  });

  onMount(async () => {
    try {
      config = await getConnectorConfig();
      onConfigChanged(config);
    } catch (e: any) {
      showMessage(e?.message || String(e), "error");
    } finally {
      loading = false;
    }
  });

  function showMessage(msg: string, type: "success" | "error" = "success") {
    notify(t(type === "error" ? "Konnektor-Fehler" : "Konnektoren"), msg, type);
  }

  async function saveConfig(nextConfig: ConnectorConfig = config): Promise<boolean> {
    if (saving) return false;
    saving = true;
    try {
      config = await updateConnectorConfig(nextConfig);
      onConfigChanged(config);
      showMessage(t("Einstellungen erfolgreich gesichert."));
      return true;
    } catch (e: any) {
      showMessage(e?.message || String(e), "error");
      return false;
    } finally {
      saving = false;
    }
  }

  async function toggleOffline() {
    if (saving || airGapBusy) return;
    await onToggleAirGap(config);
  }

  /** Schaltet einen Dienst ein oder aus. Bei Air Gap an lässt sich kein Dienst einschalten:
      Konnektoren sollen nie „aktiv“ aussehen, solange IAP offline sein soll. */
  async function toggle(field: "exa_enabled" | "wikipedia_enabled" | "open_meteo_enabled" | "brave_enabled") {
    if (config.offline_mode && !config[field]) return;
    await saveConfig({ ...config, [field]: !config[field] });
  }

  /** Speichert einen einzelnen Wert (Schlüssel), ohne andere ungespeicherte Eingaben zu verlieren. */
  async function saveField(patch: Partial<ConnectorConfig>) {
    await saveConfig({ ...config, ...patch });
  }
</script>

<div class="connectors-page v-page">
  <PageHeader title={t("Konnektoren")} description="Externe Dienste, die IAP nutzen darf: Websuche (Exa, Brave), Wikipedia und Wetter. Solange der Air Gap an ist, wird nichts gesendet und IAP bleibt vollständig offline.">
    {#snippet actions()}
      <button onclick={() => saveConfig()} disabled={saving || loading} class="v-btn v-btn-primary">{t(saving ? "Speichere …" : "Änderungen speichern")}</button>
    {/snippet}
    {#snippet help()}{t("Nur diese Dienste arbeiten im Internet, und nur nach deiner Freigabe für einen Lauf oder Test. Jeder Aufruf wird im Protokoll festgehalten.")}{/snippet}
  </PageHeader>

  <!-- Hauptschalter: Air Gap. Gestaltung über Design-Tokens, damit sie zum Rest passt. -->
  <section class="v-card v-air" class:on={config.offline_mode}>
    <div class="v-air-icon" aria-hidden="true">
      {#if config.offline_mode}
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>
          <path d="m9 12 2 2 4-4"/>
        </svg>
      {:else}
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="10"/>
          <path d="M12 2a14.5 14.5 0 0 0 0 20M12 2a14.5 14.5 0 0 1 0 20M2 12h20"/>
        </svg>
      {/if}
    </div>
    <div class="v-air-text">
      <div class="v-row v-air-title">
        <h2 class="v-card-title">{t("Air Gap · Konnektoren")}</h2>
        <span class="v-chip" class:accent={config.offline_mode}>{t(config.offline_mode ? "OFFLINE-MODUS AKTIV" : "AIR GAP AUS")}</span>
      </div>
      <p class="v-card-text">
        {#if config.offline_mode}
          {t("Die Konnektoren sind gesperrt. IAP bleibt lokal; nur der eigene Inferenzprozess auf 127.0.0.1 wird angesprochen.")}
        {:else}
          {t("Der Air Gap ist aus. Nur die Dienste unten können ins Internet, und nur für Workflow-Läufe und Tests, die du jeweils ausdrücklich freigibst; es gehen nur sichtbare, öffentliche Suchbegriffe hinaus.")}
        {/if}
      </p>
    </div>
    <div class="v-air-switch">
      <button
        type="button"
        class={`v-apple-switch ${config.offline_mode ? "active" : ""}`}
        onclick={toggleOffline}
        disabled={saving || airGapBusy}
        title={t("Air Gap umschalten")}
        aria-label={t("Air Gap umschalten")}
        aria-pressed={config.offline_mode}
      >
        <span class="v-apple-thumb"></span>
      </button>
      <span class="v-help">{t(config.offline_mode ? "Offline · gesperrt" : "Air Gap aus")}</span>
    </div>
  </section>

  <div class="v-stack v-connector-list">

    <ConnectorCard
      id="exa"
      title={tk("Exa Websuche")}
      subtitle={tk("Websuche über die Exa-Schnittstelle")}
      description={tk("Workflows können über Exa im Web suchen und Seiten lesen. Es gehen nur sichtbare, öffentliche Suchbegriffe hinaus, und jeder Lauf braucht deine Freigabe.")}
      enabled={config.exa_enabled}
      airGap={config.offline_mode}
      onToggle={() => toggle("exa_enabled")}
      keyLabel={tk("Exa-API-Schlüssel")}
      keyValue={config.exa_api_key}
      keyPlaceholder="exa-api-key-..."
      onKey={(value) => saveField({ exa_api_key: value })}
      testPlaceholder={tk("Suchanfrage oder Fragestellung eingeben ...")}
      consentText={tk("Ich bestätige: Dieser Suchtext ist öffentlich und darf an Exa (api.exa.ai) gesendet werden. Die Freigabe gilt nur für diese eine Suche.")}
    />

    <ConnectorCard
      id="wikipedia"
      title={tk("Wikipedia")}
      subtitle={tk("Artikel suchen und Kurzfassungen lesen")}
      description={tk("Kostenlos und ohne Schlüssel. Workflows können Wikipedia (deutsch oder englisch) nach Artikeln durchsuchen. Es gehen nur sichtbare, öffentliche Suchbegriffe an de.wikipedia.org oder en.wikipedia.org.")}
      enabled={config.wikipedia_enabled}
      airGap={config.offline_mode}
      onToggle={() => toggle("wikipedia_enabled")}
      testPlaceholder={tk("Suchbegriff eingeben ...")}
      consentText={tk("Ich bestätige: Dieser Suchtext ist öffentlich und darf an Wikipedia gesendet werden. Die Freigabe gilt nur für diese eine Suche.")}
    />

    <ConnectorCard
      id="open_meteo"
      title={tk("Wetter (Open-Meteo)")}
      subtitle={tk("Aktuelles Wetter und Vorhersage für einen Ort")}
      description={tk("Kostenlos und ohne Schlüssel. Workflows können das Wetter für einen Ort abfragen, etwa im Tagesüberblick. Es geht nur der sichtbare, öffentliche Ortsname an api.open-meteo.com.")}
      note={tk("Open-Meteo ist in der kostenlosen Form nur für nicht-kommerzielle Nutzung gedacht.")}
      enabled={config.open_meteo_enabled}
      airGap={config.offline_mode}
      onToggle={() => toggle("open_meteo_enabled")}
      testPlaceholder={tk("Ort eingeben, zum Beispiel Wien")}
      consentText={tk("Ich bestätige: Dieser Ortsname ist öffentlich und darf an Open-Meteo gesendet werden. Die Freigabe gilt nur für diese eine Anfrage.")}
      textResult
    />

    <ConnectorCard
      id="brave"
      title={tk("Brave Search")}
      subtitle={tk("Websuche als Alternative zu Exa")}
      description={tk("Websuche über Brave mit eigenem Schlüssel. Es gehen nur sichtbare, öffentliche Suchbegriffe an api.search.brave.com, und jeder Lauf braucht deine Freigabe.")}
      note={tk("Laut Anbieter kostet die Suche 5 US-Dollar je 1000 Anfragen, dazu gibt es monatlich ein Gratisguthaben. IAP rechnet 0,005 US-Dollar je Aufruf ins Kostenlimit des Laufs. Ob für das Konto ein Zahlungsmittel nötig ist, siehst du bei der Anmeldung.")}
      enabled={config.brave_enabled}
      airGap={config.offline_mode}
      onToggle={() => toggle("brave_enabled")}
      keyLabel={tk("Brave-API-Schlüssel")}
      keyValue={config.brave_api_key}
      keyPlaceholder="BSA..."
      onKey={(value) => saveField({ brave_api_key: value })}
      testPlaceholder={tk("Suchanfrage eingeben ...")}
      consentText={tk("Ich bestätige: Dieser Suchtext ist öffentlich und darf an Brave (api.search.brave.com) gesendet werden. Die Freigabe gilt nur für diese eine Suche.")}
    />

    <GmailCard {config} onSave={(patch) => saveConfig({ ...config, ...patch })} />
  </div>
</div>

<style>
  .v-connector-list { gap: var(--v-space-4); margin-top: var(--v-space-5); }
  .v-air { display: grid; grid-template-columns: auto 1fr auto; align-items: center; gap: var(--v-space-4); }
  .v-air-icon { display: grid; place-items: center; width: 3rem; height: 3rem; border-radius: var(--v-radius-control); background: var(--v-surface-2); border: 1px solid var(--v-line); color: var(--v-text-muted); }
  .v-air.on .v-air-icon { background: var(--v-accent-blue-soft); border-color: color-mix(in oklab, var(--v-accent-blue) 45%, transparent); color: var(--v-accent-blue); }
  .v-air-title { align-items: center; gap: var(--v-space-3); flex-wrap: wrap; }
  .v-air-switch { display: inline-flex; flex-direction: column; align-items: center; gap: var(--v-space-2); }
  @media (max-width: 650px) {
    .v-air { grid-template-columns: auto 1fr; }
    .v-air-switch { grid-column: 1 / -1; flex-direction: row; justify-content: flex-start; }
  }
</style>
