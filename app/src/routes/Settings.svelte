<script lang="ts">
  import { t, tk, i18n, LANGUAGES, locale } from "../lib/i18n/index.svelte";
  import { applySettings, getModelSettings, getUserProfile, installedModels, saveModelSettings, switchVault, updateUserProfile, hostTraces, setAskOnExit, loadUiState, saveUiState } from "../lib/ipc";
  import type { HostTraceReport } from "../lib/types";
  import type {
    AvailableModel,
    BootstrapStatus,
    HardwareTier,
    ModelSettings,
    SettingsSnapshot,
    ThemePreference,
    TierOverrideChange,
    UserProfile,
  } from "../lib/types";
  import { onMount, untrack } from "svelte";
  import PetCustomizer from "../lib/components/PetCustomizer.svelte";
  import ThemeColorsCard from "../lib/components/ThemeColorsCard.svelte";
  import { initPetStyle } from "../lib/stores/petStyle.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import PerformanceCard from "../lib/components/PerformanceCard.svelte";
  import PasswordChangeCard from "../lib/components/PasswordChangeCard.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import Marquee from "../lib/components/Marquee.svelte";
  import { notify } from "../lib/notifications";
  import { errorText } from "../lib/errors";
  import { appState, applyFontScale, applyHelpMode, FONT_SCALES, type EffectsLevel, type HelpMode } from "../lib/stores/app.svelte";

  interface Props {
    settings: SettingsSnapshot;
    bootstrap: BootstrapStatus;
    effects: EffectsLevel;
    onEffectsChange: (effects: EffectsLevel) => void;
    onThemeChange: (theme: ThemePreference) => void;
    onLanguageChange: (code: string) => void;
    onSettingsChanged: (snapshot: SettingsSnapshot) => void;
  }
  let { settings, bootstrap, effects, onEffectsChange, onThemeChange, onLanguageChange, onSettingsChanged }: Props = $props();

  let models = $state<AvailableModel[]>([]);
  let tierChoice = $state<"auto" | HardwareTier>(untrack(() => settings.tier_override ?? "auto"));
  let modelChoice = $state<string>(untrack(() => settings.model_id));
  let modelSettings = $state<ModelSettings | null>(null);
  let vaultPath = $state<string>(untrack(() => settings.vault_path));
  let vaultPassphrase = $state<string>("");
  let vaultCreateIfMissing = $state<boolean>(false);
  let busy = $state<boolean>(false);
  let error = $state<unknown>(null);

  let profile = $state<UserProfile>({ enabled: false, user_name: "", user_about: "", custom_system_prompt: "" });
  let profileSaving = $state(false);

  const TIER_LABEL: Record<HardwareTier, string> = {
    unsupported: tk("Nicht unterstützt"),
    t0: tk("Sparsam (8 GB RAM, nur Prozessor)"),
    t1: tk("Standard (16 GB RAM, nur Prozessor)"),
    t2: tk("Erweitert (16 GB RAM mit Grafikchip)"),
    t3: tk("Stark (eigene Grafikkarte)"),
  };

  type ModelPreset = "focused" | "balanced" | "creative";
  const modelPresets: { id: ModelPreset; title: string; description: string }[] = [
    { id: "focused", title: tk("Fokussiert"), description: tk("Gleichmäßige Antworten für Analyse und Code") },
    { id: "balanced", title: tk("Ausgewogen"), description: tk("Empfohlene Werte des Modells") },
    { id: "creative", title: tk("Ideenreich"), description: tk("Mehr Abwechslung beim Schreiben") },
  ];

  function presetValues(preset: ModelPreset): ModelSettings {
    const isLlama = models.find((model) => model.id === modelChoice)?.family.startsWith("llama") ?? false;
    const sampling = preset === "focused"
      ? { temperature: 0.2, top_p: 0.8 }
      : preset === "creative"
        ? { temperature: 0.9, top_p: 0.95 }
        : isLlama ? { temperature: 0.4, top_p: 0.85 } : { temperature: 0.7, top_p: 1.0 };
    return { model_id: modelChoice, context_tokens: null, ...sampling };
  }

  const selectedPreset = $derived(modelSettings?.context_tokens === null
    ? modelPresets.find(({ id }) => {
        const values = presetValues(id);
        return values.temperature === modelSettings?.temperature && values.top_p === modelSettings?.top_p;
      })?.id ?? null
    : null);

  onMount(async () => {
    void initPetStyle();
    try {
      models = await installedModels();
      modelSettings = await getModelSettings(modelChoice);
    } catch (reason) {
      error = reason;
    }
    try { profile = await getUserProfile(); } catch { /* neuer oder leerer Vault */ }
    try { traces = await hostTraces(); } catch (reason) { console.error(reason); }
    try { compareEnabled = (await loadUiState("ui.compare_models")) !== "off"; } catch (reason) { console.error(reason); }
  });

  // Optionale Funktion „Modelle vergleichen“ (lädt kurz ein zweites Modell).
  let compareEnabled = $state(true);
  async function toggleCompare() {
    const next = !compareEnabled;
    try {
      await saveUiState("ui.compare_models", next ? "on" : "off");
      compareEnabled = next;
    } catch (reason) { error = reason; }
  }

  // Spuren auf diesem PC: Anzeige und die Nachfrage beim Beenden (je PC im Tresor gespeichert).
  let traces = $state<HostTraceReport | null>(null);
  const TRACE_LABELS: Record<string, string> = {
    model_cache: tk("Modellkopie für einen schnelleren Start"),
    vault_hot: tk("Arbeitskopie des verschlüsselten Tresors"),
    legacy_webview: tk("Browserdaten älterer IAP-Versionen"),
  };

  function formatSize(bytes: number): string {
    const units = ["B", "KB", "MB", "GB"];
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
    return `${value.toLocaleString(locale(), { maximumFractionDigits: unit >= 2 ? 1 : 0 })} ${units[unit]}`;
  }

  async function toggleAskOnExit() {
    if (!traces) return;
    const next = !traces.ask_on_exit;
    try {
      await setAskOnExit(next);
      traces = { ...traces, ask_on_exit: next };
    } catch (reason) { error = reason; }
  }

  async function loadModelSettings() {
    error = null;
    modelSettings = null;
    try { modelSettings = await getModelSettings(modelChoice); } catch (reason) { error = reason; }
  }


  async function saveModel(event: SubmitEvent) {
    event.preventDefault();
    error = null;
    busy = true;
    try {
      const tier_change: TierOverrideChange | null = tierChoice === (settings.tier_override ?? "auto")
        ? null : tierChoice === "auto" ? { op: "clear" } : { op: "set", tier: tierChoice };
      if (modelSettings) await saveModelSettings(modelSettings);
      onSettingsChanged(await applySettings({ tier_override: tier_change, model_id: null, context_tokens: null, theme: null }));
      notify("Gespeichert", "Modell und Antwortstil sind übernommen. Das Modell wurde bei Bedarf neu geladen.", "success");
    } catch (reason) {
      error = reason;
    } finally {
      busy = false;
    }
  }

  async function saveProfile() {
    error = null;
    profileSaving = true;
    try {
      await updateUserProfile(profile);
      notify("Profil gespeichert", "Deine Angaben liegen verschlüsselt im Tresor.", "success");
    } catch (reason) {
      error = reason;
    } finally {
      profileSaving = false;
    }
  }

  async function toggleProfileEnabled() {
    profile.enabled = !profile.enabled;
    await saveProfile();
  }

  async function changeVault(event: SubmitEvent) {
    event.preventDefault();
    error = null;
    if (!vaultPath || !vaultPassphrase) {
      error = t("Bitte Ordner und Passwort des Tresors angeben.");
      return;
    }
    busy = true;
    try {
      const snapshot = await switchVault({ new_vault_path: vaultPath, passphrase: vaultPassphrase, create_if_missing: vaultCreateIfMissing });
      vaultPassphrase = "";
      onSettingsChanged(snapshot);
      modelChoice = snapshot.model_id;
      modelSettings = await getModelSettings(modelChoice);
      profile = await getUserProfile();
      notify("Tresor gewechselt", "Der andere Tresor ist jetzt geöffnet.", "success");
    } catch (reason) {
      error = reason;
    } finally {
      busy = false;
    }
  }

  async function changeHelpMode(mode: HelpMode) {
    applyHelpMode(mode);
    try { await saveUiState("ui.help_mode", mode); } catch (reason) { notify("Hilfetexte nicht gespeichert", errorText(reason), "error"); }
  }

  async function changeFontScale(percent: number) {
    applyFontScale(percent);
    try { await saveUiState("ui.font_scale", String(percent)); } catch (reason) { notify("Schriftgröße nicht gespeichert", errorText(reason), "error"); }
  }
