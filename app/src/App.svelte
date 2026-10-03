<script lang="ts">
  import { t } from "./lib/i18n/index.svelte";
  import { errorText } from "./lib/errors";
  import { onMount, onDestroy } from "svelte";
  import { bootstrapStatus, settingsSnapshot, getConnectorConfig, updateConnectorConfig, installedModels, applySettings, loadUiState, saveUiState, getUiLanguage, setUiLanguage, onCloseRequested, cancelQuit, performanceReport, enterPetMode, onWindowMode, setPetHotkey } from "./lib/ipc";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import type { BootstrapStatus, SettingsSnapshot, ThemePreference, Route, ConnectorConfig, AvailableModel } from "./lib/types";
  import Unlock from "./routes/Unlock.svelte";
  import Chat from "./routes/Chat.svelte";
  import Home from "./routes/Home.svelte";
  import Flow from "./routes/Flow.svelte";
  import Settings from "./routes/Settings.svelte";
  import Memory from "./routes/Memory.svelte";
  import Pci from "./routes/Pci.svelte";
  import Files from "./routes/Files.svelte";
  import Logs from "./routes/Logs.svelte";
  import Rubric from "./routes/Rubric.svelte";
  import Code from "./routes/Code.svelte";
  import Calendar from "./routes/Calendar.svelte";
  import Skills from "./routes/Skills.svelte";
  import Templates from "./routes/Templates.svelte";
  import Updates from "./routes/Updates.svelte";
  import Focus from "./routes/Focus.svelte";
  import Scratch from "./routes/Scratch.svelte";
  import Voice from "./routes/Voice.svelte";
  import Digests from "./routes/Digests.svelte";
  import Palace from "./routes/Palace.svelte";
  import Import from "./routes/Import.svelte";
  import Connectors from "./routes/Connectors.svelte";
  import ToolsOverview from "./routes/ToolsOverview.svelte";
  import ActivityBar from "./lib/components/ActivityBar.svelte";
  import { initJobsStore } from "./lib/stores/jobs.svelte";
  import CommandPalette, { type PaletteCommand } from "./lib/components/CommandPalette.svelte";
  import ExitDialog from "./lib/components/ExitDialog.svelte";
  import CloseChoiceDialog from "./lib/components/CloseChoiceDialog.svelte";
  import Dock from "./lib/components/Dock.svelte";
  import SectionTabs from "./lib/components/SectionTabs.svelte";
  import ErrorNotice from "./lib/components/ErrorNotice.svelte";
  import Wordmark from "./lib/components/Wordmark.svelte";
  import ScanlineOverlay from "./lib/components/ScanlineOverlay.svelte";
  import AuroraBackdrop from "./lib/components/AuroraBackdrop.svelte";
  import Toast from "./lib/components/Toast.svelte";
  import { notify, type Notice } from "./lib/notifications";
  import logoUrl from "./lib/assets/iap-logo.png";
  import { pointerGlowSurfaces } from "./lib/actions/pointerGlow";
  import { DESTINATIONS, ROUTE_INDEX, ROUTE_LABELS, destinationOf, type DestinationId } from "./lib/navigation";
  import { appState, applyAppearance, applyFontScale, applyHelpMode, resolvedTheme, type EffectsLevel, type HelpMode } from "./lib/stores/app.svelte";
  import { initConversationStore } from "./lib/stores/conversation.svelte";
  import { initAvatarStore } from "./lib/stores/avatar.svelte";
  import { initThemeColors, themeColors } from "./lib/stores/themeColors.svelte";
  import { wavePalette } from "./lib/themeColors";
  import { initVoiceStore, voiceCommands } from "./lib/stores/voice.svelte";
  import type { VoiceCommand, WindowMode } from "./lib/types";
  import { LANGUAGES, setLanguage } from "./lib/i18n/index.svelte";

  let bootstrap = $state<BootstrapStatus | null>(null);
  let settings = $state<SettingsSnapshot | null>(null);
  let models = $state<AvailableModel[]>([]);
  let connectorConfig = $state<ConnectorConfig | null>(null);
  let airGapBusy = $state(false);
  let currentRoute = $state<Route>("home");
  // Frühere Ansichten für den Zurück-Knopf der Flow Version (nur für diese Sitzung).
  let routeHistory = $state<Route[]>([]);
  let flowTab = $state<"agent" | "workflows">("agent");
  // Der Chat bleibt nach dem ersten Öffnen im Hintergrund erhalten, damit Entwurf,
  // laufende Antwort und Verlauf beim Ansichtswechsel nicht verloren gehen.
  let chatMounted = $state(false);
  let toolsOverview = $state(false);
  let bootstrapError = $state<unknown>(null);
  let bootstrapLoading = $state(true);
  let isCommandPaletteOpen = $state(false);
  // Das Backend hält das Schließen an und fragt hier nach den Spuren auf dem PC.
  let isExitDialogOpen = $state(false);
  let unlistenClose: UnlistenFn | null = null;
  let unlistenMode: UnlistenFn | null = null;
  // Erster Hinweis beim Schließen: Pet oder vollständig beenden.
  let isCloseChoiceOpen = $state(false);
  let windowMode = $state<WindowMode>("main");
  let notices = $state<Notice[]>([]);
  let nextNoticeId = 1;
  // Merkt sich je Ziel die zuletzt geöffnete Unterseite (nur für diese Sitzung).
  const lastRouteOf: Partial<Record<DestinationId, Route>> = {};

  let destination = $derived(destinationOf(currentRoute));
  let modelName = $derived(models.find((model) => model.id === settings?.model_id)?.display_name ?? settings?.model_id ?? "");
  let shortModelName = $derived(modelName.replace(/(?:[\s-]+)(?:Instruct|Abliterated)(?:[\s-].*)?$/i, "").replace(/\s+Q\d+_[A-Za-z0-9_]+$/i, "").replace(/-/g, " "));
  // Der Schlüssel enthält die eigenen Farben, damit der animierte Hintergrund bei einer Änderung neu aufgebaut wird.
  let themeKey = $derived(`${appState.effects}-${resolvedTheme()}-${JSON.stringify(themeColors[resolvedTheme()])}`);
  let waveColors = $derived(wavePalette(resolvedTheme(), themeColors[resolvedTheme()]));

  function handleNotice(event: Event) {
    const detail = (event as CustomEvent<Omit<Notice, "id">>).detail;
    notices = [...notices.slice(-3), { ...detail, id: nextNoticeId++ }];
  }

  function openRoute(route: Route) {
    if (route !== currentRoute) routeHistory = [...routeHistory.slice(-19), currentRoute];
    currentRoute = route;
    if (route === "chat") chatMounted = true;
    toolsOverview = false;
    lastRouteOf[destinationOf(route).id] = route;
    if (route === "settings") void refreshSettings();
  }

  function openDestination(id: DestinationId) {
    const target = DESTINATIONS.find((entry) => entry.id === id);
    if (!target) return;
    if (id === "tools" && !lastRouteOf.tools) {
      currentRoute = target.routes[0];
      toolsOverview = true;
      return;
    }
    openRoute(lastRouteOf[id] ?? target.routes[0]);
  }

  function openFlow(tab: "agent" | "workflows" = flowTab) {
    flowTab = tab;
    openRoute("flow");
  }

  function goBack() {
    const previous = routeHistory[routeHistory.length - 1];
    if (!previous) return;
    routeHistory = routeHistory.slice(0, -1);
    currentRoute = previous;
    toolsOverview = false;
    if (previous === "chat") chatMounted = true;
  }

  // Sprachbefehle lösen nur diese harmlosen Ansichtswechsel aus.
  function runVoiceCommand(command: VoiceCommand) {
    if (command === "open_chat") openRoute("chat");
    else if (command === "open_flow") openFlow();
    else if (command === "open_workflows") openFlow("workflows");
    else openRoute("home");
  }
  voiceCommands.handler = runVoiceCommand;

  function handleGlobalKeyDown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      isCommandPaletteOpen = !isCommandPaletteOpen;
    }
    // Tastenkürzel für die Sprachaufnahme: startet oder beendet sie über den sichtbaren Knopf.
    if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.code === "Space") {
      e.preventDefault();
      window.dispatchEvent(new CustomEvent("iap-voice-toggle"));
    }
  }

  onDestroy(() => {
    window.removeEventListener("keydown", handleGlobalKeyDown);
    window.removeEventListener("iap-notify", handleNotice);
    unlistenClose?.();
    unlistenMode?.();
  });

  async function choosePet() {
    isCloseChoiceOpen = false;
    try { await saveUiState("ui.pet_hint_shown", "1"); } catch (error) { console.error(error); }
    try { await enterPetMode(); } catch (error) { notify("Pet konnte nicht gestartet werden", errorText(error), "error"); }
  }

  function chooseQuit() {
    isCloseChoiceOpen = false;
    isExitDialogOpen = true;
  }

  function cancelClose() {
    isCloseChoiceOpen = false;
    void cancelQuit();
  }

  onMount(async () => {
    window.addEventListener("keydown", handleGlobalKeyDown);
    window.addEventListener("iap-notify", handleNotice);
    try { setLanguage(await getUiLanguage()); } catch (error) { console.error(error); }
    try {
      unlistenClose = await onCloseRequested((kind) => {
        isCommandPaletteOpen = false;
        if (kind === "choice") isCloseChoiceOpen = true;
        else isExitDialogOpen = true;
      });
    } catch (error) { console.error(error); }
    try { unlistenMode = await onWindowMode((mode) => { windowMode = mode; }); } catch (error) { console.error(error); }
    try {
      bootstrap = await bootstrapStatus();
      // Auf schwacher Hardware startet IAP ruhig; nach dem Entsperren gilt die gespeicherte Wahl.
      applyAppearance("system", bootstrap.plan.tier === "t0" || bootstrap.plan.tier === "unsupported" ? "calm" : "full");
      try { connectorConfig = await getConnectorConfig(); } catch (e) { connectorConfig = null; console.error(e); }
    } catch (error) {
      bootstrapError = error;
    } finally {
      bootstrapLoading = false;
    }
  });

  async function handleUnlocked(snapshot: SettingsSnapshot) {
    settings = snapshot;
    // Nach dem Entsperren beginnt IAP auf der Avatar-Startseite.
    routeHistory = [];
    currentRoute = "home";
    void initConversationStore();
    void initAvatarStore();
    void initThemeColors();
    void initJobsStore();
    void initVoiceStore();
    let effects: EffectsLevel = appState.effects;
    try {
      const saved = await loadUiState("ui.effects");
      if (saved === "full" || saved === "calm") effects = saved;
    } catch (error) { console.error(error); }
    applyAppearance(snapshot.theme, effects);
    try {
      const help = await loadUiState("ui.help_mode");
      applyHelpMode(help === "compact" ? "compact" : "guided");
    } catch (error) { console.error(error); }
    try { applyFontScale(Number(await loadUiState("ui.font_scale"))); } catch (error) { console.error(error); }
    // Das Tastenkürzel des Pets gilt nur, wenn es der Nutzer eingeschaltet hat.
    try {
      if ((await loadUiState("ui.pet_hotkey")) === "on") await setPetHotkey(true);
    } catch (error) { console.error(error); }
    try { models = await installedModels(); } catch (error) { console.error(error); }
    try { connectorConfig = await getConnectorConfig(); } catch (error) { connectorConfig = null; console.error(error); }
    // Neuer PC: einmal auf den Leistungs-Check hinweisen (misst nicht von selbst, um den Chat nicht zu blockieren).
    try {
      if ((await performanceReport()) === null) notify("Neuer PC erkannt", "Unter Einstellungen misst der Leistungs-Check, wie schnell IAP hier antwortet.", "info");
    } catch (error) { console.error(error); }
  }

  async function handleSettingsChanged(snapshot: SettingsSnapshot) {
    settings = snapshot;
    applyAppearance(snapshot.theme, appState.effects);
  }

  async function refreshSettings() {
    try {
      settings = await settingsSnapshot();
      if (settings) applyAppearance(settings.theme, appState.effects);
      connectorConfig = await getConnectorConfig();
    } catch (error) {
      console.error(error);
    }
  }

  async function setTheme(theme: ThemePreference) {
    applyAppearance(theme, appState.effects);
    try { settings = await applySettings({ theme }); } catch (error) { notify("Darstellung nicht gespeichert", errorText(error), "error"); }
  }

  // Nur den Arbeitsbereich scrollen. `scrollIntoView` würde auch die überlaufbegrenzte Hülle der App
  // verschieben und unten einen leeren Streifen hinterlassen.
  function scrollWorkspaceTo(id: string) {
    const target = document.getElementById(id);
    const box = document.querySelector<HTMLElement>(".iap-workspace");
    if (!target || !box) return;
    box.scrollTo({ top: box.scrollTop + target.getBoundingClientRect().top - box.getBoundingClientRect().top - 12 });
  }

  async function setHelpMode(mode: HelpMode) {
    applyHelpMode(mode);
    try { await saveUiState("ui.help_mode", mode); } catch (error) { notify("Hilfetexte nicht gespeichert", errorText(error), "error"); }
  }

  async function setEffects(effects: EffectsLevel) {
    applyAppearance(appState.theme, effects);
    try { await saveUiState("ui.effects", effects); } catch (error) { notify("Effekt-Stufe nicht gespeichert", errorText(error), "error"); }
  }

  async function toggleAirGap(draft?: ConnectorConfig) {
    if (!connectorConfig || airGapBusy) return;
    airGapBusy = true;
    try {
      connectorConfig = await updateConnectorConfig({ ...(draft ?? connectorConfig), offline_mode: !connectorConfig.offline_mode });
      notify(connectorConfig.offline_mode ? t("Air Gap aktiviert") : t("Air Gap ausgeschaltet"), connectorConfig.offline_mode ? t("Konnektoren sind gesperrt.") : t("Nur Exa kann ins Internet, und nur mit Freigabe für einen Lauf."), "success");
    } catch (error) {
      notify("Air Gap konnte nicht wechseln", errorText(error), "error");
    } finally {
      airGapBusy = false;
    }
  }

  const ACTION_ICONS = {
    plus: "M12 5v14M5 12h14",
    model: "M4 5h16v11H4zM9 20h6M12 16v4",
    shield: "M12 2 4 5v6c0 5 3.2 8.7 8 11 4.8-2.3 8-6 8-11V5l-8-3Z",
    sun: "M12 4V2M12 22v-2M4 12H2M22 12h-2M5.6 5.6 4.2 4.2M19.8 19.8l-1.4-1.4M5.6 18.4l-1.4 1.4M19.8 4.2l-1.4 1.4M12 7a5 5 0 1 0 0 10 5 5 0 0 0 0-10Z",
    moon: "M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z",
    sparkle: "m12 3 1.6 5.4L19 10l-5.4 1.6L12 17l-1.6-5.4L5 10l5.4-1.6L12 3Z",
    globe: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18ZM3 12h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3Z",
  };

  // Sprache sofort umstellen und dauerhaft auf dem Stick speichern.
  async function changeLanguage(code: string) {
    setLanguage(code);
    try { setLanguage(await setUiLanguage(code)); } catch (error) { notify("Sprache nicht gespeichert", errorText(error), "error"); }
  }

  let commands = $derived<PaletteCommand[]>([
    { id: "act-new-chat", label: t("Neue Unterhaltung"), group: t("Aktionen"), icon: ACTION_ICONS.plus, keywords: "chat neu", run: () => { appState.pendingConversationId = "new"; openRoute("chat"); } },
    { id: "act-model", label: t("Modell und Antwortstil einstellen"), group: t("Aktionen"), icon: ACTION_ICONS.model, keywords: "modell temperatur kontext", run: () => openRoute("settings") },
    { id: "act-airgap", label: t(connectorConfig?.offline_mode ? "Air Gap ausschalten" : "Air Gap einschalten"), group: t("Aktionen"), icon: ACTION_ICONS.shield, keywords: "offline konnektoren", run: () => void toggleAirGap() },
    { id: "act-light", label: t("Helle Darstellung"), group: t("Aktionen"), icon: ACTION_ICONS.sun, keywords: "theme hell light", run: () => void setTheme("light") },
    { id: "act-dark", label: t("Dunkle Darstellung"), group: t("Aktionen"), icon: ACTION_ICONS.moon, keywords: "theme dunkel dark", run: () => void setTheme("dark") },
    { id: "act-system", label: t("Darstellung wie System"), group: t("Aktionen"), icon: ACTION_ICONS.sun, keywords: "theme system automatisch", run: () => void setTheme("system") },
    { id: "act-effects", label: t(appState.effects === "calm" ? "Effekte einschalten" : "Effekte beruhigen"), group: t("Aktionen"), icon: ACTION_ICONS.sparkle, keywords: "animation bewegung leistung", run: () => void setEffects(appState.effects === "calm" ? "full" : "calm") },
    { id: "act-new-event", label: t("Neuer Termin"), group: t("Aktionen"), icon: ACTION_ICONS.plus, keywords: "kalender termin", run: () => { appState.pendingCalendarAction = "event"; openRoute("calendar"); } },
    { id: "act-new-task", label: t("Neue Aufgabe"), group: t("Aktionen"), icon: ACTION_ICONS.plus, keywords: "kalender aufgabe todo", run: () => { appState.pendingCalendarAction = "task"; openRoute("calendar"); } },
    { id: "act-new-fact", label: t("Fakt anlegen"), group: t("Aktionen"), icon: ACTION_ICONS.plus, keywords: "gedächtnis merken fakten", run: () => openRoute("memory") },
    { id: "act-colors", label: t("Farben anpassen"), group: t("Aktionen"), icon: ACTION_ICONS.sun, keywords: "farbe theme hintergrund akzent", run: () => { openRoute("settings"); setTimeout(() => scrollWorkspaceTo("set-colors"), 120); } },
    { id: "act-help-mode", label: t(appState.helpMode === "compact" ? "Hilfetexte ausführlich zeigen" : "Hilfetexte kompakt zeigen"), group: t("Aktionen"), icon: ACTION_ICONS.sparkle, keywords: "hilfe erklärung text kompakt", run: () => void setHelpMode(appState.helpMode === "compact" ? "guided" : "compact") },
    { id: "act-pet", label: t("Als Pet anzeigen"), group: t("Aktionen"), icon: ACTION_ICONS.model, keywords: "pet maskottchen klein", run: () => void enterPetMode().catch((error) => notify("Pet konnte nicht gestartet werden", errorText(error), "error")) },
    { id: "act-pet-customize", label: t("Pet anpassen"), group: t("Aktionen"), icon: ACTION_ICONS.model, keywords: "pet form farbe größe tastenkürzel", run: () => { openRoute("settings"); setTimeout(() => scrollWorkspaceTo("set-pet"), 120); } },
    ...LANGUAGES.map((language) => ({ id: `lang-${language.id}`, label: `${t("Sprache")}: ${language.label}`, group: t("Aktionen"), icon: ACTION_ICONS.globe, keywords: "sprache language idioma langue 言語", run: () => void changeLanguage(language.id) })),
    ...ROUTE_INDEX.map((entry) => ({ id: `route-${entry.id}`, label: t(entry.label), group: t("Bereich: {name}", { name: t(entry.group) }), hint: t(entry.hint), icon: entry.icon, keywords: entry.label, run: () => openRoute(entry.id) })),
  ]);
