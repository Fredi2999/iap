import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  JobInfo,
  PackStatus,
  ScreenAnswer,
  ScreenLookRequest,
  ScreenSources,
  ScreenStatus,
  ScreenTarget,
  Transcript,
  VoiceBenchmark,
  VoiceStateEvent,
  VoiceStatus,
  WindowMode,
  AuditEntryView,
  AuditFilter,
  AvailableModel,
  BootstrapStatus,
  Conversation,
  ConversationDetail,
  Fact,
  GitCommit,
  GitInfo,
  GitStatusEntry,
  PendingPermissionView,
  ToolPromptResponse,
  MailLogEntry,
  MailStatus,
  CalendarOverview,
  CalendarRef,
  CalendarSource,
  CalendarSyncReport,
  NewCalendarEvent,
  RemoteEvent,
  PciActivity,
  PciComputer,
  PciStatus,
  CodeRootsStatus,
  AgentEvent,
  AgentChange,
  AgentChangeContent,
  CommandPrompt,
  CodeRootCheck,
  MemoryExport,
  PetStyle,
  SystemStats,
  MemoryGraph,
  MemoryUpdateRequest,
  MemoryRetrieveResponse,
  MemoryUpsertRequest,
  ModelSettings,
  MonteCarloResult,
  QuestionnaireResponses,
  Rubric,
  RubricScoreSheet,
  RunMonteCarloRequest,
  SaveQuestionnaireResult,
  SendMessageRequest,
  SettingsSnapshot,
  SettingsUpdate,
  SnapshotView,
  StreamEvent,
  ToolStreamEvent,
  UnifiedDiff,
  UserProfile,
  HostTraceReport,
  Project,
  DocumentInfo,
  MessageSource,
  PerfReport,
  VaultInfo,
  VaultSwitchRequest,
  WeightedResult,
  WorkspaceListing,
  CodeAssistReply,
  CodeAssistRequest,
  CodeHit,
} from "./types";

// Dünner typisierter Wrapper um `invoke`, damit der Rest der App nur die
// serialisierbaren pa-types-DTOs sieht und Tauri-spezifische Namen an einer
// Stelle stehen.

export function bootstrapStatus(): Promise<BootstrapStatus> {
  return invoke("bootstrap_status");
}

export function listVaults(): Promise<VaultInfo[]> {
  return invoke("list_vaults");
}

export function unlockVault(
  passphrase: string,
  createIfMissing: boolean,
  recoverFromHost = false,
  vaultPath?: string | null,
): Promise<SettingsSnapshot> {
  return invoke("unlock_vault", {
    passphrase,
    createIfMissing,
    recoverFromHost,
    vaultPath: vaultPath ?? null,
  });
}

export function getUserProfile(): Promise<UserProfile> {
  return invoke("get_user_profile");
}

export function updateUserProfile(profile: UserProfile): Promise<UserProfile> {
  return invoke("update_user_profile", { profile });
}

export function listConversations(): Promise<Conversation[]> {
  return invoke("list_conversations");
}

export function openConversation(id: string): Promise<ConversationDetail> {
  return invoke("open_conversation", { conversationId: id });
}

export function createConversation(title: string): Promise<string> {
  return invoke("create_conversation", { title });
}

export function renameConversation(id: string, title: string): Promise<void> {
  return invoke("rename_conversation", { conversationId: id, title });
}

export function deleteConversation(id: string): Promise<void> {
  return invoke("delete_conversation", { conversationId: id });
}

export function installedModels(): Promise<AvailableModel[]> {
  return invoke("installed_models");
}

export function selectModel(modelId: string): Promise<SettingsSnapshot> {
  return invoke("select_model", { modelId });
}

export function getModelSettings(modelId: string): Promise<ModelSettings> {
  return invoke("get_model_settings", { modelId });
}

export function saveModelSettings(settings: ModelSettings): Promise<ModelSettings> {
  return invoke("save_model_settings", { settings });
}

export function settingsSnapshot(): Promise<SettingsSnapshot> {
  return invoke("settings_snapshot");
}

export function applySettings(update: SettingsUpdate): Promise<SettingsSnapshot> {
  return invoke("apply_settings", { update });
}

export function switchVault(request: VaultSwitchRequest): Promise<SettingsSnapshot> {
  return invoke("switch_vault", { request });
}

export function sendMessage(request: SendMessageRequest): Promise<string> {
  return invoke("send_message", { request });
}

export function sendMessageWithTools(request: SendMessageRequest): Promise<string> {
  return invoke("send_message_with_tools", { request });
}

export function subscribeToToolStream(
  callback: (event: ToolStreamEvent) => void,
): Promise<UnlistenFn> {
  return listen<ToolStreamEvent>("tool-stream", (message) => callback(message.payload));
}

