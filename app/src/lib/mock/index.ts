// Mock-Backend für die Browser-Vorschau und die Screenshot-Prüfung (`npm run screenshots`).
//
// Warum: Die Oberfläche läuft nur in Tauri mit echtem Backend. Ohne Mock lässt sich keine Seite
// ansehen, ohne dass ein Mensch den Stick startet. Dieses Modul ersetzt `invoke` durch feste
// Antworten, damit jede Seite in einem normalen Browser rendert.
//
// Wichtig: Es wird nur eingebunden, wenn mit VITE_MOCK=1 gebaut wird (siehe main.ts). Die
// Beispieldaten hier gelangen also nie in ein ausgeliefertes Paket. Das Backend (Rust) wird damit
// nicht geprüft, nur die Darstellung.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

const NOW = 1_790_000_000_000;
const DAY = 86_400_000;

const uiState = new Map<string, string>();

const catalog = [
  { id: "gemma-4-e2b-q4-k-m", display_name: "Gemma 4 E2B Instruct Q4_K_M", family: "gemma4", file_bytes: 3_106_738_272, peak_ram_bytes_8k: 2_900_000_000, max_context_tokens: 131_072, license: "Apache-2.0", source_url: "https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF", installed: true, is_default: true },
  { id: "llama32-3b-abl", display_name: "Llama 3.2 3B abliterated Q4_K_M", family: "llama", file_bytes: 2_240_000_000, peak_ram_bytes_8k: 4_400_000_000, max_context_tokens: 131_072, license: "Llama 3.2 Community License", source_url: "https://huggingface.co/mradermacher/Llama-3.2-3B-Instruct-abliterated-GGUF", installed: false, is_default: false },
  { id: "qwen3-4b-instruct-2507-q4-k-m", display_name: "Qwen3 4B Instruct 2507 Q4_K_M", family: "qwen3", file_bytes: 2_497_280_736, peak_ram_bytes_8k: 5_547_028_480, max_context_tokens: 131_072, license: "Apache-2.0", source_url: "https://huggingface.co/bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF", installed: false, is_default: false },
];

const model = {
  id: "gemma-4-e2b-q4-k-m",
  display_name: "Gemma 4 E2B Instruct Q4_K_M",
  family: "gemma4",
  gguf_bytes: 3_106_738_272,
  sha256: "0".repeat(64),
  max_context_tokens: 131_072,
  is_default: true,
};

const bootstrap = {
  manifest: { version: "mock", verified_files: 120, verified_bytes: 84_000_000, embedding_model_id: null },
  hardware: { cpu_model: "Mock CPU", logical_processors: 8, total_ram_bytes: 16 * 2 ** 30, available_ram_bytes: 9 * 2 ** 30, operating_system: "Windows 11" },
  plan: {
    tier: "t2", tier_hardware_validated: true, available_bytes: 9 * 2 ** 30, model_budget_bytes: 4 * 2 ** 30,
    kv_budget_bytes: 2 ** 30, gpu_budget_bytes: 0, context_tokens: 8192, context_was_clamped: false,
    threads: 7, gpu_layers: 0, kv_quantization: "f16", warnings: [],
  },
  default_model: model,
  warnings: [],
  vault_initialized: true,
};

const snapshot = {
  tier_override: null, model_id: model.id, context_tokens: 8192, kv_quantization: "f16", theme: "dark",
  vault_path: "D:\\IAP\\vault.db",
};

const conversations = [
  { id: "c1", title: "Wochenplan besprechen", created_at_unix_ms: NOW - 2 * DAY, updated_at_unix_ms: NOW - DAY, project_id: null },
  { id: "c2", title: "Fehler in der Login-Funktion", created_at_unix_ms: NOW - 5 * DAY, updated_at_unix_ms: NOW - 4 * DAY, project_id: null },
];

const messages = [
  { id: "m1", conversation_id: "c1", position: 0, role: "user", content: "Was steht diese Woche an?", status: "complete", created_at_unix_ms: NOW - DAY },
  { id: "m2", conversation_id: "c1", position: 1, role: "assistant", content: "Diese Woche stehen **drei Termine** an:\n\n- Montag: Teambesprechung\n- Mittwoch: Zahnarzt\n- Freitag: Abgabe Bericht", status: "complete", created_at_unix_ms: NOW - DAY + 5_000 },
];

const events = [
  { id: "e1", title: "Teambesprechung", start_unix_ms: NOW + DAY, end_unix_ms: NOW + DAY + 3_600_000, description: "", location: "", all_day: false },
];

const calendarOverview = { supported: true, sources: [], air_gap: false };

