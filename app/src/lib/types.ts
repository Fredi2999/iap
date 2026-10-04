// TypeScript-Spiegel der in `crates/pa-types/src/ipc.rs` definierten IPC-Verträge.
// Änderungen dort müssen hier nachgezogen werden – der ipc_contract-Test bewacht
// die Serialisierung nur einseitig auf Rust-Seite.

export type MessageRole = "system" | "user" | "assistant";
export type MessageStatus = "complete" | "streaming" | "aborted";

export interface Conversation {
  id: string;
  title: string;
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
  /** Projekt der Unterhaltung; null = ohne Projekt. */
  project_id?: string | null;
}

export interface Message {
  id: string;
  conversation_id: string;
  position: number;
  role: MessageRole;
  content: string;
  status: MessageStatus;
  created_at_unix_ms: number;
}

export interface ServerTimingsDto {
  prompt_ms: number | null;
  prompt_per_second: number | null;
  predicted_per_second: number | null;
}

export interface StreamOutcomeDto {
  text: string;
  aborted: boolean;
  timings: ServerTimingsDto;
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
}

export type StreamErrorKind = "transport" | "protocol" | "process_exited" | "adapter";

export interface ManifestSummary {
  version: string;
  verified_files: number;
  verified_bytes: number;
  embedding_model_id?: string | null;
}

export interface AvailableModel {
  id: string;
  display_name: string;
  family: string;
  gguf_bytes: number;
  sha256: string;
  max_context_tokens: number;
  is_default: boolean;
}

export interface CatalogModel {
  id: string;
  display_name: string;
  family: string;
  file_bytes: number;
  peak_ram_bytes_8k: number;
  max_context_tokens: number;
  license: string | null;
  source_url: string | null;
  installed: boolean;
  is_default: boolean;
}

export type HardwareTier = "unsupported" | "t0" | "t1" | "t2" | "t3";
export type KvQuantization = "f16" | "q8_0";

export interface Measurement<T> {
  status: "measured" | "unavailable";
  value?: T;
  reason?: string;
  method: string;
}

export interface ResourcePlan {
  tier: HardwareTier;
  tier_hardware_validated: boolean;
  available_bytes: number;
  model_budget_bytes: number;
  kv_budget_bytes: number;
  gpu_budget_bytes: number;
  context_tokens: number;
  context_was_clamped: boolean;
  threads: number;
  gpu_layers: number;
  kv_quantization: KvQuantization;
  warnings: string[];
}

export interface HardwareProfile {
  cpu_model: string;
  logical_processors: number;
  total_ram_bytes: number;
  available_ram_bytes: number;
  operating_system: string;
  // Weitere Felder aus pa-types::hardware::HardwareProfile werden hier bewusst
  // nicht ausformuliert; die UI liest sie generisch als Record.
  [extra: string]: unknown;
}

export interface BootstrapStatus {
  manifest: ManifestSummary;
  hardware: HardwareProfile;
  plan: ResourcePlan;
  default_model: AvailableModel;
  warnings: string[];
  vault_initialized?: boolean;
}

export interface ConversationDetail {
  conversation: Conversation;
  messages: Message[];
}

export interface SendMessageRequest {
  conversation_id: string | null;
  content: string;
  thinking_level?: "kurz" | "standard" | "sorgfältig" | "vertieft" | "maximal";
  /** Mit „/name“ für genau diesen Zug gewählter Skill. */
  skill_id?: string;
}

export type StreamEvent =
  | {
      kind: "started";
      conversation_id: string;
      user_message_id: string;
      assistant_message_id: string;
      engine_restarted: boolean;
    }
  | {
      kind: "delta";
      assistant_message_id: string;
      text: string;
    }
  | {
      kind: "finished";
      assistant_message_id: string;
      outcome: StreamOutcomeDto;
      dropped_older_turns: number;
    }
  | {
      kind: "failed";
      assistant_message_id: string | null;
      error_kind: StreamErrorKind;
      message: string;
      partial_text: string;
      timings: ServerTimingsDto;
    };

export type ThemePreference = "system" | "dark" | "light";