export function cancelStream(): Promise<void> {
  return invoke("cancel_stream");
}

export function subscribeToStream(
  callback: (event: StreamEvent) => void,
): Promise<UnlistenFn> {
  return listen<StreamEvent>("chat-stream", (message) => callback(message.payload));
}

// --- Phase 2 -------------------------------------------------------------

export function retrieveMemory(query: string): Promise<MemoryRetrieveResponse> {
  return invoke("retrieve_memory", { query });
}

export function listActiveFacts(): Promise<Fact[]> {
  return invoke("list_active_facts");
}

export function upsertFact(request: MemoryUpsertRequest): Promise<Fact> {
  return invoke("upsert_fact", { request });
}

export function forgetFact(id: string): Promise<void> {
  return invoke("forget_fact", { factId: id });
}

export function memoryGraph(includeSuperseded: boolean): Promise<MemoryGraph> {
  return invoke("memory_graph", { includeSuperseded });
}

export function updateFact(request: MemoryUpdateRequest): Promise<Fact> {
  return invoke("update_fact", { request });
}

export function learnFromConversation(conversationId: string): Promise<Fact[]> {
  return invoke("learn_from_conversation", { conversationId });
}

export function learnCancel(): Promise<void> {
  return invoke("learn_cancel");
}

export function exportMemory(): Promise<MemoryExport> {
  return invoke("export_memory");
}

export function listAudit(filter: AuditFilter): Promise<AuditEntryView[]> {
  return invoke("list_audit", { filter });
}

export function listWorkspace(relativePath: string): Promise<WorkspaceListing> {
  return invoke("list_workspace", { relativePath });
}

export function readWorkspaceFile(relativePath: string): Promise<string> {
  return invoke("read_workspace_file", { relativePath });
}

// --- Rubrik-Engine (Konzept 4.2) ---

export function rubricDefault(): Promise<Rubric> {
  return invoke("rubric_default");
}

export function saveQuestionnaire(
  responses: QuestionnaireResponses,
): Promise<SaveQuestionnaireResult> {
  return invoke("save_questionnaire", { responses });
}

export function loadQuestionnaire(): Promise<QuestionnaireResponses> {
  return invoke("load_questionnaire");
}

export function evaluateRubric(sheet: RubricScoreSheet): Promise<WeightedResult> {
  return invoke("evaluate_rubric", { sheet });
}

export function runRubricMonteCarlo(
  request: RunMonteCarloRequest,
): Promise<MonteCarloResult> {
  return invoke("run_rubric_monte_carlo", { request });
}

// --- Code-Bereich (Konzept 9) ---

export function readCodeFile(relativePath: string): Promise<string> {
  return invoke("read_code_file", { relativePath });
}

export function writeCodeFile(relativePath: string, content: string): Promise<void> {
  return invoke("write_code_file", { relativePath, content });
}

export function diffCodeFile(relativePath: string, newContent: string): Promise<UnifiedDiff> {
  return invoke("diff_code_file", { relativePath, newContent });
}

export function applyHunks(
  relativePath: string,
  newContent: string,
  hunkIndices: number[],
): Promise<string> {
  return invoke("apply_hunks", { relativePath, newContent, hunkIndices });
}

export function codeList(relativePath: string): Promise<WorkspaceListing> {
  return invoke("code_list", { relativePath });
}

export function codeCreate(relativePath: string, isDirectory: boolean): Promise<void> {
  return invoke("code_create", { relativePath, isDirectory });
}

export function codeRename(from: string, to: string): Promise<void> {
  return invoke("code_rename", { from, to });
}

/** Verschiebt in den Papierkorb (.trash im Arbeitsordner) und liefert dessen Pfad. */
export function codeDelete(relativePath: string): Promise<string> {
  return invoke("code_delete", { relativePath });
}

export function codeSearch(query: string): Promise<CodeHit[]> {
  return invoke("code_search", { query });
}

export function codeAssist(request: CodeAssistRequest): Promise<CodeAssistReply> {
  return invoke("code_assist", { request });
}

export function codeAssistCancel(): Promise<void> {
  return invoke("code_assist_cancel");
}

/** Anzahl bisher erzeugter Zeichen der laufenden Code-Hilfe. */
export function onCodeAssistProgress(callback: (chars: number) => void): Promise<UnlistenFn> {
  return listen<number>("code-assist-progress", (message) => callback(message.payload));
}

export function snapshotCreate(): Promise<SnapshotView> {
  return invoke("snapshot_create");
}

export function snapshotList(): Promise<SnapshotView[]> {
  return invoke("snapshot_list");
}

