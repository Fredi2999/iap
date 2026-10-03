// Mehrsprachigkeit der Oberfläche.
//
// Der deutsche Text ist zugleich der Schlüssel: t("Neue Unterhaltung") liefert
// die Übersetzung der aktiven Sprache oder, falls sie fehlt, den deutschen Text.
// So bleibt der Quelltext lesbar und fehlende Übersetzungen brechen nichts.
// Platzhalter stehen in geschweiften Klammern: t("{n} Fakten", { n: 3 }).
import { en } from "./en";
import { es } from "./es";
import { fr } from "./fr";
import { ja } from "./ja";

export type Language = "de" | "en" | "es" | "fr" | "ja";

export const LANGUAGES: { id: Language; label: string; short: string }[] = [
  { id: "de", label: "Deutsch", short: "DE" },
  { id: "en", label: "English", short: "EN" },
  { id: "es", label: "Español", short: "ES" },
  { id: "fr", label: "Français", short: "FR" },
  { id: "ja", label: "日本語", short: "日本" },
];

const DICTIONARIES: Record<Exclude<Language, "de">, Record<string, string>> = { en, es, fr, ja };

const LOCALES: Record<Language, string> = { de: "de-AT", en: "en-GB", es: "es-ES", fr: "fr-FR", ja: "ja-JP" };

export const i18n = $state({ lang: "de" as Language });

/** Übersetzt einen deutschen Text in die aktive Sprache. */
export function t(text: string, params?: Record<string, string | number>): string {
  const lang = i18n.lang;
  const translated = lang === "de" ? text : DICTIONARIES[lang][text] ?? text;
  if (!params) return translated;
  return translated.replace(/\{(\w+)\}/g, (match, key: string) => (key in params ? String(params[key]) : match));
}

/** Gebietsschema für Datum, Uhrzeit und Zahlen in der aktiven Sprache. */
export function locale(): string {
  return LOCALES[i18n.lang];
}

/** Setzt die Sprache und das lang-Attribut, damit Screenreader und Silbentrennung stimmen. */
export function setLanguage(lang: string) {
  const next = (LANGUAGES.find((entry) => entry.id === lang)?.id ?? "de") as Language;
  i18n.lang = next;
  if (typeof document !== "undefined") document.documentElement.lang = next;
}

/** Markiert einen Text in Datentabellen als Übersetzungsschlüssel; übersetzt wird beim Anzeigen mit t(). */
export function tk(text: string): string {
  return text;
}