export interface SettingsSnapshot {
  tier_override: HardwareTier | null;
  model_id: string;
  context_tokens: number;
  kv_quantization: KvQuantization;
  theme: ThemePreference;
  vault_path: string;
}

export type TierOverrideChange =
  | { op: "clear" }
  | { op: "set"; tier: HardwareTier };

export interface SettingsUpdate {
  tier_override?: TierOverrideChange | null;
  model_id?: string | null;
  context_tokens?: number | null;
  theme?: ThemePreference | null;
}

export interface ModelSettings {
  model_id: string;
  context_tokens: number | null;
  temperature: number;
  top_p: number;
}

export interface VaultSwitchRequest {
  new_vault_path: string;
  passphrase: string;
  create_if_missing: boolean;
}

export interface VaultInfo {
  name: string;
  path: string;
  is_default: boolean;
  initialized: boolean;
  size_bytes: number;
}

export interface UserProfile {
  enabled: boolean;
  user_name: string;
  user_about: string;
  custom_system_prompt: string;
}

// --- Phase 2: Memory, Logs, Dateien, Werkzeuge --------------------------

export type FactCategory =
  | "preference"
  | "project"
  | "person"
  | "skill"
  | "constraint"
  | "other";

export type ItemType = "fact" | "chunk";

export interface Fact {
  id: string;
  text: string;
  category: FactCategory;
  confidence: number;
  source_message_id: string | null;
  valid_from_unix_ms: number;
  valid_until_unix_ms: number | null;
  superseded_by: string | null;
  user_verified: boolean;
  access_count: number;
  last_accessed_unix_ms: number | null;
}

export interface MemoryHit {
  item_type: ItemType;
  item_id: string;
  score: number;
  preview: string;
}

export interface MemoryRetrieveResponse {
  hits: MemoryHit[];
  query: string;
}

export interface MemoryUpsertRequest {
  id: string | null;
  text: string;
  category: FactCategory;
  user_verified: boolean;
}

export interface MemoryExport {
  exported_at_unix_ms: number;
  facts: Fact[];
}

export interface AuditFilter {
  actions: string[];
  outcomes: string[];
  contains: string | null;
  since_unix_ms: number | null;
  limit: number | null;
}

export interface AuditEntryView {
  id: number;
  created_unix_ms: number;
  mode: string;
  action: string;
  target: string | null;
  outcome: string;
  reason: string;
  hash_hex: string;
  prev_hash_hex: string;
}

export interface WorkspaceEntry {
  name: string;
  relative_path: string;
  is_directory: boolean;
  bytes: number;
  modified_unix_ms: number | null;
}

export interface WorkspaceListing {
  root: string;
  relative_path: string;
  entries: WorkspaceEntry[];
}

export type ToolStreamEvent =
  | {
      kind: "tool_call";
      assistant_message_id: string;
      tool: string;
      arguments_json: string;
    }
  | {
      kind: "tool_result";
      assistant_message_id: string;
      tool: string;
      content: string;
      is_untrusted: boolean;
    }
  | {
      kind: "tool_error";
      assistant_message_id: string;
      tool: string;
      message: string;
    }
  | {
      kind: "permission_requested";
      pending_id: string;
      assistant_message_id: string;
      tool: string;
      arguments_json: string;
      reason: string;
    };

export interface ToolPromptResponse {
  pending_id: string;
  allow: boolean;
  remember_for_session: boolean;
}

// ============================================================================
// Rubrik-Engine (Konzept 4.2)
// ============================================================================

export type EvidenceSource = "user" | "document" | "assumption";

export interface Criterion {
  id: string;
  label: string;
  weight: number;
  description: string;
}

export interface Rubric {
  id: string;
  label: string;
  criteria: Criterion[];
}

export interface QuestionnaireResponses {
  problem: string | null;
  audience: string | null;
  solution: string | null;
  revenue_model: string | null;
  cost_drivers: string | null;
  competition: string | null;
  unfair_advantage: string | null;
  regulatory_context: string | null;
  required_capital: string | null;
  time_to_first_revenue: string | null;
}

export interface SaveQuestionnaireResult {
  missing_fields: string[];
  saved_unix_ms: number;
}

export interface CriterionScore {
  criterion_id: string;
  score: number;
  rationale: string;
  source: EvidenceSource;
}