export function snapshotRestore(snapshotId: string): Promise<void> {
  return invoke("snapshot_restore", { snapshotId });
}

export function snapshotDiscard(snapshotId: string): Promise<void> {
  return invoke("snapshot_discard", { snapshotId });
}

export function gitInfo(): Promise<GitInfo> {
  return invoke("git_info");
}

export function gitStatus(): Promise<GitStatusEntry[]> {
  return invoke("git_status");
}

export function gitLog(limit?: number): Promise<GitCommit[]> {
  return invoke("git_log", { limit });
}

export function gitCommit(message: string, paths: string[] = []): Promise<string> {
  return invoke("git_commit", { message, paths });
}

// --- Freigabe-Rendezvous ---

export function pendingPermissions(): Promise<PendingPermissionView[]> {
  return invoke("pending_permissions");
}

export function respondPermission(response: ToolPromptResponse): Promise<boolean> {
  return invoke("respond_permission", { response });
}

// --- Phase 4 (Scheduler/Skills/Updates/Export) ---
import type {
  BundleVerification,
  DryRunOutcome,
  IcsImportResult,
  InstalledSkillView,
  SchedulerEvent,
  SchedulerPlanRequest,
  SchedulerTask,
  SolvedPlan,
} from "./types";

export function schedulePlan(request: SchedulerPlanRequest): Promise<SolvedPlan> {
  return invoke("schedule_plan", { request });
}
export function importIcsText(text: string): Promise<IcsImportResult> {
  return invoke("import_ics_text", { text });
}
export function exportIcsText(events: SchedulerEvent[], tasks: SchedulerTask[]): Promise<string> {
  return invoke("export_ics_text", { events, tasks });
}
export function listInstalledSkills(): Promise<InstalledSkillView[]> {
  return invoke("list_installed_skills");
}
export function setInstructionSkillActive(id: string, active: boolean): Promise<void> {
  return invoke("set_instruction_skill_active", { id, active });
}
export function uninstallSkill(id: string, kind: "wasm" | "instructions"): Promise<void> {
  return invoke("uninstall_skill", { id, kind });
}
export function importSkillFromPath(sourcePath: string): Promise<InstalledSkillView> {
  return invoke("import_skill_from_path", { sourcePath });
}
export function skillDryRun(skillId: string, tool: string, argumentsJson: string): Promise<DryRunOutcome> {
  return invoke("skill_dry_run", { skillId, tool, argumentsJson });
}
export function verifyUpdateBundle(bundlePath: string): Promise<BundleVerification> {
  return invoke("verify_update_bundle", { bundlePath });
}
export function listBackups(): Promise<string[]> {
  return invoke("list_backups");
}
export function createFullExport(): Promise<string> {
  return invoke("create_full_export");
}

// ============================================================================
// Konnektoren: Exa und Air Gap
// ============================================================================
import type {
  ConnectorConfig,
  ExaSearchResult,
} from "./types";

export function getConnectorConfig(): Promise<ConnectorConfig> {
  return invoke("get_connector_config");
}

export function updateConnectorConfig(config: ConnectorConfig): Promise<ConnectorConfig> {
  return invoke("update_connector_config", { config });
}

/** Echter Einzeltest; `approved` ist die ausdrückliche Freigabe, den Suchtext an Exa zu senden. */
/** Einzeltest eines Konnektors: `exa`, `wikipedia`, `open_meteo` oder `brave`. */
export function testConnector(connector: string, query: string, approved: boolean): Promise<ExaSearchResult[]> {
  return invoke("test_connector", { connector, query, approved });
}

export function testExaSearch(query: string, approved: boolean): Promise<ExaSearchResult[]> {
  return invoke("test_exa_search", { query, approved });
}


// --- Oberflächen-Zustand im Vault (nur Schlüssel mit Präfix "ui.") ---------

/** Liest einen gespeicherten Oberflächenwert; `null`, wenn noch nichts gespeichert ist. */
export function loadUiState(key: string): Promise<string | null> {
  return invoke("load_ui_state", { key });
}

/** Speichert einen Oberflächenwert verschlüsselt im Vault des Sticks. */
export function saveUiState(key: string, value: string): Promise<void> {
  return invoke("save_ui_state", { key, value });
}

// --- Sprache (liegt auf dem Stick, vor dem Entsperren lesbar) -------------

/** Liest die gespeicherte Sprache der Oberfläche. */
export function getUiLanguage(): Promise<string> {
  return invoke("get_ui_language");
}

/** Speichert die Sprache und stellt die Modellantworten um. */
export function setUiLanguage(code: string): Promise<string> {
  return invoke("set_ui_language", { code });
}

// --- Spuren auf dem Host und Beenden --------------------------------------

