// Gemeinsamer Modellzustand der beiden Eingabeleisten im Code-Bereich (Agent und „IAP fragen“).
// Beide Leisten steuern dasselbe aktive Modell; ein gemeinsamer `busy`-Zustand verhindert,
// dass in der einen ein Wechsel läuft, während die andere noch einen startet.
import { installedModels, selectModel } from "../ipc";
import type { AvailableModel, SettingsSnapshot } from "../types";

export const codeModels = $state<{ models: AvailableModel[]; busy: boolean }>({ models: [], busy: false });

/** Lädt die installierten Modelle neu, damit nachträglich auf den Stick kopierte Modelle erscheinen. */
export async function loadCodeModels(): Promise<void> {
  codeModels.models = await installedModels();
}

/** Wechselt das aktive Modell; `null`, wenn nichts zu tun war (gleiches Modell oder Wechsel läuft schon). */
export async function switchCodeModel(id: string, currentId: string): Promise<SettingsSnapshot | null> {
  if (codeModels.busy || id === currentId) return null;
  codeModels.busy = true;
  try {
    return await selectModel(id);
  } finally {
    codeModels.busy = false;
  }
}