export interface RubricScoreSheet {
  scores: CriterionScore[];
}

export interface WeightedContribution {
  criterion_id: string;
  label: string;
  score: number;
  weight: number;
  contribution: number;
}

export interface WeightedResult {
  weighted_score: number;
  normalized: number;
  contributions: WeightedContribution[];
}

export interface RangeInput {
  min: number;
  max: number;
}

export interface MonteCarloInput {
  price_per_unit: RangeInput;
  units_per_month: RangeInput;
  fixed_cost_per_month: RangeInput;
  variable_cost_per_unit: RangeInput;
  starting_capital: number;
}

export interface MonteCarloResult {
  runs: number;
  monthly_profit_p10: number;
  monthly_profit_p50: number;
  monthly_profit_p90: number;
  probability_positive_month: number;
  expected_break_even_months: number | null;
  expected_runway_months: number | null;
}

export interface RunMonteCarloRequest {
  input: MonteCarloInput;
  runs?: number;
  seed?: number;
}

// ============================================================================
// Code-Bereich (Konzept 9)
// ============================================================================

export type LineOp = "context" | "insert" | "delete";

export interface HunkLine {
  op: LineOp;
  text: string;
}

export interface DiffHunk {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  lines: HunkLine[];
}

export interface UnifiedDiff {
  old_path: string;
  new_path: string;
  hunks: DiffHunk[];
}

export type SnapshotKind = "hardlink" | "copy" | "mixed";

export interface SnapshotView {
  id: string;
  source_root: string;
  snapshot_root: string;
  kind: SnapshotKind;
  file_count: number;
  bytes_referenced: number;
  created_unix_ms: number;
}

/** Ob Git im Code-Bereich nutzbar ist (Git-Paket und Repository), mit Zweig oder Grund. */
export interface GitInfo {
  available: boolean;
  branch: string | null;
  note: string | null;
}

export interface GitStatusEntry {
  status: string;
  relative_path: string;
}

export interface GitCommit {
  id: string;
  summary: string;
  author: string;
  unix_ts: number;
}

// --- Freigabe-Rendezvous (Konzept 10.3) ---

export interface PendingPermissionView {
  pending_id: string;
  tool: string;
  arguments_json: string;
  reason: string;
  created_unix_ms: number;
}

// ============================================================================
// Phase 4: Scheduler, Skills, Updates, Export
// ============================================================================

export type SchedulerWeekday = "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday";
export type SchedulerEnergy = "low" | "medium" | "high";
export type SchedulerTaskStatus = "open" | "in_progress" | "done" | "cancelled";

export interface SchedulerAvailability {
  weekday: SchedulerWeekday;
  start_minute: number;
  end_minute: number;
}

export interface SchedulerEvent {
  id: string;
  title: string;
  start_unix_ms: number;
  end_unix_ms: number;
  location?: string | null;
  project?: string | null;
  external_uid?: string | null;
  /** Nur in der Anzeige: Ganztagstermin eines verbundenen Kalenders. */
  all_day?: boolean;
  /** Nur in der Anzeige: gesetzt bei Terminen aus einem verbundenen Kalender (schreibgeschützt). */
  remote?: RemoteEvent | null;
}

// --- Kalender-Anbindung (Apple, Google) ---

export type CalendarSourceKind = "icloud" | "google_ics";

export interface CalendarRef {
  host: string;
  href: string;
  name: string;
  color?: string | null;
  can_write: boolean;
}

/** Eine verbundene Quelle; Passwort und geheime Adresse verlassen den Tresor nie. */
export interface CalendarSource {
  id: string;
  kind: CalendarSourceKind;
  label: string;
  account: string;
  calendars: CalendarRef[];
  last_sync_unix_ms?: number | null;
  last_error?: string | null;
  unsupported_rules: number;
}

export interface CalendarOverview {
  air_gap: boolean;
  sources: CalendarSource[];
  supported: boolean;
}