/** Listet, was IAP auf diesem PC hinterlässt, und ob beim Beenden gefragt wird. */
export function hostTraces(): Promise<HostTraceReport> {
  return invoke("host_traces");
}

/** Merkt sich für diesen PC, ob beim Beenden nach den Spuren gefragt wird. */
export function setAskOnExit(ask: boolean): Promise<void> {
  return invoke("set_ask_on_exit", { ask });
}

/** Beendet IAP sauber; mit `purge` werden die Spuren auf diesem PC entfernt. */
export function quitApp(purge: boolean): Promise<void> {
  return invoke("quit_app", { purge });
}

/** Meldet, dass das Fenster geschlossen werden soll und die Nachfrage gezeigt werden muss. */
/**
 * Das Hauptfenster soll schließen. `choice`: erster Hinweis „Schließen ≠ Beenden“,
 * `quit`: „IAP vollständig beenden“ wurde gewählt (Nachfrage nach den Spuren).
 */
export function onCloseRequested(callback: (kind: "choice" | "quit") => void): Promise<UnlistenFn> {
  return listen<string | null>("iap-close-requested", (message) => callback(message.payload === "choice" ? "choice" : "quit"));
}

/** Wechselt bewusst zum kleinen Pet; das Hauptfenster wird ausgeblendet. */
export function enterPetMode(): Promise<void> {
  return invoke("enter_pet_mode");
}

/** Zeigt das Hauptfenster wieder („IAP öffnen“). */
export function showMainWindow(): Promise<void> {
  return invoke("show_main_window");
}

/** „IAP vollständig beenden“ aus dem Pet. */
export function requestQuit(): Promise<void> {
  return invoke("request_quit");
}

/** Klappt das Pet für Menü, Diktat und Antwort auf oder zu. */
export function petExpand(expanded: boolean, avatarSize?: number, rows?: number, bubble?: boolean): Promise<void> {
  return invoke("pet_expand", { expanded, avatarSize, rows, bubble });
}

export function getPetStyle(): Promise<PetStyle> {
  return invoke("get_pet_style");
}

export function setPetStyle(style: PetStyle): Promise<PetStyle> {
  return invoke("set_pet_style", { style });
}

export function systemStats(): Promise<SystemStats> {
  return invoke("system_stats");
}

export function onPetStyleChanged(callback: (style: PetStyle) => void): Promise<UnlistenFn> {
  return listen<PetStyle>("pet-style-changed", (message) => callback(message.payload));
}

/** Schaltet das globale Tastenkürzel (Strg+Alt+V) für das Schreibfeld des Pets; liefert, ob es danach aktiv ist. */
export function setPetHotkey(enabled: boolean): Promise<boolean> {
  return invoke("set_pet_hotkey", { enabled });
}

/** Das Tastenkürzel wurde gedrückt: Das Pet soll sein Schreibfeld öffnen. */
export function onPetOpenPanel(callback: () => void): Promise<UnlistenFn> {
  return listen("pet-open-panel", () => callback());
}

export function onWindowMode(callback: (mode: WindowMode) => void): Promise<UnlistenFn> {
  return listen<WindowMode>("window-mode", (message) => callback(message.payload));
}

/** Meldet einen Sprachwechsel aus dem Hauptfenster an andere Fenster (etwa das Pet). */
export function onUiLanguageChanged(callback: (code: string) => void): Promise<UnlistenFn> {
  return listen<string>("ui-language-changed", (message) => callback(message.payload));
}

/** Lässt den Nutzer auf einem Bildschirm einen Bereich aufziehen (nichts wird aufgenommen). */
export function screenPickRegion(monitor: number): Promise<ScreenTarget | null> {
  return invoke("screen_pick_region", { monitor });
}

/** Die Nachfrage beim Beenden wurde abgebrochen; das nächste Schließen fragt wieder. */
export function cancelQuit(): Promise<void> {
  return invoke("cancel_quit");
}

/** Führt ein Skill-Werkzeug in der Sandbox aus; liefert die JSON-Antwort als Text. */
export function runSkill(skillId: string, tool: string, argumentsJson: string): Promise<string> {
  return invoke("run_skill", { skillId, tool, argumentsJson });
}

// --- Projekte ---------------------------------------------------------------

export function listProjects(): Promise<Project[]> {
  return invoke("list_projects");
}

/** Legt ein Projekt an (ohne `id`) oder ändert es. */
export function saveProject(id: string | null, name: string, systemPrompt: string): Promise<Project> {
  return invoke("save_project", { id, name, systemPrompt });
}

export function deleteProject(id: string): Promise<void> {
  return invoke("delete_project", { id });
}

