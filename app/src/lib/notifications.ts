import { t } from "./i18n/index.svelte";

export type NoticeTone = "info" | "success" | "error";
export interface Notice { id: number; title: string; description?: string; tone: NoticeTone; }

// Lokale UI-Ereignisse; Benachrichtigungen verlassen den Prozess nicht.
// Titel und Text werden hier übersetzt, damit Aufrufer deutsche Schlüssel übergeben können.
export function notify(title: string, description = "", tone: NoticeTone = "info") {
  window.dispatchEvent(new CustomEvent("iap-notify", { detail: { title: t(title), description: description ? t(description) : "", tone } }));
}