export interface RemoteEvent {
  id: string;
  source_id: string;
  source_label: string;
  calendar_name: string;
  color?: string | null;
  title: string;
  start_unix_ms: number;
  end_unix_ms: number;
  all_day: boolean;
  /** `YYYY-MM-DD`, nur Ganztag. */
  start_date?: string | null;
  /** `YYYY-MM-DD`, exklusiv, nur Ganztag. */
  end_date?: string | null;
  location?: string | null;
  notes?: string | null;
  recurring: boolean;
}

export interface CalendarSyncReport {
  source_id: string;
  ok: boolean;
  count: number;
  error?: string | null;
}

/** Ein neuer Termin für einen verbundenen Kalender. Ganztag: `end_date` ist der letzte Tag (einschließlich). */
export interface NewCalendarEvent {
  title: string;
  all_day: boolean;
  start_unix_ms: number;
  end_unix_ms: number;
  start_date?: string | null;
  end_date?: string | null;
  location?: string | null;
  notes?: string | null;
}

export interface SchedulerTask {
  id: string;
  title: string;
  project?: string | null;
  due_unix_ms?: number | null;
  duration_minutes: number;
  energy: SchedulerEnergy;
  priority: number;
  depends_on: string[];
  status: SchedulerTaskStatus;
}

export interface SchedulerPlanRequest {
  now_unix_ms: number;
  tz_offset_minutes: number;
  horizon_days: number;
  availabilities: SchedulerAvailability[];
  events: SchedulerEvent[];
  tasks: SchedulerTask[];
  break_minutes?: number;
  day_energy_budget?: number;
}

export type ScheduledSlot =
  | { kind: "event"; event_id: string; title: string; start_unix_ms: number; end_unix_ms: number }
  | { kind: "task"; task_id: string; title: string; start_unix_ms: number; end_unix_ms: number; energy: SchedulerEnergy }
  | { kind: "break"; start_unix_ms: number; end_unix_ms: number };

export interface UnscheduledTask { task_id: string; reason: string }

export interface SolvedPlan { slots: ScheduledSlot[]; unscheduled: UnscheduledTask[] }

export interface IcsImportResult { events: SchedulerEvent[]; tasks: SchedulerTask[] }

export interface SkillPermissionEntry { topic: string; detail: string; is_sensitive: boolean }
export interface SkillToolView { name: string; description: string }
export interface InstalledSkillView {
  id: string;
  name: string;
  version: string;
  permissions: SkillPermissionEntry[];
  tools: SkillToolView[];
  /** "wasm": Programm in der Sandbox; "instructions": Anleitung aus einer SKILL.md. */
  kind: "wasm" | "instructions";
  description: string;
  /** Nur Anleitungen: wird dem Modell bei jeder Antwort mitgegeben. */
  active: boolean;
  body_preview: string;
  files: string[];
  /** Beim Import bewusst übersprungene Dateien. */
  skipped: string[];
}
export interface DryRunOutcome {
  skill_id: string;
  tool: string;
  arguments: Record<string, unknown>;
  permissions: SkillPermissionEntry[];
  warnings: string[];
}

export type SignatureVerification =
  | { kind: "ok" }
  | { kind: "skipped"; reason: string }
  | { kind: "not_provided" };
export interface BundleVerification {
  verified_files: number;
  verified_bytes: number;
  signature: SignatureVerification;
}

// ============================================================================
// Konnektoren: Exa Research und Air Gap
// ============================================================================

export interface ConnectorConfig {
  offline_mode: boolean;
  exa_enabled: boolean;
  exa_api_key: string;
  wikipedia_enabled: boolean;
  open_meteo_enabled: boolean;
  brave_enabled: boolean;
  brave_api_key: string;
  gmail_enabled: boolean;
  gmail_address: string;
  gmail_target_email: string;
  gmail_check_interval_minutes: number;
  gmail_reply_mode: MailReplyMode;
  gmail_send_acknowledged: boolean;
  gmail_max_per_hour: number;
  gmail_max_per_day: number;
  gmail_instruction: string;
  gmail_allow_read: boolean;
}

export type MailReplyMode = "draft" | "send";

export interface MailStatus {
  supported: boolean;
  enabled: boolean;
  has_password: boolean;
  polling: boolean;
  running: boolean;
  last_run_unix_ms: number | null;
  next_run_unix_ms: number | null;
  last_error: string | null;
  sent_last_hour: number;
  sent_today: number;
  blocked_reason: string | null;
}