/** Ordnet eine Unterhaltung einem Projekt zu; `null` löst die Zuordnung. */
export function assignConversation(conversationId: string, projectId: string | null): Promise<void> {
  return invoke("assign_conversation", { conversationId, projectId });
}

// --- Dokumente und Quellen --------------------------------------------------

/** Schickt eine vom Nutzer gewählte Datei als Rohdaten; IAP öffnet selbst keinen Pfad. */
export async function importDocument(file: File): Promise<DocumentInfo> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  return invoke("import_document", bytes, { headers: { "x-file-name": encodeURIComponent(file.name) } });
}

export function listDocuments(): Promise<DocumentInfo[]> {
  return invoke("list_documents");
}

export function deleteDocument(id: string): Promise<void> {
  return invoke("delete_document", { id });
}

export function setDocumentAttached(conversationId: string, documentId: string, attached: boolean): Promise<void> {
  return invoke("set_document_attached", { conversationId, documentId, attached });
}

export function conversationDocuments(conversationId: string): Promise<DocumentInfo[]> {
  return invoke("conversation_documents", { conversationId });
}

export function conversationSources(conversationId: string): Promise<MessageSource[]> {
  return invoke("conversation_sources", { conversationId });
}

/** Quellen einer gerade gestarteten Antwort. */
export function onChatSources(callback: (sources: MessageSource[]) => void): Promise<UnlistenFn> {
  return listen<MessageSource[]>("chat-sources", (message) => callback(message.payload));
}

// --- Leistungs-Check --------------------------------------------------------

/** Letzte Messung dieses PCs oder `null`. */
export function performanceReport(): Promise<PerfReport | null> {
  return invoke("performance_report");
}

/** Misst das aktive Modell auf diesem PC (dauert einige Sekunden). */
export function runPerformanceCheck(): Promise<PerfReport> {
  return invoke("run_performance_check");
}

// --- Modelle vergleichen (optional) -------------------------------------------

export interface CompareAnswer {
  model_id: string;
  model_name: string;
  answer: string;
  tokens_per_second: number | null;
}

/** Beantwortet dieselbe Frage mit einem anderen Modell und wechselt danach zurück. */
export function compareAnswer(question: string, modelId: string, thinkingLevel: string): Promise<CompareAnswer> {
  return invoke("compare_answer", { question, modelId, thinkingLevel });
}

// --- Jobs, aktive Unterhaltung, Fenstermodus ------------------------------------

export function listJobs(): Promise<JobInfo[]> {
  return invoke("list_jobs");
}

export function cancelJob(jobId: string): Promise<boolean> {
  return invoke("cancel_job", { jobId });
}

export function onJobsChanged(callback: (jobs: JobInfo[]) => void): Promise<UnlistenFn> {
  return listen<JobInfo[]>("jobs-changed", (message) => callback(message.payload));
}

/** Die Unterhaltung, die Hauptseite, Chat und Pet gemeinsam nutzen. */
export function getActiveConversation(): Promise<string | null> {
  return invoke("get_active_conversation");
}

export function setActiveConversation(conversationId: string | null): Promise<void> {
  return invoke("set_active_conversation", { conversationId });
}

export function onActiveConversation(callback: (conversationId: string | null) => void): Promise<UnlistenFn> {
  return listen<string | null>("active-conversation", (message) => callback(message.payload));
}

export function getWindowMode(): Promise<WindowMode> {
  return invoke("get_window_mode");
}

// --- Zusatzpakete und Sprache ---------------------------------------------------

export function listPacks(): Promise<PackStatus[]> {
  return invoke("list_packs");
}

export function setPackEnabled(packId: string, enabled: boolean): Promise<void> {
  return invoke("set_pack_enabled", { packId, enabled });
}

/** Prüft die Prüfsummen eines Pakets vollständig (dauert bei großen Paketen). */
export function verifyPack(packId: string): Promise<void> {
  return invoke("verify_pack", { packId });
}

export function voiceStatus(): Promise<VoiceStatus> {
  return invoke("voice_status");
}

export function voiceStartCapture(): Promise<void> {
  return invoke("voice_start_capture");
}

/** Beendet die Aufnahme und liefert den erkannten Text; gesendet wird er nicht. */
export function voiceStopCapture(): Promise<Transcript> {
  return invoke("voice_stop_capture");
}

export function voiceDiscard(): Promise<void> {
  return invoke("voice_discard");
}

export function voiceSpeak(text: string): Promise<void> {
  return invoke("voice_speak", { text });
}

export function voiceStopOutput(): Promise<void> {
  return invoke("voice_stop_output");
}

export function voiceSetMuted(muted: boolean): Promise<void> {
  return invoke("voice_set_muted", { muted });
}

