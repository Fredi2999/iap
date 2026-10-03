// Sprachsitzung: Zustand aus dem Backend plus die kleinen Abläufe.
//
// Das Backend entscheidet über Freigaben (Tresor, Paket, T0-Tor, Ticket) und
// führt aus. Dieser Store zeigt nur den Zustand und ruft die Befehle auf.
// Der erkannte Text geht an den Aufrufer. Normales Diktat landet dort als editierbarer
// Entwurf; nur in der Sprachsitzung („Mit mir reden“) und im Pet wird er sofort gesendet.

import type { UnlistenFn } from "@tauri-apps/api/event";
import { errorText } from "../errors";
import {
  onVoiceState,
  voiceBlockReason,
  voiceDiscard,
  voiceSetMuted,
  voiceSpeak,
  voiceStartCapture,
  voiceStatus,
  voiceStopCapture,
  voiceStopOutput,
} from "../ipc";
import type { Transcript, VoiceCommand, VoiceState, VoiceStatus } from "../types";

export const voice = $state<{
  status: VoiceStatus | null;
  state: VoiceState;
  muted: boolean;
  elapsedMs: number;
  level: number;
  /** Letzter Fehler als Klartext für die Anzeige. */
  error: string | null;
  /** „Mit mir reden“: erkannte Äußerungen werden beantwortet und vorgelesen. */
  conversationMode: boolean;
  /** Warum Sprache gesperrt ist (Klartext), sonst `null`. */
  blockReason: string | null;
  /** Hinweis aus dem Backend, z. B. `max_duration`. */
  notice: string | null;
}>({ status: null, state: "idle", muted: false, elapsedMs: 0, level: 0, error: null, conversationMode: false, blockReason: null, notice: null });

/** Wer Navigationsbefehle ausführt (setzt App.svelte). Sprache löst nur harmlose Ansichtswechsel aus. */
export const voiceCommands: { handler: ((command: VoiceCommand) => void) | null } = { handler: null };

let unlisten: UnlistenFn | null = null;

export async function initVoiceStore(): Promise<void> {
  await refreshVoiceStatus();
  unlisten?.();
  try {
    unlisten = await onVoiceState((event) => {
      voice.state = event.state;
      voice.muted = event.muted;
      voice.elapsedMs = event.elapsed_ms;
      voice.level = event.level;
      voice.notice = event.message;
    });
  } catch (error) {
    console.error(error);
  }
}

export async function refreshVoiceStatus(): Promise<void> {
  try {
    voice.status = await voiceStatus();
    voice.state = voice.status.state;
    voice.muted = voice.status.muted;
    voice.blockReason = await voiceBlockReason();
  } catch (error) {
    voice.status = null;
    console.error(error);
  }
}

function message(error: unknown): string {
  return typeof error === "string" ? error : errorText(error);
}

/** Startet eine einzelne, sichtbare Aufnahme. Gibt `false` zurück, wenn sie nicht startet. */
export async function startListening(): Promise<boolean> {
  voice.error = null;
  try {
    await voiceStartCapture();
    return true;
  } catch (error) {
    voice.error = message(error);
    await refreshVoiceStatus();
    return false;
  }
}

/** Beendet die Aufnahme und liefert den erkannten Text (nicht gesendet). */
export async function stopListening(): Promise<Transcript | null> {
  try {
    return await voiceStopCapture();
  } catch (error) {
    voice.error = message(error);
    return null;
  }
}

/** Verwirft Aufnahme oder laufende Erkennung ohne Ergebnis. */
export async function discardListening(): Promise<void> {
  try {
    await voiceDiscard();
  } catch (error) {
    voice.error = message(error);
  }
}

/** Liest einen Text lokal vor; Fehler (z. B. keine Stimme) erscheinen als Hinweis. */
export async function speak(text: string): Promise<void> {
  if (voice.muted) return;
  try {
    await voiceSpeak(text);
  } catch (error) {
    voice.error = message(error);
  }
}

export async function stopSpeaking(): Promise<void> {
  try {
    await voiceStopOutput();
  } catch (error) {
    voice.error = message(error);
  }
}

export async function setMuted(muted: boolean): Promise<void> {
  voice.muted = muted;
  try {
    await voiceSetMuted(muted);
  } catch (error) {
    voice.error = message(error);
  }
}

/** Beendet Sprachsitzung, Aufnahme und Ausgabe. */
export async function endConversationMode(): Promise<void> {
  voice.conversationMode = false;
  await discardListening();
  await stopSpeaking();
}
