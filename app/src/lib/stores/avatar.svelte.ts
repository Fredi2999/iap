// Avatar-Zustand aus echten App-Ereignissen.
//
// Der Avatar behauptet nichts: Er zeigt nur, was tatsächlich läuft (Aufnahme,
// Erkennung, Antwort, Vorlesen, offene Freigabe, Abbruch, Fehler). Diese Datei
// sammelt die Ereignisse; die Darstellung (Bloub) bekommt nur den Zustand.

import type { UnlistenFn } from "@tauri-apps/api/event";
import { subscribeToStream, subscribeToToolStream } from "../ipc";
import type { AvatarState } from "../types";
import { speak, voice } from "./voice.svelte";

export const avatar = $state<{ streaming: boolean; awaitingApproval: boolean; failed: boolean; stopped: boolean }>({
  streaming: false,
  awaitingApproval: false,
  failed: false,
  stopped: false,
});

let unlistenStream: UnlistenFn | null = null;
let unlistenTools: UnlistenFn | null = null;
let stoppedTimer: number | undefined;
let failedTimer: number | undefined;

/** Zeigt kurz „gestoppt“, etwa nach einem Abbruch. */
export function markStopped(): void {
  avatar.stopped = true;
  window.clearTimeout(stoppedTimer);
  stoppedTimer = window.setTimeout(() => (avatar.stopped = false), 2500);
}

/** Zeigt kurz „Fehler“. */
export function markFailed(): void {
  avatar.failed = true;
  window.clearTimeout(failedTimer);
  failedTimer = window.setTimeout(() => (avatar.failed = false), 4000);
}

export async function initAvatarStore(): Promise<void> {
  unlistenStream?.();
  unlistenTools?.();
  try {
    unlistenStream = await subscribeToStream((event) => {
      if (event.kind === "started") avatar.streaming = true;
      else if (event.kind === "finished") {
        avatar.streaming = false;
        avatar.awaitingApproval = false;
        if (event.outcome.aborted) markStopped();
        // In der Sprachsitzung wird die fertige Antwort lokal vorgelesen (nie eine abgebrochene).
        else if (voice.conversationMode) void speak(event.outcome.text);
      } else if (event.kind === "failed") {
        avatar.streaming = false;
        avatar.awaitingApproval = false;
        markFailed();
      }
    });
    unlistenTools = await subscribeToToolStream((event) => {
      if (event.kind === "permission_requested") avatar.awaitingApproval = true;
      else avatar.awaitingApproval = false;
    });
  } catch (error) {
    console.error(error);
  }
}

/** Der Zustand, den der Avatar gerade zeigt. Fehler und Freigaben haben Vorrang. */
export function currentAvatarState(): AvatarState {
  if (avatar.failed || (voice.error !== null && voice.state === "idle")) return "error";
  if (avatar.awaitingApproval) return "awaiting_approval";
  if (voice.state === "listening") return "listening";
  if (voice.state === "transcribing") return "transcribing";
  if (voice.state === "speaking") return "speaking";
  if (avatar.streaming || voice.state === "processing") return "processing";
  if (avatar.stopped) return "stopped";
  return "ready";
}
