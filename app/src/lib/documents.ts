// Gemeinsame Helfer für Dokumente (Bibliothek und Chat).
import { locale } from "./i18n/index.svelte";

/** Dateitypen für den Auswahldialog; entspricht dem, was das Backend lesen kann. */
export const DOCUMENT_ACCEPT = ".txt,.md,.markdown,.docx,.pdf";

export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
  return `${value.toLocaleString(locale(), { maximumFractionDigits: unit >= 2 ? 1 : 0 })} ${units[unit]}`;
}