export type MailLogKind = "drafted" | "sent" | "skipped" | "error" | "info";

export interface MailLogEntry {
  at_unix_ms: number;
  kind: MailLogKind;
  subject: string;
  detail: string;
}

export interface ConnectorState {
  id: "exa" | "wikipedia" | "open_meteo" | "brave";
  name: string;
  enabled: boolean;
  ready: boolean;
  reason?: string | null;
}

export interface ConnectorOverview {
  air_gap: boolean;
  services: ConnectorState[];
}

export interface ExaSearchResult {
  title: string;
  url: string;
  snippet: string;
}


export type Route =
  | "home"
  | "chat"
  | "flow"
  | "focus"
  | "rubric"
  | "code"
  | "calendar"
  | "scratch"
  | "voice"
  | "import"
  | "digests"
  | "palace"
  | "connectors"
  | "skills"
  | "templates"
  | "updates"
  | "memory"
  | "pci"
  | "files"
  | "logs"
  | "settings";

// --- Spuren auf dem Host (Feature „Spurlos beenden“) -------------------------

/** Ein Ort auf diesem PC, an dem IAP Daten hinterlässt. */
export interface HostTrace {
  /** Stabile Kennung: "model_cache", "vault_hot", "legacy_webview". */
  id: string;
  path: string;
  size_bytes: number;
}

export interface HostTraceReport {
  traces: HostTrace[];
  total_bytes: number;
  /** false, wenn der Nutzer diesen PC als eigenen markiert hat. */
  ask_on_exit: boolean;
}

// --- Projekte (Feature 5) und Dokumente mit Quellen (Feature 1) ---------------

export interface Project {
  id: string;
  name: string;
  /** Wird dem Systemprompt jeder Unterhaltung des Projekts vorangestellt. */
  system_prompt: string;
  created_at_unix_ms: number;
  updated_at_unix_ms: number;
}

export interface DocumentInfo {
  id: string;
  name: string;
  kind: "txt" | "md" | "docx" | "pdf";
  size_bytes: number;
  chunk_count: number;
  created_at_unix_ms: number;
}

/** Nummerierte Quelle einer Antwort, auf die der Text mit [n] verweist. */
export interface MessageSource {
  message_id: string;
  number: number;
  document_id: string;
  document_name: string;
  chunk_ordinal: number;
  excerpt: string;
}

// --- Leistungs-Check (Feature 3) ---------------------------------------------

/** Messergebnis je PC. Fehlende Werte wurden nicht gemessen und werden nie geschätzt. */
export interface PerfReport {
  measured_at_unix_ms: number;
  model_id: string;
  model_name: string;
  model_start_ms: number | null;
  prompt_per_second: number | null;
  tokens_per_second: number | null;
  ram_peak_bytes: number | null;
  total_ram_bytes: number;
  recommended_thinking: string | null;
  recommended_model_id: string | null;
}

// --- Avatar, Fenstermodus und Jobs -------------------------------------------

/** Echte App-Zustände, die der Avatar zeigt. */
export type AvatarState = "ready" | "listening" | "transcribing" | "processing" | "speaking" | "awaiting_approval" | "stopped" | "error";

export type WindowMode = "main" | "pet";

export type JobKind = "chat" | "agent" | "agent_flow" | "workflow" | "vision" | "speech_to_text" | "text_to_speech" | "model_switch" | "compare" | "mail";
export type JobStatus = "waiting" | "running" | "finished" | "failed" | "cancelled";

/** Eintrag der Job-Übersicht mit Warteposition der seriellen Modell-Queue. */
export interface JobInfo {
  id: string;
  kind: JobKind;
  label: string;
  status: JobStatus;
  queue_position: number | null;
  started_unix_ms: number;
  cancelable: boolean;
}

// --- Sprache ---------------------------------------------------------------

export type VoiceState = "idle" | "listening" | "transcribing" | "processing" | "speaking";
export type VoiceBlock = "pack_missing" | "benchmark_pending" | "vault_locked" | "microphone_denied";
export type VoiceCommand = "open_chat" | "open_flow" | "open_workflows" | "open_home";

