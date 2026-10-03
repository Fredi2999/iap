<script lang="ts">
  // Eine Konnektor-Karte: Schalter, optionaler Schlüssel, Hinweis und Einzeltest. Alle Dienste
  // (Exa, Wikipedia, Open-Meteo, Brave) nutzen dieselbe Karte, damit sie sich gleich verhalten.
  // Gestaltung über die Design-Tokens (v-card, v-label, v-help), damit sie zum Rest der App passt
  // und auch im hellen Modus stimmt.
  import { t } from "../i18n/index.svelte";
  import { testConnector } from "../ipc";
  import { friendlyError } from "../errors";
  import { notify } from "../notifications";
  import type { ExaSearchResult } from "../types";

  interface Props {
    /** Name für den Backend-Test: exa, wikipedia, open_meteo, brave. */
    id: string;
    title: string;
    subtitle: string;
    description: string;
    /** Zusätzlicher Hinweis (Preis, Nutzungsbedingung). */
    note?: string;
    enabled: boolean;
    airGap: boolean;
    onToggle: () => void;
    /** Nur Dienste mit Schlüssel. */
    keyLabel?: string;
    keyValue?: string;
    keyPlaceholder?: string;
    onKey?: (value: string) => void;
    /** Wofür der Testtext steht, etwa „Suchbegriff“ oder „Ort“. */
    testPlaceholder: string;
    consentText: string;
    /** Ergebnisse als Fließtext zeigen (Wetter) statt als Trefferliste. */
    textResult?: boolean;
  }
  let {
    id, title, subtitle, description, note, enabled, airGap, onToggle,
    keyLabel, keyValue = "", keyPlaceholder = "", onKey, testPlaceholder, consentText, textResult = false,
  }: Props = $props();

  let showKey = $state(false);
  let query = $state("");
  let consent = $state(false);
  let busy = $state(false);
  let results = $state<ExaSearchResult[]>([]);
  let failure = $state<string | null>(null);

  const keyMissing = $derived(keyLabel !== undefined && keyValue.trim() === "");
  const testBlock = $derived(
    airGap ? "Der Test braucht Air Gap aus." : !enabled ? "Schalte den Dienst erst ein." : keyMissing ? "Trage zuerst den Schlüssel ein." : null,
  );

  async function run() {
    if (!query.trim() || testBlock || !consent) return;
    busy = true;
    failure = null;
    results = [];
    try {
      results = await testConnector(id, query, true);
      notify(title, t("{n} Ergebnisse erhalten.", { n: results.length }), "success");
    } catch (reason) {
      failure = friendlyError(reason).message;
    } finally {
      busy = false;
      // Die Einwilligung gilt nur für diese eine Anfrage.
      consent = false;
    }
  }
</script>