</script>

<div class="v-page v-settings">
  <PageHeader title={t("Einstellungen")} description="Darstellung, Modell, Profil und Tresor. Alles bleibt verschlüsselt auf dem Stick." />

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  <section class="v-card v-stack" aria-labelledby="set-look">
    <h2 id="set-look" class="v-card-title">{t("Darstellung")}</h2>
    <div class="v-setting-row">
      <div><span class="v-label">{t("Sprache")}</span><p data-hint class="v-help">{t("Gilt auch für die Antworten.")}</p></div>
      <div class="v-segmented" role="group" aria-label={t("Sprache")}>
        {#each LANGUAGES as language (language.id)}
          <button type="button" lang={language.id} class:active={i18n.lang === language.id} aria-pressed={i18n.lang === language.id} title={language.label} onclick={() => onLanguageChange(language.id)}>{language.label}</button>
        {/each}
      </div>
    </div>
    <div class="v-setting-row">
      <div><span class="v-label">{t("Farbschema")}</span><p data-hint class="v-help">{t("„System“ folgt Windows.")}</p></div>
      <div class="v-segmented" role="group" aria-label={t("Farbschema")}>
        {#each [["system", tk("System")], ["light", tk("Hell")], ["dark", tk("Dunkel")]] as [value, label] (value)}
          <button type="button" class:active={appState.theme === value} aria-pressed={appState.theme === value} onclick={() => onThemeChange(value as ThemePreference)}>{t(label)}</button>
        {/each}
      </div>
    </div>
    <div class="v-setting-row">
      <div><span class="v-label">{t("Effekte")}</span><p data-hint class="v-help">{t("„Ruhig“ schaltet Bewegung ab und spart Leistung.")}</p></div>
      <div class="v-segmented" role="group" aria-label={t("Effekte")}>
        <button type="button" class:active={effects === "full"} aria-pressed={effects === "full"} onclick={() => onEffectsChange("full")}>{t("Voll")}</button>
        <button type="button" class:active={effects === "calm"} aria-pressed={effects === "calm"} onclick={() => onEffectsChange("calm")}>{t("Ruhig")}</button>
      </div>
    </div>
    <div class="v-setting-row">
      <div><span class="v-label">{t("Schriftgröße")}</span></div>
      <div class="v-segmented" role="group" aria-label={t("Schriftgröße")}>
        {#each FONT_SCALES as percent (percent)}
          <button type="button" class:active={appState.fontScale === percent} aria-pressed={appState.fontScale === percent} onclick={() => changeFontScale(percent)}>{percent} %</button>
        {/each}
      </div>
    </div>
    <div class="v-setting-row">
      <div><span class="v-label">{t("Hilfetexte")}</span><p class="v-help">{t("„Kompakt“ blendet Erklärsätze aus. Warnungen und Hinweise zum Datenschutz bleiben.")}</p></div>
      <div class="v-segmented" role="group" aria-label={t("Hilfetexte")}>
        {#each [["guided", tk("Ausführlich")], ["compact", tk("Kompakt")]] as [value, label] (value)}
          <button type="button" class:active={appState.helpMode === value} aria-pressed={appState.helpMode === value} onclick={() => changeHelpMode(value as HelpMode)}>{t(label)}</button>
        {/each}
      </div>
    </div>
  </section>

  <ThemeColorsCard />

  <section class="v-card v-stack" aria-labelledby="set-pet">
    <h2 id="set-pet" class="v-card-title">{t("Pet")}</h2>
    <PetCustomizer />
  </section>

  <form class="v-card v-stack" aria-labelledby="set-model" onsubmit={saveModel}>
    <h2 id="set-model" class="v-card-title">{t("Modell und Antwortstil")}</h2>
    <div class="v-grid-2">
      <label class="v-field"><span class="v-label">{t("Modell")}</span>
        <select bind:value={modelChoice} onchange={loadModelSettings}>
          {#each models as model (model.id)}<option value={model.id}>{model.display_name}{model.id === settings.model_id ? ` (${t("aktiv")})` : ""}</option>{/each}
        </select>
        <span data-hint class="v-help">{t("Das Modell wählst du in der Eingabeleiste des Chats.")}</span>
      </label>
      <label class="v-field"><span class="v-label">{t("Leistungsstufe")}</span>
        <select bind:value={tierChoice}>
          <option value="auto">{t("Automatisch (erkannt: {tier})", { tier: t(TIER_LABEL[bootstrap.plan.tier]) })}</option>
          {#each ["t0", "t1", "t2", "t3"] as tier (tier)}<option value={tier}>{t(TIER_LABEL[tier as HardwareTier])}</option>{/each}
        </select>
      </label>
    </div>

    {#if models.length > 1}
      <div class="v-row v-compare-toggle">
        <div><span class="v-label">{t("Modelle vergleichen (optional)")}</span><p data-hint class="v-help">{t("Zeigt an jeder Antwort einen Knopf für dieselbe Frage mit einem anderen Modell. Das Laden kann eine Minute dauern.")}</p></div>
        <button type="button" role="switch" aria-checked={compareEnabled} aria-label={t("Modelle vergleichen (optional)")} onclick={toggleCompare} class="v-apple-switch" class:active={compareEnabled}><span class="v-apple-thumb"></span></button>
      </div>
    {/if}

    {#if modelSettings}
      <div class="v-field" role="group" aria-label={t("Antwortstil")}>
        <span class="v-label">{t("Antwortstil")}</span>
        <div class="v-presets">
          {#each modelPresets as preset (preset.id)}
            <button type="button" class="v-preset" class:active={selectedPreset === preset.id} aria-pressed={selectedPreset === preset.id} disabled={busy} onclick={() => (modelSettings = presetValues(preset.id))}>
              <strong>{t(preset.title)}</strong><span>{t(preset.description)}</span>
            </button>
          {/each}
        </div>
      </div>
      <details class="v-details">
        <summary>{t("Feinabstimmung für Fortgeschrittene")}</summary>
        <div class="v-grid-3">
          <label class="v-field"><span class="v-label">{t("Kontextlänge")}</span>
            <input type="number" min="0" max={models.find((model) => model.id === modelChoice)?.max_context_tokens ?? 131072} step="256" value={modelSettings.context_tokens ?? 0}
              oninput={(event) => { const value = Number(event.currentTarget.value); modelSettings!.context_tokens = value === 0 ? null : value; }} />
            <span data-hint class="v-help">{t("0 = automatisch passend zum Arbeitsspeicher.")}</span>
          </label>
          <label class="v-field"><span class="v-label">{t("Temperatur")}</span>
            <input type="number" min="0" max="2" step="0.05" bind:value={modelSettings.temperature} />
            <span class="v-help">{t("Niedriger = gleichmäßiger.")}</span>
          </label>
          <label class="v-field"><span class="v-label">{t("Top-p")}</span>
            <input type="number" min="0.01" max="1" step="0.01" bind:value={modelSettings.top_p} />
            <span class="v-help">{t("Begrenzt die Wortauswahl.")}</span>
          </label>
        </div>
      </details>
    {/if}
    <div class="v-row v-row-end"><button type="submit" disabled={busy} class="v-btn v-btn-primary">{t(busy ? "Speichere …" : "Übernehmen")}</button></div>
  </form>

  <section class="v-card v-stack" aria-labelledby="set-profile">
    <div class="v-card-title">
      <h2 id="set-profile" class="v-card-title-text">{t("Persönliches Profil")}</h2>
      <button type="button" role="switch" aria-checked={profile.enabled} aria-label={t("Persönliches Profil verwenden")} onclick={toggleProfileEnabled} class="v-apple-switch" class:active={profile.enabled}><span class="v-apple-thumb"></span></button>
    </div>
    <p class="v-card-text">{t(profile.enabled ? "IAP berücksichtigt diese Angaben in jeder Antwort." : "Ausgeschaltet. Einschalten, damit IAP deinen Namen und deine Wünsche kennt.")}</p>
    {#if profile.enabled}
      <label class="v-field"><span class="v-label">{t("Wie soll IAP dich nennen?")}</span>
        <input type="text" bind:value={profile.user_name} placeholder={t("Dein Vorname")} />
      </label>
      <label class="v-field"><span class="v-label">{t("Über dich")}</span>
        <textarea rows="2" bind:value={profile.user_about} placeholder={t("Z. B. Lehrerin für Mathematik, mag kurze Antworten.")}></textarea>
      </label>
      <label class="v-field"><span class="v-label">{t("Eigene Anweisungen")}</span>
        <textarea rows="3" bind:value={profile.custom_system_prompt} placeholder={t("Z. B. Antworte knapp und auf Deutsch.")}></textarea>
      </label>
      <div class="v-row v-row-end"><button type="button" class="v-btn v-btn-primary" onclick={saveProfile} disabled={profileSaving}>{t(profileSaving ? "Speichere …" : "Profil speichern")}</button></div>
    {/if}
  </section>

  <PerformanceCard {models} onError={(reason) => (error = reason)} />

  <section class="v-card v-stack" aria-labelledby="set-traces">
    <div class="v-card-title">
      <h2 id="set-traces" class="v-card-title-text">{t("Spuren auf diesem PC")}</h2>
      <button type="button" role="switch" aria-checked={traces?.ask_on_exit ?? true} aria-label={t("Beim Beenden nachfragen")} onclick={toggleAskOnExit} disabled={!traces} class="v-apple-switch" class:active={traces?.ask_on_exit ?? true}><span class="v-apple-thumb"></span></button>
    </div>
    <p class="v-card-text">{t(traces?.ask_on_exit === false ? "Eigener PC: IAP beendet sich ohne Nachfrage und behält die Modellkopie." : "Beim Beenden fragt IAP, ob diese Daten entfernt werden. Empfohlen auf fremden PCs.")}</p>
    {#if traces && traces.traces.length > 0}
      <ul class="v-list">
        {#each traces.traces as trace (trace.id)}<li><div class="v-list-main"><strong>{t(TRACE_LABELS[trace.id] ?? trace.id)}</strong><span class="v-help v-path">{trace.path}</span></div><span class="v-num v-help">{formatSize(trace.size_bytes)}</span></li>{/each}
      </ul>
    {:else if traces}
      <p class="v-help">{t("Auf diesem PC liegen keine Daten von IAP.")}</p>
    {/if}
  </section>

  <PasswordChangeCard />

  <details class="v-card v-details">
    <summary>{t("Anderen Tresor öffnen")}</summary>
    <form class="v-stack" onsubmit={changeVault}>
      <p class="v-card-text">{t("Der aktuelle Tresor wird vorher gesichert und geschlossen.")}</p>
      <label class="v-field"><span class="v-label">{t("Ordner des Tresors")}</span><input type="text" bind:value={vaultPath} /></label>
      <label class="v-field"><span class="v-label">{t("Passwort")}</span><input type="password" bind:value={vaultPassphrase} autocomplete="off" /></label>
      <label class="v-row v-label"><input type="checkbox" bind:checked={vaultCreateIfMissing} /> {t("Neu anlegen, falls er noch nicht existiert")}</label>
      <div class="v-row v-row-end"><button type="submit" disabled={busy} class="v-btn v-btn-ghost">{t("Tresor wechseln")}</button></div>
    </form>
  </details>

  <section class="v-card v-about" aria-labelledby="legal-heading">
    <Marquee texts={[t("IAP · Rechtliche Hinweise · Credits"), t("Lokal · Transparent · Ohne Telemetrie")]} velocity={18} />
    <h2 id="legal-heading" class="v-card-title">{t("Über IAP")}</h2>
    <p class="v-card-text">{t("IAP verarbeitet alles lokal. KI-Antworten können Fehler enthalten; prüfe wichtige Ergebnisse vor der Verwendung.")}</p>
    <details class="v-details">
      <summary>{t("Verwendete Projekte")}</summary>
      <p class="v-card-text">{t("Oberfläche: Tauri, Svelte. Inferenz: llama.cpp. Tresor: SQLCipher. Der Animationskern des Pets stammt von Bloub (MIT).")}</p>
    </details>
  </section>
</div>

<style>
  .v-settings { max-width: 52rem; margin-inline: auto; }
  .v-setting-row { display: flex; align-items: center; justify-content: space-between; gap: var(--v-space-5); }
  .v-setting-row + .v-setting-row { padding-top: var(--v-space-3); border-top: 1px solid var(--v-line); }
  .v-setting-row p { margin: 2px 0 0; max-width: 42ch; }
  .v-card-title-text { margin: 0; font-size: inherit; font-weight: inherit; }
  .v-presets { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: var(--v-space-2); }
  .v-preset { display: flex; flex-direction: column; gap: 2px; min-height: 4.5rem; padding: var(--v-space-3); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); background: rgb(var(--v-tint) / .03); color: var(--v-text-secondary); text-align: left; transition: border-color 150ms ease, background-color 150ms ease, transform 120ms var(--v-ease-out-strong); }
  .v-preset strong { color: var(--v-text-primary); font-size: var(--v-text-sm); }
  .v-preset span { color: var(--v-text-muted); font-size: var(--v-text-xs); line-height: 1.4; }
  .v-preset.active { border-color: color-mix(in oklab, var(--v-accent-blue) 55%, transparent); background: var(--v-accent-blue-soft); }
  .v-preset:active { transform: scale(.98); }
  .v-preset:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: 2px; }
  .v-about { overflow: hidden; padding-top: 0; }
  .v-about :global(.marquee) { margin: 0 calc(var(--v-space-5) * -1) var(--v-space-4); }
  @media (max-width: 640px) { .v-setting-row { flex-direction: column; align-items: flex-start; } .v-presets { grid-template-columns: 1fr; } }
  .v-path { overflow-wrap: anywhere; }
  .v-compare-toggle { justify-content: space-between; align-items: flex-start; gap: var(--v-space-4); }
  .v-compare-toggle p { margin: 2px 0 0; }
</style>