export interface PackStatus {
  id: string;
  name: string;
  installed: boolean;
  enabled: boolean;
  size_bytes: number;
  version?: string;
  license?: string;
  missing_reason: string | null;
}

export interface VoiceBenchmark {
  measured_unix_ms: number;
  ram_reserve_bytes: number;
  stt_real_time_factor: number;
  tts_first_audio_ms: number;
  load_ms: number;
  passed: boolean;
  notes: string[];
}

export interface VoiceStatus {
  available: boolean;
  block: VoiceBlock | null;
  state: VoiceState;
  muted: boolean;
  packs: PackStatus[];
  language: string;
  voice_available: boolean;
  benchmark: VoiceBenchmark | null;
}

export interface VoiceStateEvent {
  state: VoiceState;
  muted: boolean;
  elapsed_ms: number;
  level: number;
  message: string | null;
}

export interface Transcript {
  text: string;
  language: string;
  audio_ms: number;
  transcribe_ms: number;
  command: VoiceCommand | null;
}

// --- Bildschirm ansehen --------------------------------------------------------

export type ScreenTarget =
  | { kind: "monitor"; index: number }
  | { kind: "window"; handle: number }
  | { kind: "region"; monitor: number; x: number; y: number; width: number; height: number };

export interface MonitorInfo { index: number; name: string; x: number; y: number; width: number; height: number; primary: boolean }
export interface WindowInfo { handle: number; title: string }
export interface ScreenSources { monitors: MonitorInfo[]; windows: WindowInfo[] }
export type ScreenBlock = "not_set_up" | "tier_blocked" | "vault_locked";
export interface ScreenStatus { available: boolean; block: ScreenBlock | null; detail: string | null }
export interface ScreenLookRequest { target: ScreenTarget; question: string; conversation_id: string | null }
export interface ScreenAnswer {
  conversation_id: string;
  message_id: string;
  captured_unix_ms: number;
  source_label: string;
  answer: string;
}

// ---------------------------------------------------------------------------
// Agent Flow (Spiegel von pa-types/agent_flow.rs)
// ---------------------------------------------------------------------------

export type ProjectLocation = "stick" | "host";
export type FileState = "modified" | "added" | "deleted" | "untracked" | "ignored";

export interface DirtyFile {
  path: string;
  state: FileState;
  sensitive: boolean;
}

export interface ProjectPreflight {
  repo_root: string;
  location: ProjectLocation;
  branch: string | null;
  head: string | null;
  worktrees_root: string;
  filesystem: string;
  free_bytes: number;
  dirty_files: DirtyFile[];
  blockers: string[];
  warnings: string[];
}

export type BaseChoice =
  | { kind: "head_commit" }
  | { kind: "snapshot"; include_untracked: string[] };

export interface BaseInfo {
  choice: BaseChoice;
  base_commit: string;
  target_head: string;
}

export interface CandidateSpec {
  model_id: string;
  max_tokens: number;
  max_seconds: number;
}

export interface StartAgentFlowRequest {
  project_path: string;
  host_project_approved: boolean;
  prompt: string;
  base: BaseChoice;
  candidates: CandidateSpec[];
  context_files: string[];
}

export type CandidateStatus =
  | { kind: "queued"; position: number }
  | { kind: "loading" }
  | { kind: "generating" }
  | { kind: "applying" }
  | { kind: "ready" }
  | { kind: "failed"; reason: string }
  | { kind: "cancelled" };

export type TestStatus =
  | { kind: "not_run"; reason: string }
  | { kind: "passed"; summary: string }
  | { kind: "failed"; summary: string };

export interface ResourceUsage {
  tokens: number;
  seconds: number;
  peak_rss_bytes: number | null;
}

export interface Candidate {
  id: string;
  index: number;
  branch: string;
  worktree_path: string;
  model_id: string;
  status: CandidateStatus;
  usage: ResourceUsage;
  tests: TestStatus;
  changed_files: string[];
  base_commit: string;
}

export interface AgentFlowTask {
  id: string;
  project_path: string;
  location: ProjectLocation;
  prompt: string;
  base: BaseInfo;
  candidates: Candidate[];
  created_unix_ms: number;
}

export interface AdoptConflict {
  path: string;
  reason: string;
}