export function voiceRunBenchmark(): Promise<VoiceBenchmark> {
  return invoke("voice_run_benchmark");
}

export function onVoiceState(callback: (event: VoiceStateEvent) => void): Promise<UnlistenFn> {
  return listen<VoiceStateEvent>("voice-state", (message) => callback(message.payload));
}

// --- Bildschirm ansehen -----------------------------------------------------------

export function screenStatus(): Promise<ScreenStatus> {
  return invoke("screen_status");
}

export function screenSources(): Promise<ScreenSources> {
  return invoke("screen_sources");
}

/** Genau eine Aufnahme, lokal ausgewertet; Frage und Antwort landen in der Unterhaltung. */
export function screenLook(request: ScreenLookRequest): Promise<ScreenAnswer> {
  return invoke("screen_look", { request });
}

/** Klartext, warum Sprache gerade gesperrt ist; `null`, wenn sie frei ist. */
export function voiceBlockReason(): Promise<string | null> {
  return invoke("voice_block_reason");
}

// ---------------------------------------------------------------------------
// Agent Flow
// ---------------------------------------------------------------------------

export function agentFlowLimits(): Promise<import("./types").FlowLimits> {
  return invoke("agent_flow_limits");
}

/** Prüft ein Projekt; Host-Projekte brauchen `hostApproved` (wird im Tresor gemerkt). */
export function agentFlowPreflight(
  projectPath: string,
  hostApproved: boolean,
): Promise<import("./types").ProjectPreflight> {
  return invoke("agent_flow_preflight", { projectPath, hostApproved });
}

export function agentFlowProjects(): Promise<import("./types").ApprovedProject[]> {
  return invoke("agent_flow_projects");
}

export function agentFlowRevokeProject(path: string): Promise<void> {
  return invoke("agent_flow_revoke_project", { path });
}

export function agentFlowStart(
  request: import("./types").StartAgentFlowRequest,
): Promise<import("./types").AgentFlowTask> {
  return invoke("agent_flow_start", { request });
}

export function agentFlowList(): Promise<import("./types").AgentFlowTask[]> {
  return invoke("agent_flow_list");
}

export function agentFlowDiff(taskId: string, candidateId: string): Promise<string> {
  return invoke("agent_flow_diff", { taskId, candidateId });
}

export function agentFlowCancel(taskId: string): Promise<void> {
  return invoke("agent_flow_cancel", { taskId });
}

export function agentFlowAdoptPreview(
  taskId: string,
  candidateId: string,
): Promise<import("./types").AdoptPreview> {
  return invoke("agent_flow_adopt_preview", { taskId, candidateId });
}

/** `confirmed` ist die zweite, ausdrückliche Bestätigung nach der Diff-Vorschau. */
export function agentFlowAdopt(
  taskId: string,
  candidateId: string,
  confirmed: boolean,
): Promise<import("./types").AdoptResult> {
  return invoke("agent_flow_adopt", { taskId, candidateId, confirmed });
}

export function agentFlowCleanup(taskId: string, confirmed: boolean): Promise<void> {
  return invoke("agent_flow_cleanup", { taskId, confirmed });
}

export function onAgentFlowChanged(
  callback: (task: import("./types").AgentFlowTask) => void,
): Promise<UnlistenFn> {
  return listen<import("./types").AgentFlowTask>("agent-flow-changed", (message) =>
    callback(message.payload),
  );
}

// ---------------------------------------------------------------------------
// Workflows und Exa
// ---------------------------------------------------------------------------


/** Zustand aller Web-Konnektoren (ohne Schlüssel). */
export function connectorOverview(): Promise<import("./types").ConnectorOverview> {
  return invoke("connector_overview");
}
export function exaStatus(): Promise<import("./types").ExaStatus> {
  return invoke("exa_status");
}
export function workflowList(): Promise<import("./types").WorkflowDefinition[]> {
  return invoke("workflow_list");
}
export function workflowSave(definition: import("./types").WorkflowDefinition): Promise<import("./types").WorkflowDefinition> {
  return invoke("workflow_save", { definition });
}
export function workflowDelete(id: string): Promise<void> {
  return invoke("workflow_delete", { id });
}
export function workflowValidate(graph: import("./types").WorkflowGraph): Promise<import("./types").WorkflowValidation> {
  return invoke("workflow_validate", { graph });
}
/** `exa_approved` gilt nur für genau diesen Lauf. */
export function workflowStartRun(request: {
  workflow_id: string;
  budget: import("./types").RunBudget;
  exa_approved: boolean;
  /** Texte der „Eingabe beim Start“-Knoten, nach Knoten-ID. */
  inputs: Record<string, string>;
  /** Abweichung der lokalen Zeit von UTC in Minuten (für den Kalender-Baustein). */
  tz_offset_minutes: number;
}): Promise<string> {
  return invoke("workflow_start_run", { request });
}
export function workflowCancelRun(runId: string): Promise<boolean> {
  return invoke("workflow_cancel_run", { runId });
}
export function workflowRunReport(runId: string): Promise<import("./types").WorkflowRunReport | null> {
  return invoke("workflow_run_report", { runId });
}
export function workflowRuns(workflowId: string): Promise<import("./types").RunSummary[]> {
  return invoke("workflow_runs", { workflowId });
}
export function onWorkflowRunEvent(callback: (progress: import("./types").RunProgress) => void): Promise<UnlistenFn> {
  return listen<import("./types").RunProgress>("workflow-run-event", (m) => callback(m.payload));
}
export function onWorkflowRunFinished(callback: (report: import("./types").WorkflowRunReport) => void): Promise<UnlistenFn> {
  return listen<import("./types").WorkflowRunReport>("workflow-run-finished", (m) => callback(m.payload));
}