<section class="v-card v-stack v-connector">
  <header class="v-connector-head">
    <div>
      <h3 class="v-card-title">{t(title)}</h3>
      <p class="v-help">{t(subtitle)}</p>
    </div>
    <div class="v-connector-switch">
      <span class="v-help">{t(enabled && !airGap ? "Aktiv" : airGap ? "Gesperrt (Air Gap)" : "Deaktiviert")}</span>
      <button type="button" class={`v-apple-switch ${enabled && !airGap ? "active" : ""}`} onclick={onToggle}
        disabled={airGap && !enabled} aria-pressed={enabled && !airGap} aria-label={t("{name} aktivieren", { name: t(title) })}>
        <span class="v-apple-thumb"></span>
      </button>
    </div>
  </header>

  <p class="v-card-text">{t(description)}</p>
  {#if note}<p class="v-help">{t(note)}</p>{/if}
  {#if enabled && airGap}
    <p class="v-notice warn">{t("Der Air Gap ist an: Dieser Dienst ist weiter gespeichert eingeschaltet, aber gesperrt und nicht nutzbar. Du kannst ihn jetzt ausschalten; einschalten geht erst wieder, wenn der Air Gap aus ist.")}</p>
  {/if}

  {#if keyLabel !== undefined}
    <div class="v-connector-key">
      <div class="v-row v-row-between">
        <label for={`key-${id}`} class="v-label">{t(keyLabel)}</label>
        <button type="button" class="v-link-btn" onclick={() => (showKey = !showKey)}>{t(showKey ? "Ausblenden" : "Anzeigen")}</button>
      </div>
      <!-- Der Schlüssel lässt sich auch bei Air Gap an eintragen: dabei geht nichts hinaus. -->
      <input
        id={`key-${id}`}
        type={showKey ? "text" : "password"}
        autocomplete="off"
        placeholder={keyPlaceholder}
        value={keyValue}
        onchange={(event) => onKey?.(event.currentTarget.value)}
        class="v-connector-mono"
      />
      <span data-hint class="v-help">{t("Der Schlüssel liegt verschlüsselt im Tresor und erscheint in keinem Protokoll.")}</span>
    </div>
  {/if}

  <div class="v-stack v-connector-test">
    <span class="v-label">{t("Verbindung testen")}</span>
    <div class="v-row v-connector-testrow">
      <input
        type="text"
        placeholder={t(testPlaceholder)}
        bind:value={query}
        disabled={testBlock !== null}
        aria-label={t(testPlaceholder)}
        onkeydown={(event) => { if (event.key === "Enter") void run(); }}
      />
      <button type="button" class="v-btn v-btn-primary" onclick={run} disabled={testBlock !== null || busy || !consent || !query.trim()}>
        {t(busy ? "Teste …" : "Testen")}
      </button>
    </div>
    <label class="v-row v-connector-consent">
      <input type="checkbox" bind:checked={consent} disabled={testBlock !== null} />
      <span class="v-help">{t(consentText)}</span>
    </label>
    {#if testBlock}<p class="v-help">{t(testBlock)}</p>{/if}
    {#if failure}<p class="v-notice warn" role="alert">{t(failure)}</p>{/if}

    {#if results.length > 0}
      <ul class="v-list v-connector-results">
        {#each results as res (res.url + res.title)}
          <li class="v-connector-result">
            <strong>{res.title}</strong>
            {#if textResult}
              <pre class="v-connector-report">{res.snippet}</pre>
            {:else}
              <p class="v-card-text">{res.snippet}</p>
              <span class="v-help v-connector-url">{res.url}</span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</section>

<style>
  .v-connector-head { display: flex; flex-wrap: wrap; align-items: flex-start; justify-content: space-between; gap: var(--v-space-4); }
  .v-connector-switch { display: inline-flex; align-items: center; gap: var(--v-space-3); }
  .v-row-between { justify-content: space-between; }
  .v-link-btn { border: 0; background: transparent; color: var(--v-text-muted); font-size: var(--v-text-xs); cursor: pointer; }
  .v-link-btn:hover { color: var(--v-text-primary); }
  .v-connector-key { display: grid; gap: var(--v-space-2); padding: var(--v-space-4); border: 1px solid var(--v-line); border-radius: var(--v-radius-control); background: var(--v-surface-2); }
  .v-connector-mono { width: 100%; font-family: var(--v-font-mono, monospace); font-size: var(--v-text-xs); }
  .v-connector-testrow input { flex: 1 1 12rem; min-width: 0; }
  .v-connector-testrow { gap: var(--v-space-3); flex-wrap: wrap; }
  .v-connector-consent { align-items: flex-start; gap: var(--v-space-2); }
  .v-connector-consent input { margin-top: 2px; }
  .v-connector-results { gap: var(--v-space-2); }
  .v-connector-result { display: grid; gap: 4px; padding: var(--v-space-3); border: 1px solid var(--v-line); border-radius: var(--v-radius-control); background: var(--v-surface-2); }
  .v-connector-result strong { color: var(--v-text-primary); font-size: var(--v-text-sm); }
  .v-connector-report { margin: 0; white-space: pre-wrap; font-family: inherit; color: var(--v-text-secondary); font-size: var(--v-text-sm); }
  .v-connector-url { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
