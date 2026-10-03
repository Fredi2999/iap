import { tk } from "./i18n/index.svelte";
// Zuordnung der App-Zustände zu Bloub-Animationen und Statustexten.
//
// Die Animation ist nur Darstellung. Sie bekommt einen Zustand und hat keine
// Aufnahme-, Bildschirm- oder Werkzeugrechte.
import type { StateId } from "./vendor/bloub/states";
import type { AvatarState } from "./types";

/** Welche Bloub-Animation zu welchem echten Zustand gehört. */
export const ENGINE_STATE: Record<AvatarState, StateId> = {
  ready: "idle",
  listening: "wide",
  transcribing: "thinking",
  processing: "orbit",
  speaking: "egg",
  awaiting_approval: "alert",
  stopped: "sleep",
  error: "exclaim",
};

/** Textstatus für Bildschirmleser und Sichtbarkeit ohne Animation (deutscher Schlüssel für `t()`). */
export const STATE_TEXT: Record<AvatarState, string> = {
  ready: tk("Bereit"),
  listening: tk("Hört zu"),
  transcribing: tk("Erkennt Sprache"),
  processing: tk("Verarbeitet"),
  speaking: tk("Spricht"),
  awaiting_approval: tk("Wartet auf Freigabe"),
  stopped: tk("Gestoppt"),
  error: tk("Fehler"),
};