// ---------------------------------------------------------------------------
// Ordnerdialog und Agent-Flow-Fortschritt
// ---------------------------------------------------------------------------

/** Öffnet den Systemdialog „Ordner auswählen“; `null` heißt abgebrochen. */
export function pickFolderDialog(title: string): Promise<string | null> {
  return invoke("pick_folder_dialog", { title });
}

export interface AgentFlowProgress {
  task_id: string;
  candidate_id: string;
  tokens: number;
  seconds: number;
}

export function onAgentFlowProgress(callback: (progress: AgentFlowProgress) => void): Promise<UnlistenFn> {
  return listen<AgentFlowProgress>("agent-flow-progress", (message) => callback(message.payload));
}

/** Der gerade laufende Workflow-Lauf, falls einer läuft. */
export function workflowActiveRun(): Promise<string | null> {
  return invoke("workflow_active_run");
}

// --- Gmail ---------------------------------------------------------------

export function mailStatus(): Promise<MailStatus> {
  return invoke("mail_status");
}

/** Das App-Passwort geht nur hinein und kommt nie wieder heraus. */
export function mailSetPassword(password: string): Promise<void> {
  return invoke("mail_set_password", { password });
}

export function mailClearPassword(): Promise<void> {
  return invoke("mail_clear_password");
}

/** Liefert die Zahl ungelesener Mails des eingetragenen Absenders als Text. */
export function mailTestConnection(): Promise<string> {
  return invoke("mail_test_connection");
}

export function mailRunNow(): Promise<void> {
  return invoke("mail_run_now");
}

export function mailSetPolling(enabled: boolean): Promise<void> {
  return invoke("mail_set_polling", { enabled });
}

export function mailEmergencyStop(): Promise<ConnectorConfig> {
  return invoke("mail_emergency_stop");
}

export function mailLog(): Promise<MailLogEntry[]> {
  return invoke("mail_log");
}

export function mailClearLog(): Promise<void> {
  return invoke("mail_clear_log");
}

export function onMailChanged(callback: () => void): Promise<UnlistenFn> {
  return listen("mail-changed", () => callback());
}

// --- Kalender (Apple, Google) -----------------------------------------------

export function calendarOverview(): Promise<CalendarOverview> {
  return invoke("calendar_overview");
}

/** Termine aus dem Zwischenspeicher. Verbindet sich nie. */
export function calendarEvents(): Promise<RemoteEvent[]> {
  return invoke("calendar_events");
}

/** Meldet sich an und listet die Kalender. Speichert nichts. Das Passwort geht nur hinein. */
export function calendarIcloudDiscover(appleId: string, appPassword: string): Promise<CalendarRef[]> {
  return invoke("calendar_icloud_discover", { appleId, appPassword });
}

export function calendarAddIcloud(label: string, appleId: string, appPassword: string, calendars: CalendarRef[]): Promise<CalendarSource> {
  return invoke("calendar_add_icloud", { label, appleId, appPassword, calendars });
}

export function calendarAddGoogle(label: string, url: string): Promise<CalendarSource> {
  return invoke("calendar_add_google", { label, url });
}

export function calendarRemoveSource(sourceId: string): Promise<void> {
  return invoke("calendar_remove_source", { sourceId });
}

/** Ruft eine Quelle (oder alle, bei `null`) ab. Nur auf Klick, nie im Hintergrund. */
export function calendarSync(sourceId: string | null, tzOffsetMinutes: number): Promise<CalendarSyncReport[]> {
  return invoke("calendar_sync", { sourceId, tzOffsetMinutes });
}