export interface AdoptPreview {
  candidate_id: string;
  diff: string;
  files: string[];
  conflicts: AdoptConflict[];
}

export interface AdoptResult {
  written_files: string[];
  note: string;
}

export interface FlowLimits {
  default_candidates: number;
  max_candidates: number;
  weak_hardware: boolean;
}

export interface ApprovedProject {
  path: string;
  location: ProjectLocation;
}

// ---------------------------------------------------------------------------
// Workflows (Spiegel von pa-types/flow.rs)
// ---------------------------------------------------------------------------

export type DataKind = "signal" | "text" | "results" | "verdict";

export type BranchRule = { kind: "contains"; text: string } | { kind: "model_yes_no"; question: string };

export type StoreTarget = { kind: "memory" } | { kind: "task" } | { kind: "file"; relative_path: string };

export type NodeKind =
  | { type: "manual_start" }
  | { type: "input"; text: string }
  | { type: "exa_search"; num_results: number }
  | { type: "exa_contents"; max_characters: number }
  | { type: "local_model"; instruction: string }
  | { type: "check"; criteria: string[] }
  | { type: "condition"; max_iterations: number }
  | { type: "output" }
  | { type: "runtime_input"; label: string; public: boolean }
  | { type: "calendar"; days_ahead: number; include_tasks: boolean }
  | { type: "memory_search"; max_hits: number }
  | { type: "skill"; skill_id: string }
  | { type: "branch"; rule: BranchRule }
  | { type: "join" }
  | { type: "merge"; template: string }
  | { type: "notify"; title: string }
  | { type: "store"; target: StoreTarget }
  | { type: "wikipedia_search"; num_results: number; lang: string }
  | { type: "brave_search"; num_results: number }
  | { type: "weather"; days: number }
  | { type: "mail_search"; from: string; subject: string; unread_only: boolean; limit: number };

export interface WorkflowNode {
  id: string;
  kind: NodeKind;
  x: number;
  y: number;
}

export interface WorkflowEdge {
  id: string;
  from: string;
  from_port: string;
  to: string;
  to_port: string;
}

export interface WorkflowGraph {
  version: number;
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
}

export interface WorkflowDefinition {
  id: string;
  name: string;
  graph: WorkflowGraph;
  updated_unix_ms: number;
}

export interface GraphProblem {
  node_id?: string;
  edge_id?: string;
  message: string;
}

export interface WorkflowValidation {
  ok: boolean;
  problems: GraphProblem[];
}

export interface RunBudget {
  max_searches: number;
  max_iterations: number;
  max_seconds: number;
  max_tokens: number;
  max_cost_usd: number;
}

export interface RunUsage {
  searches: number;
  iterations: number;
  seconds: number;
  tokens: number;
  cost_usd: number;
}

export type RunStatus =
  | "running"
  | "finished"
  | "not_sufficiently_supported"
  | "budget_exhausted"
  | "cancelled"
  | "failed";

export type RunEventKind =
  | "started"
  | "node_started"
  | "node_finished"
  | "data_flow"
  | "budget_update"
  | "notice"
  | "denied"
  | "error"
  | "finished";

export interface RunEvent {
  seq: number;
  at_unix_ms: number;
  kind: RunEventKind;
  node_id?: string;
  message: string;
  origin?: string;
}

export interface WorkflowSource {
  title: string;
  url: string;
}

/** Vorschlag aus einem „Ablegen“-Knoten; geschrieben wird erst nach Bestätigung. */
export interface WorkflowProposal {
  node_id: string;
  target: StoreTarget;
  text: string;
}

export interface WorkflowResult {
  answer: string;
  sources: WorkflowSource[];
  open_points: string[];
  proposals?: WorkflowProposal[];
}

export interface WorkflowRunReport {
  run_id: string;
  workflow_id: string;
  status: RunStatus;
  usage: RunUsage;
  budget: RunBudget;
  started_unix_ms: number;
  finished_unix_ms?: number;
  current_node?: string;
  result?: WorkflowResult;
  events: RunEvent[];
}

export interface RunProgress {
  run_id: string;
  event: RunEvent;
  usage: RunUsage;
  current_node: string | null;
}

