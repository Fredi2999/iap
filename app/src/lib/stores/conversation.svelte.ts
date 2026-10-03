// Gemeinsame, aktive Unterhaltung für Hauptseite, Chat und Pet.
//
// Der Zustand liegt im Backend (`get/set_active_conversation`), damit ein
// Ansichtswechsel oder das kleine Pet-Fenster dieselbe Unterhaltung sieht. Die
// Entwürfe bleiben je Ansicht erhalten, auch wenn sie nicht sichtbar ist.

import { getActiveConversation, onActiveConversation, setActiveConversation } from "../ipc";
import type { UnlistenFn } from "@tauri-apps/api/event";

export const conversation = $state<{ activeId: string | null; ready: boolean }>({ activeId: null, ready: false });

/** Ungesendete Eingaben, je Ansicht getrennt. */
export const drafts = $state<{ home: string }>({ home: "" });

let unlisten: UnlistenFn | null = null;

/** Lädt die aktive Unterhaltung und hört auf Änderungen aus anderen Fenstern. */
export async function initConversationStore(): Promise<void> {
  try {
    conversation.activeId = await getActiveConversation();
  } catch (error) {
    console.error(error);
  }
  conversation.ready = true;
  unlisten?.();
  try {
    unlisten = await onActiveConversation((id) => {
      conversation.activeId = id;
    });
  } catch (error) {
    console.error(error);
  }
}

/** Setzt die aktive Unterhaltung lokal und im Backend. */
export async function setActive(id: string | null): Promise<void> {
  if (conversation.activeId === id) return;
  conversation.activeId = id;
  try {
    await setActiveConversation(id);
  } catch (error) {
    console.error(error);
  }
}
