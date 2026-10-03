// Anzeigenamen für interne Werte, damit die Oberfläche keine englischen
// Schlüssel wie "preference" oder "deny" zeigt.
import type { FactCategory } from "./types";

export const FACT_CATEGORIES: FactCategory[] = ["preference", "project", "person", "skill", "constraint", "other"];

export const FACT_CATEGORY_LABELS: Record<FactCategory, string> = {
  preference: "Vorliebe",
  project: "Projekt",
  person: "Person",
  skill: "Fähigkeit",
  constraint: "Einschränkung",
  other: "Sonstiges",
};

export const AUDIT_OUTCOME_LABELS: Record<string, string> = {
  allow: "Erlaubt",
  prompt: "Nachgefragt",
  deny: "Abgelehnt",
};

export const AUDIT_ACTION_LABELS: Record<string, string> = {
  file_read: "Datei gelesen",
  file_write: "Datei geschrieben",
  file_list: "Ordner aufgelistet",
  pure: "Reine Berechnung",
};
