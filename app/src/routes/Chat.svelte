<script lang="ts">
  import { t, tk, locale } from "../lib/i18n/index.svelte";
  import { onDestroy, onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import {
    cancelStream,
    createConversation,
    deleteConversation,
    installedModels,
    listConversations,
    openConversation,
    renameConversation,
    selectModel,
    sendMessage,
    sendMessageWithTools,
    subscribeToStream,
    subscribeToToolStream,
    listInstalledSkills,
    listProjects,
    assignConversation,
    conversationSources,
    onChatSources,
    loadUiState,
  } from "../lib/ipc";
  import type {
    Conversation,
    AvailableModel,
    Message,
    SettingsSnapshot,
    StreamEvent,
    ServerTimingsDto,
    ToolStreamEvent,
    InstalledSkillView,
    Route,
    Project,
    MessageSource,
  } from "../lib/types";
  import Markdown from "../lib/components/Markdown.svelte";
  import InfoTooltip from "../lib/components/InfoTooltip.svelte";
  import PromptBar from "../lib/components/PromptBar.svelte";
  import MessageActions from "../lib/components/MessageActions.svelte";
  import type { ThinkingLevel, SlashItem } from "../lib/components/PromptBar.svelte";
  import { applyTemplate, commandName, loadTemplates, type PromptTemplate } from "../lib/templates";
  import StatusLine from "../lib/components/StatusLine.svelte";
  import ConversationList from "../lib/components/ConversationList.svelte";
  import ChatWelcome from "../lib/components/ChatWelcome.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import ProjectDialog from "../lib/components/ProjectDialog.svelte";
  import DocumentBar from "../lib/components/DocumentBar.svelte";
  import SourceChips from "../lib/components/SourceChips.svelte";
  import CompareBlock from "../lib/components/CompareBlock.svelte";
  import { appState } from "../lib/stores/app.svelte";
  import { conversation, setActive } from "../lib/stores/conversation.svelte";

  interface ToolTrace {
    kind: "call" | "result" | "error";
    tool: string;
    detail: string;
    is_untrusted: boolean;
  }

  interface Props {
    settings: SettingsSnapshot;
    /** Der Chat bleibt im Hintergrund erhalten; `false`, solange er nicht sichtbar ist. */
    active?: boolean;
    onOpenSettings: () => void;
    onOpenFiles: () => void;
    onSettingsChanged: (snapshot: SettingsSnapshot) => void;
    onOpenRoute: (route: Route) => void;
  }
  let { settings, active = true, onOpenSettings, onOpenFiles, onSettingsChanged, onOpenRoute }: Props = $props();

  // „/“-Menü: eingebaute Befehle, eigene Vorlagen und installierte Skills.
  const BUILTIN_COMMANDS: SlashItem[] = [
    { id: "cmd:neu", group: "command", name: tk("/neu"), description: tk("Neue Unterhaltung beginnen") },
    { id: "cmd:modell", group: "command", name: tk("/modell"), description: tk("Lokales Modell wechseln") },
    { id: "cmd:denken", group: "command", name: tk("/denken"), description: tk("Denkstufe einstellen") },
    { id: "cmd:dokument", group: "command", name: tk("/dokument"), description: tk("Dokument an die Unterhaltung hängen") },
    { id: "cmd:sprache", group: "command", name: tk("/sprache"), description: tk("Sprache der Oberfläche wechseln") },
    { id: "cmd:vorlagen", group: "command", name: tk("/vorlagen"), description: tk("Vorlagen verwalten") },
  ];
  // Projekte (Feature 5): `undefined` = Dialog zu, `null` = neues Projekt.
  let projects = $state<Project[]>([]);
  let editingProject = $state<Project | null | undefined>(undefined);
  // Dokumente und Quellen (Feature 1).
  let docPickerOpen = $state(false);
  let sources = $state<Record<string, MessageSource[]>>({});
  let unlistenSources: UnlistenFn | null = null;
  // Optionaler Modellvergleich (Feature 7); abschaltbar in den Einstellungen.
  let compareEnabled = $state(false);
  let comparing = $state(false);
  const currentProject = $derived(
    projects.find((project) => project.id === conversations.find((conversation) => conversation.id === currentId)?.project_id) ?? null,
  );
  let templates = $state<PromptTemplate[]>([]);
  let skills = $state<InstalledSkillView[]>([]);
  const slashItems = $derived<SlashItem[]>([
    ...BUILTIN_COMMANDS.map((item) => ({ ...item, name: t(item.name), description: t(item.description) })),
    ...templates.map((template) => ({ id: "tpl:" + template.id, group: "template" as const, name: commandName(template.name), description: template.text.replace(/\s+/g, " ").slice(0, 90) })),
    ...skills.map((skill) => ({
      id: "skill:" + skill.id,
      group: "skill" as const,
      name: "/" + skill.id,
      description: skill.name + " · " + (skill.kind === "wasm" ? t("Programm") : t("Anleitung")) + (skill.description ? " · " + skill.description : ""),
    })),
  ]);

  function handleSlash(item: SlashItem) {
    if (item.id.startsWith("tpl:")) {
      const template = templates.find((entry) => "tpl:" + entry.id === item.id);
      if (!template) return;
      const applied = applyTemplate(template.text);
      usePrompt(applied.value, applied.cursor);
      return;
    }
    if (item.id.startsWith("skill:")) { usePrompt(item.name + " "); return; }
    input = "";
    if (item.id === "cmd:neu") void startNew();
    else if (item.id === "cmd:dokument") docPickerOpen = true;
    else if (item.id === "cmd:sprache") onOpenRoute("settings");
    else if (item.id === "cmd:vorlagen") onOpenRoute("templates");
  }

  /**
   * Erkennt „/skill-id Auftrag“. Der Text bleibt, wie getippt, damit der Verlauf ihn so zeigt;
   * der Skill reist als eigenes Feld mit und wirkt nur auf diesen Zug.
   */
  function skillRequest(text: string): { skill: InstalledSkillView; rest: string } | null {
    const match = /^\/([A-Za-z0-9_-]+)\s+([\s\S]+)$/.exec(text.trim());
    if (!match) return null;
    const skill = skills.find((entry) => entry.id === match[1]);
    return skill ? { skill, rest: match[2] } : null;
  }

  let models = $state<AvailableModel[]>([]);
  let modelBusy = $state(false);

  let conversations = $state<Conversation[]>([]);
  let currentId = $state<string | null>(null);
  let messages = $state<Message[]>([]);
  let input = $state("");
  let historyOpen = $state(false);
  let presentationMode = $state(false);
  let presentationIndex = $state(0);

  function presentOnTop(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
  }

  function togglePresentation() {
    presentationMode = !presentationMode;
    if (presentationMode) {
      presentationIndex = Math.max(0, messages.length - 1);
      document.body.style.overflow = "hidden";
    } else {
      document.body.style.overflow = "";
    }
  }

  function presentationKey(e: KeyboardEvent) {
    if (!presentationMode) return;
    if (e.key === "Escape") { togglePresentation(); }
    if (e.key === "ArrowRight" || e.key === " ") {
      e.preventDefault();
      presentationIndex = Math.min(messages.length - 1, presentationIndex + 1);
    }
    if (e.key === "ArrowLeft") {
      e.preventDefault();
      presentationIndex = Math.max(0, presentationIndex - 1);
    }
  }
  let phase = $state(t("Bereit"));
  let timings = $state<ServerTimingsDto | null>(null);
  let streaming = $state(false);
  let assistantId = $state<string | null>(null);
  let error = $state<unknown>(null);
  let unlisten: UnlistenFn | null = null;
  let unlistenTools: UnlistenFn | null = null;
  let toolsEnabled = $state<boolean>(false);
  let toolTraces = $state<Record<string, ToolTrace[]>>({});
  let thoughtStartedAt = $state<Record<string, number>>({});
  let thoughtDurations = $state<Record<string, number>>({});
  let waitingForPlanApproval = $state(false);

  type ChatMode = "normal" | "plan" | "agent";
  let mode = $state<ChatMode>("normal");
  let thinking = $state<ThinkingLevel>("standard");
  let activeThinking = $state<ThinkingLevel>("standard");
  const MODE_INFO: Record<ChatMode, { label: string; icon: string; tip: string }> = {
    normal: {
      label: "Normal",
      icon: "M4 5.5A2.5 2.5 0 0 1 6.5 3h11A2.5 2.5 0 0 1 20 5.5v9A2.5 2.5 0 0 1 17.5 17H9l-5 4v-4.5a2.5 2.5 0 0 1 0-1V5.5Z",
      tip: "Direkte Antwort ohne Werkzeuge oder Zwischenschritte. Am schnellsten.",
    },
    plan: {
      label: "Plan",
      icon: "M4 6h16M4 12h10M4 18h6",
      tip: "IAP skizziert erst einen Lösungsplan und wartet auf deine Bestätigung, bevor er ausführt.",
    },
    agent: {
      label: "Agent",
      icon: "M12 4a3 3 0 0 1 3 3v1h2a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2h-1v1a2 2 0 0 1-2 2h-1v2a2 2 0 0 1-4 0v-2H8a2 2 0 0 1-2-2v-1H5a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h2V7a3 3 0 0 1 3-3h2Z",
      tip: "Nutzt Werkzeuge und prüft die eigene Antwort in mehreren Schritten. Langsamer, aber gründlicher.",
    },
  };

  const contextUsed = $derived(() => {
    const total = messages.reduce((sum, m) => sum + m.content.length, 0);
    // Grobe Schätzung: 4 Zeichen ≈ 1 Token
    return Math.round(total / 4);
  });
  const contextPercent = $derived(
    Math.min(100, Math.round((contextUsed() / settings.context_tokens) * 100))
  );

  onMount(async () => {
    unlisten = await subscribeToStream(handleEvent);
    unlistenTools = await subscribeToToolStream(handleToolEvent);
    unlistenSources = await onChatSources((incoming) => {
      const grouped = { ...sources };
      for (const source of incoming) grouped[source.message_id] = [...(grouped[source.message_id] ?? []).filter((item) => item.number !== source.number), source];
      sources = grouped;
    });
    try { projects = await listProjects(); } catch (reason) { console.error(reason); }
    try { compareEnabled = (await loadUiState("ui.compare_models")) !== "off"; } catch (reason) { console.error(reason); }
    try { models = await installedModels(); } catch (reason) { error = reason; }
    try { templates = await loadTemplates(); } catch (reason) { console.error(reason); }
    try { skills = await listInstalledSkills(); } catch (reason) { console.error(reason); }
    await refresh();
  });

  // Der Chat bleibt im Hintergrund erhalten; beim Zurückkehren erscheinen neu importierte Skills im „/“-Menü.
  $effect(() => {
    if (!active) return;
    void listInstalledSkills().then((list) => { skills = list; }).catch((reason) => console.error(reason));
  });

  // Aufträge aus Befehlssuche oder Tagesstart: bestimmte Unterhaltung öffnen,
  // neue beginnen oder einen Text in die Eingabe legen.
  $effect(() => {
    const pending = appState.pendingConversationId;
    if (!pending) return;
    appState.pendingConversationId = null;
    if (pending === "new") void startNew();
    else void selectConversation(pending);
  });
  $effect(() => {
    const prompt = appState.pendingPrompt;
    if (prompt == null) return;
    appState.pendingPrompt = null;
    usePrompt(prompt);
  });

  // Gemeinsame Unterhaltung: Änderungen hier gehen ans Backend, Änderungen von
  // außen (Startseite, Bildschirm, Pet) öffnet der Chat ebenfalls.
  $effect(() => {
    const id = currentId;
    if (id !== null) void setActive(id);
  });
  $effect(() => {
    const shared = conversation.activeId;
    if (shared && shared !== currentId && !streaming) void selectConversation(shared, true);
  });
  // Beim Ausblenden die Präsentation beenden, damit kein Vollbild im Hintergrund bleibt.
  $effect(() => {
    if (!active && presentationMode) togglePresentation();
  });

  function usePrompt(prompt: string, cursor = prompt.length) {
    input = prompt;
    queueMicrotask(() => {
      const field = document.getElementById("iap-prompt") as HTMLTextAreaElement | null;
      field?.focus();
      field?.setSelectionRange(cursor, cursor);
    });
  }

  /** Schickt einen fertigen Auftrag ab, etwa die Tagesübersicht aus dem Tagesstart. */
  function sendNow(prompt: string) {
    input = prompt;
    void submit(new Event("submit"));
  }

  async function chooseModel(id: string) {
    if (modelBusy || streaming || id === settings.model_id) return;
    modelBusy = true;
    error = null;
    try {
      onSettingsChanged(await selectModel(id));
    } catch (reason) {
      error = reason;
    } finally {
      modelBusy = false;
    }
  }

  onDestroy(() => {
    unlistenSources?.();
    unlisten?.();
    unlistenTools?.();
    document.body.style.overflow = "";
  });

  async function refresh() {
    try {
      conversations = await listConversations();
      if (!currentId && conversations.length > 0) {
        // Die gemeinsame Unterhaltung (Startseite, Pet) hat Vorrang vor der neuesten.
        const shared = conversations.find((entry) => entry.id === conversation.activeId);
        await selectConversation((shared ?? conversations[0]).id);
      }
    } catch (reason) {
      error = reason;
    }
  }

  async function selectConversation(id: string, preservePlanState = false) {
    try {
      const detail = await openConversation(id);
      if (!preservePlanState) waitingForPlanApproval = false;
      currentId = detail.conversation.id;
      messages = detail.messages;
      void loadSources(detail.conversation.id);
      historyOpen = false;
      error = null;
    } catch (reason) {
      error = reason;
    }
  }

  async function startNew() {
    try {
      const id = await createConversation(t("Neue Unterhaltung"));
      await refresh();
      await selectConversation(id);
      historyOpen = false;
    } catch (reason) {
      error = reason;
    }
  }

  async function submit(event: Event) {
    event.preventDefault();
    if (streaming || comparing || input.trim().length === 0) return;
    error = null;
    activeThinking = thinking;
    phase = t("Sende Anfrage");
    timings = null;
    // Ein bestätigter Plan darf erst beim zweiten Senden Werkzeuge ausführen.
    const slashSkill = skillRequest(input);
    const rawContent = slashSkill?.rest ?? input;
    const planApproved = mode === "plan" && waitingForPlanApproval && rawContent.trim().toLocaleLowerCase("de-AT") === "ok";
    let content = rawContent;
    if (planApproved) {
      content = "Ich bestätige den zuletzt vorgeschlagenen Plan mit OK. Führe ihn jetzt aus und melde das Ergebnis.\n\n" + rawContent;
      waitingForPlanApproval = false;
    } else if (mode === "plan") {
      content = `Ich möchte, dass du erst einen Plan machst und auf mein "OK" wartest, bevor du ausführst.\n\nMeine Anfrage:\n${rawContent}`;
      waitingForPlanApproval = true;
    } else if (mode === "agent") {
      waitingForPlanApproval = false;
      content = `[Agent-Modus] Nutze bei Bedarf Werkzeuge und mehrere Denk-Schritte (Analyse → Vorschlag → Selbstkritik). Am Ende eine klare, geprüfte Antwort.\n\nMeine Anfrage:\n${rawContent}`;
    } else {
      waitingForPlanApproval = false;
    }
    input = "";
    // Werkzeuge im Agent-Mode automatisch aktivieren
    const useTools = (mode !== "plan" || planApproved) && (toolsEnabled || mode === "agent" || slashSkill?.skill.kind === "wasm");
    const skill_id = slashSkill?.skill.id;
    try {
      streaming = true;
      const returnedAssistantId = useTools
        ? await sendMessageWithTools({ conversation_id: currentId, content, thinking_level: thinking, skill_id })
        : await sendMessage({ conversation_id: currentId, content, thinking_level: thinking, skill_id });
      assistantId = returnedAssistantId;
      if (useTools) {
        // Neuer Trace-Slot; ältere Aufrufe des gleichen Turns überschreiben.
        toolTraces = { ...toolTraces, [returnedAssistantId]: [] };
      }
    } catch (reason) {
      error = reason;
      streaming = false;
      phase = t("Bereit");
      waitingForPlanApproval = false;
    }
  }

  function handleToolEvent(event: ToolStreamEvent) {
    const id = event.assistant_message_id;
    const current = toolTraces[id] ?? [];
    let next: ToolTrace[] = current;
    switch (event.kind) {
      case "tool_call":
        next = [
          ...current,
          {
            kind: "call",
            tool: event.tool,
            detail: event.arguments_json,
            is_untrusted: false,
          },
        ];
        phase = t("Werkzeug: {name}", { name: event.tool });
        break;
      case "tool_result":
        next = [
          ...current,
          {
            kind: "result",
            tool: event.tool,
            detail: event.content,
            is_untrusted: event.is_untrusted,
          },
        ];
        break;
      case "tool_error":
        next = [
          ...current,
          {
            kind: "error",
            tool: event.tool,
            detail: event.message,
            is_untrusted: false,
          },
        ];
        break;
      case "permission_requested":
        // Freigabedialog wird in Phase 3 mit synchronem Backend-Rendezvous
        // umgesetzt; im aktuellen Prototypen zeigen wir ihn nur als Trace.
        next = [
          ...current,
          {
            kind: "error",
            tool: event.tool,
            detail: `Bestätigung nötig: ${event.reason}`,
            is_untrusted: false,
          },
        ];
        break;
    }
    toolTraces = { ...toolTraces, [id]: next };
  }

  function handleEvent(event: StreamEvent) {
    switch (event.kind) {
      case "started":
        currentId = event.conversation_id;
        assistantId = event.assistant_message_id;
        thoughtStartedAt = { ...thoughtStartedAt, [event.assistant_message_id]: Date.now() };
        // Reload messages to include user + assistant placeholder.
        void selectConversation(event.conversation_id, true);
        phase = event.engine_restarted
          ? t("Modell neu gestartet, verarbeite Anfrage")
          : activeThinking === "kurz" ? t("Verarbeite Anfrage") : t("Modell denkt");
        break;
      case "delta":
        appendDelta(event.assistant_message_id, event.text);
        phase = t("Antworte");
        break;
      case "finished":
        thoughtDurations = { ...thoughtDurations, [event.assistant_message_id]: Date.now() - (thoughtStartedAt[event.assistant_message_id] ?? Date.now()) };
        timings = event.outcome.timings;
        if (timings.predicted_per_second != null) appState.tokensPerSecond = timings.predicted_per_second;
        if (event.outcome.aborted) waitingForPlanApproval = false;
        phase = t(event.outcome.aborted ? "Abgebrochen" : waitingForPlanApproval ? "Plan bereit, mit OK bestätigen" : "Fertig");
        streaming = false;
        // Reload conversation from vault to ensure content matches server state.
        if (currentId) void selectConversation(currentId, true);
        void refresh();
        break;
      case "failed":
        if (event.assistant_message_id) thoughtDurations = { ...thoughtDurations, [event.assistant_message_id]: Date.now() - (thoughtStartedAt[event.assistant_message_id] ?? Date.now()) };
        error = `${event.error_kind}: ${event.message}`;
        timings = event.timings;
        streaming = false;
        phase = t("Fehler");
        waitingForPlanApproval = false;
        if (currentId) void selectConversation(currentId, true);
        break;
    }
  }

  function appendDelta(id: string, text: string) {
    const existing = messages.findIndex((m) => m.id === id);
    if (existing >= 0) {
      messages[existing] = {
        ...messages[existing],
        content: messages[existing].content + text,
      };
    } else {
      messages = [
        ...messages,
        {
          id,
          conversation_id: currentId ?? "",
          position: messages.length,
          role: "assistant",
          content: text,
          status: "streaming",
          created_at_unix_ms: Date.now(),
        },
      ];
    }
    messages = [...messages];
  }

  async function abort() {
    try {
      await cancelStream();
    } catch (reason) {
      error = reason;
    }
  }

  async function loadSources(conversationId: string) {
    try {
      const grouped: Record<string, MessageSource[]> = {};
      for (const source of await conversationSources(conversationId)) (grouped[source.message_id] ??= []).push(source);
      sources = { ...sources, ...grouped };
    } catch (reason) { console.error(reason); }
  }

  /** Liefert die aktuelle Unterhaltung und legt bei Bedarf eine an, etwa beim ersten Dokument. */
  async function ensureConversation(): Promise<string> {
    if (currentId) return currentId;
    const id = await createConversation(t("Neue Unterhaltung"));
    await refresh();
    await selectConversation(id);
    return id;
  }

  async function moveConversation(conversationId: string, projectId: string | null) {
    try {
      await assignConversation(conversationId, projectId);
      conversations = await listConversations();
    } catch (reason) { error = reason; }
  }

  async function projectSaved() {
    editingProject = undefined;
    try {
      projects = await listProjects();
      conversations = await listConversations();
    } catch (reason) { error = reason; }
  }

  async function rename(id: string, title: string) {
    try {
      await renameConversation(id, title);
      await refresh();
    } catch (reason) {
      error = reason;
    }
  }

  async function remove(id: string) {
    if (!confirm(t("Konversation wirklich löschen?"))) return;
    try {
      await deleteConversation(id);
      if (currentId === id) {
        currentId = null;
        messages = [];
      }
      await refresh();
    } catch (reason) {
      error = reason;
    }
  }

</script>

<div class="iap-chat-layout">
<ConversationList {conversations} {projects} {currentId} open={historyOpen}
  onSelect={(id) => void selectConversation(id)} onNew={() => void startNew()}
  onRename={(id, title) => void rename(id, title)} onDelete={(id) => void remove(id)} onClose={() => (historyOpen = false)}
  onEditProject={(project) => (editingProject = project)} onMove={(conversationId, projectId) => void moveConversation(conversationId, projectId)} />

<section class="iap-chat">
  <header class="iap-chat-header">
    <button type="button" class="v-btn-icon iap-chat-list-toggle" onclick={() => (historyOpen = true)} aria-label={t("Unterhaltungen anzeigen")} title={t("Unterhaltungen")}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="M4 6h16M4 12h16M4 18h10"/></svg>
    </button>
    <div class="iap-chat-title">
      <h1>{conversations.find((conversation) => conversation.id === currentId)?.title ?? t("Neuer Chat")}</h1>
      {#if currentProject}<span class="iap-chat-project" title={t("Projekt")}>{currentProject.name}</span>{/if}
      <InfoTooltip
        title={t("So funktioniert der Chat")}
        badge="Lokal"
        description="Unterhaltungen bleiben im verschlüsselten Tresor auf dem Stick. Das Modell rechnet auf diesem PC."
        features={[tk("Normal: direkte Antwort"), tk("Planmodus: erst ein Vorschlag, dann dein OK"), tk("Agent: darf Werkzeuge nutzen, jede Aktion wird geprüft")]}
        tip="Enter sendet, Umschalt+Enter macht einen Zeilenumbruch. Strg+K öffnet die Suche."
      />
    </div>
    <div class="iap-chat-actions">
      <button type="button" class="v-btn v-btn-ghost" onclick={togglePresentation} disabled={messages.length === 0} title={t(messages.length === 0 ? "Erst verfügbar, wenn der Chat Nachrichten enthält" : "Unterhaltung als Präsentation zeigen")}>
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg>
        <span>{t("Präsentation")}</span>
      </button>
    </div>
  </header>

  <ol class="iap-messages" aria-label={t("Nachrichten")}>
    {#each messages as message (message.id)}
      {@const isUser = message.role === "user"}
      <li class:is-user={isUser} class="iap-message">
        <div class="iap-message-meta"><strong>{t(isUser ? "Du" : message.role === "assistant" ? "IAP" : "System")}</strong><time>{new Date(message.created_at_unix_ms).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" })}</time></div>
        <div class="iap-message-body">
          {#if message.role === "assistant" && (message.status === "streaming" || thoughtDurations[message.id] !== undefined)}
            <StatusLine working={message.status === "streaming"} startedAt={thoughtStartedAt[message.id]} durationMs={thoughtDurations[message.id]} steps={(toolTraces[message.id] ?? []).map((trace) => `${trace.tool}: ${t(trace.kind === "call" ? "Aufruf" : trace.kind === "error" ? "Fehler" : "Ergebnis")}`)} />
          {/if}
          {#if message.role === "assistant" && toolTraces[message.id]?.length}
            <details class="iap-tool-traces"><summary>{t("Werkzeugschritte ({n})", { n: toolTraces[message.id].length })}</summary><ol>{#each toolTraces[message.id] as trace}<li><strong>{trace.tool}</strong><span>{trace.kind}</span>{#if trace.is_untrusted}<em>{t("Nicht vertrauenswürdiger Inhalt")}</em>{/if}<pre>{trace.detail}</pre></li>{/each}</ol></details>
          {/if}
          {#if message.role === "assistant" && message.status === "streaming" && message.content.length === 0}
            <!-- Die StatusLine zeigt den laufenden Status bereits an. -->
          {:else if isUser}
            <div class="iap-user-text">{message.content}</div>
          {:else}
            <Markdown content={message.content} isStreaming={message.status === "streaming"} />
          {/if}
          {#if message.role === "assistant" && sources[message.id]?.length}<SourceChips sources={sources[message.id]} />{/if}
          {#if message.role === "assistant" && message.status === "complete" && message.content.trim()}<MessageActions content={message.content} />{/if}
          {#if message.role === "assistant" && message.status === "complete" && compareEnabled && models.length > 1}
            <CompareBlock question={messages[messages.findIndex((item) => item.id === message.id) - 1]?.content ?? ""} answer={message.content}
              currentModelId={settings.model_id} {models} {thinking} disabled={streaming || comparing} onBusy={(busy) => (comparing = busy)} />
          {/if}
        </div>
      </li>
    {/each}
    {#if streaming && !messages.some((message) => message.role === "assistant" && message.status === "streaming")}
      <li class="iap-message iap-pending"><div class="iap-message-meta"><strong>{t("IAP")}</strong></div><StatusLine working={true} /></li>
    {/if}
    {#if messages.length === 0 && !streaming}
      <li class="iap-empty-chat"><ChatWelcome onPick={usePrompt} onSend={sendNow} onOpenConversation={(id) => void selectConversation(id)} lastConversation={conversations.find((c) => c.id !== currentId) ?? null} /></li>
    {/if}
  </ol>

  <footer class="iap-composer-zone">
    {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}
    {#if streaming || waitingForPlanApproval || timings?.predicted_per_second != null}
      <div class="iap-chat-status" aria-live="polite">
        <span>{phase}</span>
        <span title={t("Geschätzte Kontextauslastung {n} %", { n: contextPercent })}>{contextPercent > 60 ? t("Kontext {n} % belegt", { n: contextPercent }) : ""}{timings?.predicted_per_second != null && !streaming ? `${contextPercent > 60 ? " · " : ""}${timings.predicted_per_second.toFixed(1)} Token/s` : ""}</span>
      </div>
    {/if}
    <DocumentBar conversationId={currentId} {ensureConversation} bind:pickerOpen={docPickerOpen} onError={(reason) => (error = reason)} onManage={() => onOpenRoute("import")} />
    <PromptBar onAttachDocument={() => (docPickerOpen = true)} bind:value={input} bind:mode bind:thinking bind:toolsEnabled busy={streaming} {models} modelBusy={modelBusy || comparing} modelId={settings.model_id} onSelectModel={chooseModel} onOpenSettings={onOpenSettings} onOpenFiles={onOpenFiles} onSend={submit} onStop={abort} {slashItems} onSlash={handleSlash} onDictate={(text) => { input = input ? `${input} ${text}` : text; }} />
  </footer>
</section>
</div>

<svelte:window onkeydown={presentationKey} />

{#if editingProject !== undefined}
  <ProjectDialog project={editingProject} onClose={() => (editingProject = undefined)} onSaved={() => void projectSaved()} />
{/if}

{#if presentationMode}
  <div use:presentOnTop class="iap-presentation" role="dialog" aria-modal="true" aria-label={t("Präsentation")}>
    <header class="iap-presentation-bar">
      <div class="iap-presentation-title">
        <small>{t("Präsentation")}</small>
        <strong>{conversations.find((c) => c.id === currentId)?.title ?? t("Chat")}</strong>
      </div>
      <div class="iap-presentation-meta">
        <span>{t("{n} von {total}", { n: presentationIndex + 1, total: messages.length })}</span>
        <span class="hint">{t("Pfeiltasten blättern, Esc beendet")}</span>
        <button class="v-btn v-btn-ghost" onclick={togglePresentation}>{t("Schließen")}</button>
      </div>
    </header>

    <div class="iap-presentation-body">
      {#if messages[presentationIndex]}
        {@const m = messages[presentationIndex]}
        <div class="w-full max-w-4xl">
          <div class="iap-presentation-role">{t(m.role === "user" ? "Du" : "IAP")}</div>
          {#if m.role === "assistant"}<div class="iap-presentation-copy"><Markdown content={m.content} /></div>{:else}<div class="iap-presentation-copy whitespace-pre-wrap">{m.content}</div>{/if}
        </div>
      {/if}
    </div>

    <footer class="iap-presentation-bar bottom">
      <button
        class="v-btn v-btn-ghost"
        onclick={() => (presentationIndex = Math.max(0, presentationIndex - 1))}
        disabled={presentationIndex === 0}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg>
        {t("Zurück")}
      </button>
      <div class="iap-presentation-dots">
        {#each messages as _, i}
          <button
            class:current={i === presentationIndex}
            style={`width: ${i === presentationIndex ? '2rem' : '0.5rem'}`}
            onclick={() => (presentationIndex = i)}
            aria-label={`Nachricht ${i + 1}`}
          ></button>
        {/each}
      </div>
      <button
        class="v-btn v-btn-primary"
        onclick={() => (presentationIndex = Math.min(messages.length - 1, presentationIndex + 1))}
        disabled={presentationIndex >= messages.length - 1}
      >
        {t("Weiter")}
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 18l6-6-6-6"/></svg>
      </button>
    </footer>
  </div>
{/if}