export interface RunSummary {
  run_id: string;
  status: RunStatus;
  started_unix_ms: number;
}

export interface ExaStatus {
  has_key: boolean;
  enabled: boolean;
  air_gap: boolean;
}

// --- Code-Bereich: Suche und Assistent ---

export interface CodeHit {
  relative_path: string;
  line: number;
  text: string;
}

export interface CodeAssistSelection {
  before: string;
  text: string;
  after: string;
}

export interface CodeAssistRequest {
  relative_path: string;
  instruction: string;
  content: string;
  selection: CodeAssistSelection | null;
}

export interface CodeAssistReply {
  explanation: string;
  code: string;
  /** "file": ganze Datei ersetzen, "selection": nur die Markierung. */
  scope: "file" | "selection";
}

// --- Gedächtnis-Graph ----------------------------------------------------

export type GraphNodeKind = "fact" | "category" | "ghost";
export type GraphEdgeKind = "category" | "supersedes" | "link";

export interface GraphNode {
  id: string;
  kind: GraphNodeKind;
  label: string;
  category: FactCategory | null;
  verified: boolean;
  superseded: boolean;
  confidence: number;
  access_count: number;
  created_unix_ms: number;
  learned: boolean;
}

export interface GraphEdge {
  from: string;
  to: string;
  kind: GraphEdgeKind;
}

export interface MemoryGraph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface MemoryUpdateRequest {
  id: string;
  text: string;
  category: FactCategory;
  user_verified: boolean;
}

// --- Pet: Aussehen und Systemwerte ---------------------------------------

export interface PetStyle {
  shape: string;
  /** `auto` oder `#rrggbb`. */
  color: string;
  size: number;
  show_cpu: boolean;
  show_ram: boolean;
  show_storage: boolean;
  show_task: boolean;
  refresh_secs: number;
}

export interface SystemStats {
  cpu_name: string;
  cpu_percent: number;
  ram_used_bytes: number;
  ram_total_bytes: number;
  disk_name: string;
  disk_used_bytes: number;
  disk_total_bytes: number;
}

// --- PCI (Personal Computer Information) -------------------------------------

export interface PciAppUsage {
  app: string;
  total_ms: number;
  focus_count: number;
  last_seen_unix_ms: number;
  sample_titles: string[];
}

export interface PciComputer {
  host: string;
  import_count: number;
  total_active_ms: number;
  first_unix_ms: number;
  last_unix_ms: number;
}

export interface PciActivity {
  host: string;
  total_active_ms: number;
  first_unix_ms: number;
  last_unix_ms: number;
  apps: PciAppUsage[];
}

export interface PciStatus {
  supported: boolean;
  installed: boolean;
  running: boolean;
  journal_pending: boolean;
  journal_active_ms: number;
  host: string;
}

/** Wo der Code-Bereich arbeitet: Stick (`host_path` leer) oder freigegebener Ordner auf dem PC. */
export interface CodeRootsStatus {
  host_path: string | null;
  stick_path: string;
  approved: string[];
}

export interface CodeRootCheck {
  path: string;
  approved: boolean;
}

/** Ereignis des Code-Agenten (Kanal `code-agent-event`). */
export type AgentEvent =
  | { kind: "working"; step: number }
  | { kind: "tool_call"; tool: string; arguments: string }
  | { kind: "tool_result"; tool: string; preview: string }
  | { kind: "tool_error"; tool: string; message: string }
  | { kind: "done"; text: string; changes: number }
  | { kind: "failed"; message: string }
  | { kind: "cancelled" };

/** Ein offener Änderungsvorschlag des Code-Agenten. */
export interface AgentChange {
  path: string;
  is_new: boolean;
  added: number;
  removed: number;
}

export interface AgentChangeContent {
  path: string;
  is_new: boolean;
  proposed: string;
}

/** Anfrage des Code-Agenten, einen Befehl auszuführen (Kanal `code-agent-command`). */
export interface CommandPrompt {
  id: string;
  command: string;
  cwd: string;
  timeout_seconds: number;
  /** Der Befehl stammt aus gelesenem Dateiinhalt, nicht aus der Aufgabe. */
  derived_from_content: boolean;
}