/** Antworten je Befehl; was hier fehlt, beantwortet `fallback`. */
function answer(cmd: string, payload: Record<string, unknown> | undefined): unknown {
  switch (cmd) {
    case "bootstrap_status": return bootstrap;
    case "list_vaults": return [{ name: "vault.db", path: "D:\\IAP\\vault.db", is_default: true, initialized: true, size_bytes: 4_200_000 }];
    case "unlock_vault": return snapshot;
    case "settings_snapshot": return snapshot;
    case "apply_settings": return snapshot;
    case "installed_models": return [model];
    case "model_catalog": return catalog;
    case "get_ui_language": return "de";
    case "set_ui_language": return null;
    case "load_ui_state": return uiState.get(String(payload?.key)) ?? null;
    case "save_ui_state": uiState.set(String(payload?.key), String(payload?.value)); return null;
    case "list_conversations": return conversations;
    case "open_conversation": return { conversation: conversations[0], messages };
    case "get_user_profile": return { enabled: false, user_name: "", user_about: "", custom_system_prompt: "" };
    case "get_connector_config": return {
      offline_mode: true, exa_enabled: false, exa_api_key: "", wikipedia_enabled: false, open_meteo_enabled: false, brave_enabled: false, brave_api_key: "",
      gmail_enabled: false, gmail_address: "", gmail_target_email: "", gmail_check_interval_minutes: 5, gmail_reply_mode: "draft", gmail_send_acknowledged: false,
      gmail_max_per_hour: 5, gmail_max_per_day: 30, gmail_instruction: "", gmail_allow_read: false,
    };
    case "get_active_conversation": return null;
    case "set_active_conversation": return null;
    case "voice_block_reason": return null;
    case "voice_status": return { available: true, block: null, state: "idle", muted: false, packs: [], language: "de", voice_available: true, benchmark: null };
    case "mail_status": return { supported: true, enabled: false, has_password: false, polling: false, running: false, last_run_unix_ms: null, next_run_unix_ms: null, last_error: null, sent_last_hour: 0, sent_today: 0, blocked_reason: null };
    case "get_model_settings": return { model_id: model.id, context_tokens: null, temperature: 0.7, top_p: 0.9 };
    case "list_installed_skills": case "list_active_facts": case "list_projects": case "conversation_sources": case "conversation_documents": case "snapshot_list": case "code_agent_changes": return [];
    case "code_roots_status": return { host_path: null, stick_path: "D:\\IAP\\arbeit", approved: [] };
    case "code_list": case "list_workspace": return { root: "D:\\IAP\\arbeit", relative_path: String(payload?.relativePath ?? ""), entries: [
      { name: "src", relative_path: "src", is_directory: true, bytes: 0, modified_unix_ms: null },
      { name: "README.md", relative_path: "README.md", is_directory: false, bytes: 1204, modified_unix_ms: null },
      { name: "main.rs", relative_path: "main.rs", is_directory: false, bytes: 880, modified_unix_ms: null },
    ] };
    case "git_info": return { available: false, branch: null, note: "Der Arbeitsordner ist kein Git-Projekt." };
    case "host_traces": return { traces: [], total_bytes: 0, ask_on_exit: true };
    case "performance_report": return null;
    case "pending_permissions": return [];
    case "get_pet_style": return { shape: "galet", color: "auto", size: 124, show_cpu: false, show_ram: false, show_storage: false, show_task: false, refresh_secs: 3 };
    case "calendar_overview": return calendarOverview;
    case "calendar_events": return [];
    case "list_events": case "scheduler_events": return events;
    case "list_tasks": case "scheduler_tasks": return [];
    case "list_skills": return [];
    case "list_facts": return [];
    case "memory_graph": return { nodes: [], edges: [] };
    case "list_jobs": case "jobs_snapshot": return new URLSearchParams(location.search).has("job") ? [{ id: "j1", kind: "workflow", label: "Recherche mit Quellenprüfung", status: "running", queue_position: null, started_unix_ms: Date.now() - 83_000, cancelable: true }, { id: "j2", kind: "agent", label: "Prüfung auf leere Werte ergänzen", status: "waiting", queue_position: 1, started_unix_ms: Date.now(), cancelable: true }] : [];
    default: return fallback(cmd);
  }
}

/** Unbekannte Befehle: Listen werden leer, alles andere `null`; die Konsole nennt den Befehl. */
function fallback(cmd: string): unknown {
  console.warn(`[mock] Befehl ohne Antwort: ${cmd}`);
  return /^(list_|get_.*s$|.*_list$)/.test(cmd) ? [] : null;
}

/** Ersetzt das Backend durch feste Antworten. Der Fensterbezeichner ist `main`. */
export function installMockIpc(): void {
  mockWindows("main");
  mockIPC((cmd, payload) => answer(cmd, payload as Record<string, unknown> | undefined), { shouldMockEvents: true });
}