</script>

<svelte:head><title>{t("IAP")}</title></svelte:head>

<main use:pointerGlowSurfaces class="iap-root">
  {#if bootstrapLoading}
    <section class="iap-start" aria-live="polite">
      {#key themeKey}<ScanlineOverlay scanlineStrength={0.035} />{/key}
      <div class="iap-start-wordmark"><Wordmark text="IAP" /></div>
      <p>{t("Lokales System wird vorbereitet")}</p>
    </section>
  {:else if bootstrapError}
    <section class="iap-start iap-start-error">
      <h1>{t("IAP konnte nicht starten")}</h1>
      <div class="iap-start-error-box"><ErrorNotice error={bootstrapError} onRetry={() => location.reload()} /></div>
    </section>
  {:else if bootstrap && !settings}
    <Unlock {bootstrap} onUnlocked={handleUnlocked} />
  {:else if bootstrap && settings}
    <div class="iap-app-shell">
      {#key themeKey}<AuroraBackdrop {...waveColors} speed={0.28} />{/key}
      <header class="iap-topbar">
        <div class="iap-brand"><img src={logoUrl} alt="" /><span class="iap-brand-wordmark">IAP</span></div>
        <button type="button" class="iap-model-status" onclick={() => openRoute("settings")} title={t("Modell-Einstellungen öffnen")}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><rect x="4" y="5" width="16" height="12" rx="2"/><path d="M9 20h6M12 17v3"/></svg>
          <span class="iap-model-name">{shortModelName || t("Lokales Modell")}</span>
          {#if appState.tokensPerSecond != null}<span class="iap-model-speed">{appState.tokensPerSecond.toFixed(1)} Token/s</span>{/if}
        </button>
        <button type="button" class="iap-flow-button" class:active={currentRoute === "flow"} aria-pressed={currentRoute === "flow"} onclick={() => openFlow()} title={t("Agent Flow und Workflows")}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="6" cy="6" r="2"/><circle cx="18" cy="6" r="2"/><circle cx="12" cy="18" r="2"/><path d="M6 8v2a4 4 0 0 0 4 4h4a4 4 0 0 0 4-4V8M12 14v2"/></svg>
          <span>{t("Flow Version")}</span>
        </button>
        <span class="iap-vault-status" title={t("Alle Daten liegen verschlüsselt auf dem Stick")}>
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><rect x="5" y="10" width="14" height="11" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3"/></svg>
          <span>{t("Tresor offen")}</span>
        </span>
        <div class="iap-topbar-right">
          <button type="button" class="iap-search-button" onclick={() => (isCommandPaletteOpen = true)}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></svg>
            <span>{t("Suchen")}</span><kbd>{t("Strg K")}</kbd>
          </button>
          <button type="button" class="iap-airgap-control" class:active={connectorConfig?.offline_mode} onclick={() => toggleAirGap()}
            disabled={connectorConfig === null || airGapBusy} aria-pressed={connectorConfig?.offline_mode ?? false}
            aria-label={t(connectorConfig === null ? "Air Gap: Status unbekannt" : connectorConfig.offline_mode ? "Air Gap aktiv. Konnektoren freigeben" : "Air Gap aus. Konnektoren sperren")}
            title={t(connectorConfig?.offline_mode ? "Air Gap aktiv: Konnektoren gesperrt" : "Air Gap aus: Exa nur mit Freigabe je Lauf")}>
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 2 4 5v6c0 5 3.2 8.7 8 11 4.8-2.3 8-6 8-11V5l-8-3Z"/><path d={connectorConfig?.offline_mode ? "m9 12 2 2 4-4" : "M8 12h8"}/></svg>
            <span>{t("Air Gap")}</span>
            <strong>{t(connectorConfig === null ? "Unbekannt" : connectorConfig.offline_mode ? "Ein" : "Aus")}</strong>
          </button>
        </div>
      </header>

      <ActivityBar hideChat={currentRoute === "chat"} />

      <div class="iap-body">
        <div class:chat-route={currentRoute === "chat"} class="iap-workspace" class:wide={currentRoute === "code"}>
          {#if chatMounted}
            <div class="iap-chat-host" hidden={currentRoute !== "chat"}>
              <Chat {settings} active={currentRoute === "chat"} onSettingsChanged={handleSettingsChanged} onOpenSettings={() => openRoute("settings")} onOpenFiles={() => openRoute("files")} onOpenRoute={openRoute} />
            </div>
          {/if}
          {#if currentRoute === "chat"}
            <!-- Chat bleibt im Hintergrund erhalten und ist oben eingeblendet. -->
          {:else if currentRoute === "home"}
            <div class="iap-module-frame"><Home paused={windowMode === "pet"} onOpenChat={() => openRoute("chat")} onOpenFlow={() => openFlow()} onQuit={() => { isCommandPaletteOpen = false; isExitDialogOpen = true; }} onOpenRoute={(route) => openRoute(route)} /></div>
          {:else if currentRoute === "flow"}
            <div class="iap-module-frame"><Flow tab={flowTab} canGoBack={routeHistory.length > 0} onTab={(tab) => (flowTab = tab)} onBack={goBack} onOpenConnectors={() => openRoute("connectors")} /></div>
          {:else}
            <div class="iap-module-frame">
              {#if destination.routes.length > 1}
                <SectionTabs label={t(destination.label)} active={toolsOverview ? "overview" : currentRoute}
                  items={[...(destination.id === "tools" ? [{ id: "overview", label: t("Übersicht") }] : []), ...destination.routes.map((route) => ({ id: route, label: t(ROUTE_LABELS[route]) }))]}
                  onSelect={(id) => { if (id === "overview") { toolsOverview = true; } else { openRoute(id as Route); } }} />
              {/if}
              {#if toolsOverview}<ToolsOverview onOpen={openRoute} />
              {:else if currentRoute === "connectors"}<Connectors configSnapshot={connectorConfig} onConfigChanged={(updated) => (connectorConfig = updated)} onToggleAirGap={toggleAirGap} {airGapBusy} />
              {:else if currentRoute === "focus"}<Focus />
              {:else if currentRoute === "rubric"}<Rubric />
              {:else if currentRoute === "code"}<Code {settings} onSettingsChanged={handleSettingsChanged} onOpenSettings={() => openRoute("settings")} />
              {:else if currentRoute === "calendar"}<Calendar airGap={connectorConfig?.offline_mode ?? true} {airGapBusy} onToggleAirGap={() => toggleAirGap()} />
              {:else if currentRoute === "scratch"}<Scratch />
              {:else if currentRoute === "voice"}<Voice />
              {:else if currentRoute === "import"}<Import />
              {:else if currentRoute === "digests"}<Digests />
              {:else if currentRoute === "palace"}<Palace />
              {:else if currentRoute === "templates"}<Templates />
              {:else if currentRoute === "skills"}<Skills />
              {:else if currentRoute === "updates"}<Updates />
              {:else if currentRoute === "memory"}<Memory />
              {:else if currentRoute === "pci"}<Pci />
              {:else if currentRoute === "files"}<Files />
              {:else if currentRoute === "logs"}<Logs />
              {:else}<Settings {settings} {bootstrap} effects={appState.effects} onEffectsChange={setEffects} onThemeChange={setTheme} onLanguageChange={changeLanguage} onSettingsChanged={handleSettingsChanged} />{/if}
            </div>
          {/if}
        </div>
        <Dock active={toolsOverview ? "tools" : destination.id} onSelect={openDestination} />
      </div>

      {#if notices.length > 0}
        <div class="iap-notice-stack" aria-label={t("Benachrichtigungen")}>
          {#each notices as notice (notice.id)}<Toast {notice} onClose={(id) => (notices = notices.filter((item) => item.id !== id))} />{/each}
        </div>
      {/if}
    </div>
  {/if}

  {#if settings}
    <CommandPalette bind:isOpen={isCommandPaletteOpen} {commands} onClose={() => (isCommandPaletteOpen = false)}
      onOpenConversation={(id) => { appState.pendingConversationId = id; openRoute("chat"); }}
      onOpenFacts={() => openRoute("memory")} />
  {/if}

  {#if isCloseChoiceOpen}<CloseChoiceDialog onPet={choosePet} onQuit={chooseQuit} onCancel={cancelClose} />{/if}
  {#if isExitDialogOpen}<ExitDialog onCancel={() => { isExitDialogOpen = false; void cancelQuit(); }} />{/if}
</main>