export function calendarCreateEvent(sourceId: string, calendarHref: string, event: NewCalendarEvent, tzOffsetMinutes: number): Promise<RemoteEvent> {
  return invoke("calendar_create_event", { sourceId, calendarHref, event, tzOffsetMinutes });
}

export function onCalendarChanged(callback: () => void): Promise<UnlistenFn> {
  return listen("calendar-changed", () => callback());
}

// --- PCI (Personal Computer Information) -------------------------------------

export function pciStatus(): Promise<PciStatus> {
  return invoke("pci_status");
}

export function pciImport(): Promise<PciActivity> {
  return invoke("pci_import");
}

export function pciListComputers(): Promise<PciComputer[]> {
  return invoke("pci_list_computers");
}

export function pciActivity(host: string): Promise<PciActivity> {
  return invoke("pci_activity", { host });
}

export function pciDeleteHost(host: string): Promise<void> {
  return invoke("pci_delete_host", { host });
}

export function pciInstallAndStart(recordTitles: boolean): Promise<PciStatus> {
  return invoke("pci_install_and_start", { recordTitles });
}

export function pciStart(recordTitles: boolean): Promise<PciStatus> {
  return invoke("pci_start", { recordTitles });
}

export function pciStop(): Promise<PciStatus> {
  return invoke("pci_stop");
}

export function pciRemoveCollector(): Promise<PciStatus> {
  return invoke("pci_remove_collector");
}

// ---------------------------------------------------------------------------
// Arbeitsordner des Code-Bereichs (Stick oder freigegebener Ordner auf dem PC)
// ---------------------------------------------------------------------------

export function codeRootsStatus(): Promise<CodeRootsStatus> {
  return invoke("code_roots_status");
}

/** Prüft einen gewählten Ordner gegen die Sperrliste; ändert nichts. */
export function codeRootCheck(path: string): Promise<CodeRootCheck> {
  return invoke("code_root_check", { path });
}

/** Öffnet einen PC-Ordner; `confirmed` bestätigt die Freigabe, falls sie noch fehlt. */
export function codeRootOpenHost(path: string, confirmed: boolean): Promise<CodeRootsStatus> {
  return invoke("code_root_open_host", { path, confirmed });
}

export function codeRootUseStick(): Promise<CodeRootsStatus> {
  return invoke("code_root_use_stick");
}

export function codeRootRevoke(path: string): Promise<CodeRootsStatus> {
  return invoke("code_root_revoke", { path });
}

// ---------------------------------------------------------------------------
// Code-Agent: das Modell arbeitet im Projektordner und schlägt Änderungen vor
// ---------------------------------------------------------------------------

export function codeAgentSend(text: string, activeFile: string | null): Promise<void> {
  return invoke("code_agent_send", { text, activeFile });
}

export function codeAgentCancel(): Promise<void> {
  return invoke("code_agent_cancel");
}

/** Beginnt eine neue Aufgabe: Verlauf und offene Vorschläge werden verworfen. */
export function codeAgentReset(): Promise<void> {
  return invoke("code_agent_reset");
}

export function codeAgentChanges(): Promise<AgentChange[]> {
  return invoke("code_agent_changes");
}

export function codeAgentChange(path: string): Promise<AgentChangeContent> {
  return invoke("code_agent_change", { path });
}

/** Verwirft einen Vorschlag (`path`) oder alle (`null`). */
export function codeAgentDiscard(path: string | null): Promise<void> {
  return invoke("code_agent_discard", { path });
}

export function onCodeAgentEvent(callback: (event: AgentEvent) => void): Promise<UnlistenFn> {
  return listen<AgentEvent>("code-agent-event", (message) => callback(message.payload));
}

/** Antwort auf den Bestätigungsdialog eines Befehls: `allow` führt ihn aus. */
export function codeAgentCommandRespond(id: string, allow: boolean): Promise<void> {
  return invoke("code_agent_command_respond", { id, allow });
}

export function onCodeAgentCommand(callback: (prompt: CommandPrompt) => void): Promise<UnlistenFn> {
  return listen<CommandPrompt>("code-agent-command", (message) => callback(message.payload));
}

// ---------------------------------------------------------------------------
// Tresor verwalten: Passwort ändern, Tresor löschen
// ---------------------------------------------------------------------------

/** Ändert das Passwort des geöffneten Tresors (alles oder nichts). */
export function changeVaultPassword(oldPassphrase: string, newPassphrase: string): Promise<void> {
  return invoke("change_vault_password", { oldPassphrase, newPassphrase });
}

/** Löscht einen Tresor im Startbildschirm. Verlangt sein Passwort und die Eingabe seines Namens. */
export function deleteVault(vaultPath: string, passphrase: string, confirmName: string): Promise<void> {
  return invoke("delete_vault", { vaultPath, passphrase, confirmName });
}
