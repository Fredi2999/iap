// Übersetzt technische Fehler in einen verständlichen Satz. Die Originalmeldung
// bleibt als Detail erhalten, damit Probleme weiterhin nachvollziehbar sind.

export interface FriendlyError {
  message: string;
  detail: string;
}

/** Fehler mit stabilem Code aus dem Backend (`AppError::Coded`, app/src-tauri/src/error_code.rs). */
export interface BackendError {
  code: string;
  message: string;
}

export function isBackendError(error: unknown): error is BackendError {
  if (typeof error !== "object" || error === null || error instanceof Error) return false;
  const candidate = error as Record<string, unknown>;
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}

// Ein Eintrag je Code aus error_code.rs. Fehlt ein Code hier, greift der Text des Backends.
const CODE_MESSAGES: Record<string, string> = {
  vault_locked: "Der Tresor ist gesperrt. Bitte zuerst entsperren.",
  wrong_passphrase: "Das Passwort passt nicht zu diesem Tresor.",
  vault_media_unavailable: "Der Stick ist nicht erreichbar. Bitte prüfe die Verbindung.",
  vault_integrity: "Die Prüfung des Tresors ist fehlgeschlagen. Nutze eine Sicherung.",
  hotkey_taken: "Das Tastenkürzel ist bereits von einem anderen Programm belegt.",
};

/**
 * Rohtext eines Fehlers für Meldungen und Protokolle. `String(error)` ergäbe bei einem Fehler mit
 * Code „[object Object]“.
 */
export function errorText(error: unknown): string {
  if (isBackendError(error)) return error.message;
  if (error instanceof Error) return error.message;
  return String(error ?? "");
}

const PATTERNS: { test: RegExp; message: string }[] = [
  { test: /vault.*(lock|gesperrt|not unlocked)|no active session|keine aktive sitzung/i, message: "Der Tresor ist gesperrt. Bitte zuerst entsperren." },
  { test: /wrong passphrase|invalid passphrase|falsche[sn]? passwort|file is not a database|decrypt|entschlüsselung fehl/i, message: "Das Passwort passt nicht zu diesem Tresor." },
  { test: /nicht installiert|not installed|model.*missing|gguf/i, message: "Das gewählte Modell ist auf diesem Stick nicht vollständig vorhanden." },
  { test: /timeout|timed out|zeitüberschreitung/i, message: "Das lokale Modell hat zu lange nicht geantwortet." },
  { test: /connection refused|transport|process_exited|server.*(exit|beendet)/i, message: "Das lokale Modell läuft gerade nicht. IAP versucht, es neu zu starten." },
  { test: /policy|denied|verweigert|nicht erlaubt/i, message: "Diese Aktion ist durch die Sicherheitsregeln gesperrt." },
  { test: /air ?gap|offline/i, message: "Im Air Gap sind externe Dienste gesperrt." },
  { test: /no such file|not found|nicht gefunden/i, message: "Die Datei oder der Ordner wurde nicht gefunden." },
  { test: /invoke|__TAURI|ipc/i, message: "Die Verbindung zum IAP-Kern ist nicht verfügbar." },
];

export function friendlyError(error: unknown): FriendlyError {
  if (isBackendError(error)) {
    const known = CODE_MESSAGES[error.code];
    return { message: known ?? error.message, detail: `${error.code}: ${error.message}` };
  }
  const detail = error instanceof Error ? `${error.name}: ${error.message}` : String(error ?? "");
  const match = PATTERNS.find((pattern) => pattern.test.test(detail));
  if (match) return { message: match.message, detail };
  // Bereits verständliche Sätze aus der Oberfläche (z. B. Eingabeprüfungen) bleiben unverändert.
  if (typeof error === "string" && error.length < 200 && /^[A-ZÄÖÜ][^{}\[\]<>]*[.!?]$/.test(error) && !/error|exception|panic/i.test(error)) {
    return { message: error, detail: "" };
  }
  return { message: "Das hat nicht geklappt. Bitte versuche es noch einmal.", detail };
}
