// Zentrale Beschreibung der Navigation: sechs Ziele statt achtzehn gleichrangiger
// Einträge. Die Routen-IDs bleiben unverändert, damit Befehlssuche und interne
// Sprünge (z. B. Chat -> Einstellungen) weiter funktionieren.
import type { Route } from "./types";

export type DestinationId = "home" | "flow" | "chat" | "knowledge" | "code" | "tools" | "calendar" | "files" | "settings";

export interface Destination {
  id: DestinationId;
  label: string;
  hint: string;
  icon: string;
  routes: Route[];
  /// Nicht als Eintrag in der Seitenleiste zeigen (Flow Version hat einen eigenen Knopf).
  hidden?: boolean;
}

export const ROUTE_LABELS: Record<Route, string> = {
  home: "Startseite",
  flow: "Flow Version",
  chat: "Chat",
  palace: "Wissensnetz",
  memory: "Fakten",
  pci: "PCI",
  import: "Dokumente",
  scratch: "Notizen",
  voice: "Sprachnotizen",
  digests: "Zusammenfassungen",
  rubric: "Bewertung",
  code: "Code",
  focus: "Fokus",
  skills: "Skills",
  templates: "Vorlagen",
  connectors: "Konnektoren",
  calendar: "Kalender",
  files: "Dateien",
  settings: "Allgemein",
  updates: "Updates und Sicherung",
  logs: "Protokoll",
};

/** Kurzer Zweck je Route; erscheint in Übersichten und in der Befehlssuche. */
export const ROUTE_HINTS: Record<Route, string> = {
  home: "Avatar, Sprache und Status",
  flow: "Agent Flow und Workflows",
  chat: "Mit dem lokalen Modell sprechen",
  palace: "Gemerkte Fakten als Netz",
  memory: "Was IAP sich merkt, prüfen und bearbeiten",
  pci: "Aktivität von PCs, auf denen der Begleiter lief",
  import: "Eigene Dateien als Quellen für den Chat",
  scratch: "Schnelle Notizen, die IAP lernen kann",
  voice: "Sprachaufnahmen während der Sitzung",
  digests: "Unterhaltungen kurz zusammenfassen",
  rubric: "Ideen und Pläne nach Kriterien bewerten",
  code: "Dateien bearbeiten, Änderungen prüfen, sichern",
  focus: "Konzentriert arbeiten mit Timer und Schnellfrage",
  skills: "Erweiterungen installieren und testen",
  templates: "Häufige Aufträge per „/“ abrufen",
  connectors: "Externe Dienste, im Air Gap gesperrt",
  calendar: "Aufgaben sammeln und Wochenplan erstellen",
  files: "Arbeitsordner auf dem Stick",
  settings: "Darstellung, Modell und Profil",
  updates: "Updates prüfen, Sicherungen und Export",
  logs: "Nachvollziehen, was IAP getan hat",
};

/** Symbol je Route (SVG-Pfad, 24er Raster, Strichzeichnung). */
export const ROUTE_ICONS: Record<Route, string> = {
  home: "M12 3a7 7 0 0 0-7 7v3.2C5 17 8 21 12 21s7-4 7-7.8V10a7 7 0 0 0-7-7ZM9 11h.01M15 11h.01M9.5 15c1.5 1 3.5 1 5 0",
  flow: "M6 6a2 2 0 1 0 0 .01M18 6a2 2 0 1 0 0 .01M12 18a2 2 0 1 0 0 .01M6 8v2a4 4 0 0 0 4 4h4a4 4 0 0 0 4-4V8M12 14v2",
  chat: "M4 5.5A2.5 2.5 0 0 1 6.5 3h11A2.5 2.5 0 0 1 20 5.5v9A2.5 2.5 0 0 1 17.5 17H9l-5 4v-4.5a2.5 2.5 0 0 1 0-1V5.5Z",
  focus: "M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M5.6 18.4l2.1-2.1M16.3 7.7l2.1-2.1",
  digests: "M4 6h16M4 12h12M4 18h8",
  palace: "M12 4l8 4v8l-8 4-8-4V8l8-4ZM12 12v8M12 12l8-4M12 12L4 8",
  memory: "M8 4a4 4 0 0 0-4 4v8a4 4 0 0 0 4 4h8a4 4 0 0 0 4-4V8a4 4 0 0 0-4-4H8Zm2 4h4v2h-4V8Zm-2 4h8v2H8v-2Z",
  pci: "M4 5h16v10H4V5Zm5 14h6M12 15v4M8 9l2 2 4-4",
  import: "M12 3v12M6 9l6 6 6-6M4 21h16",
  voice: "M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3ZM5 11a7 7 0 0 0 14 0M12 18v3M8 21h8",
  scratch: "M4 3h12l4 4v14H4ZM16 3v4h4M8 12h8M8 16h8M8 8h4",
  rubric: "M5 4h14v3H5V4Zm0 6.5h14v3H5v-3ZM5 17h14v3H5v-3Z",
  code: "M9.5 8 5 12l4.5 4M14.5 8 19 12l-4.5 4M13 4l-2 16",
  calendar: "M7 3v3M17 3v3M4 8h16M5 6h14a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1Z",
  connectors: "M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71",
  skills: "M12 3l2.5 5.5L20 9.5l-4 4 1 6L12 16.5 7 19.5l1-6-4-4 5.5-1L12 3Z",
  templates: "M6 3h9l4 4v14H6V3ZM15 3v4h4M9 12h6M9 16h4",
  files: "M4 5a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5Z",
  updates: "M20 12a8 8 0 1 1-2.3-5.6M20 4v5h-5",
  logs: "M5 4h14v16H5V4Zm2 3h10v2H7V7Zm0 4h10v2H7v-2Zm0 4h7v2H7v-2Z",
  settings: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Zm0-4 1.5 2.5L16 5l1 2.5 2.5 1L18 11l1 2.5-2.5 1L15.5 17 12 15l-3.5 2-1.5-2.5L4.5 14 6 11 4.5 8.5 7 7l1.5-2.5L11 6l1-2Z",
};

export const DESTINATIONS: Destination[] = [
  {
    id: "home", label: "Startseite", hint: "Avatar und Sprache", routes: ["home"],
    icon: "M12 3a7 7 0 0 0-7 7v3.2C5 17 8 21 12 21s7-4 7-7.8V10a7 7 0 0 0-7-7ZM9 11h.01M15 11h.01M9.5 15c1.5 1 3.5 1 5 0",
  },
  {
    id: "chat", label: "Chat", hint: "Mit IAP sprechen", routes: ["chat"],
    icon: "M4 5.5A2.5 2.5 0 0 1 6.5 3h11A2.5 2.5 0 0 1 20 5.5v9A2.5 2.5 0 0 1 17.5 17H9l-5 4v-4.5a2.5 2.5 0 0 1 0-1V5.5Z",
  },
  {
    id: "knowledge", label: "Gedächtnis", hint: "Fakten und Dokumente", routes: ["palace", "memory", "pci", "import", "scratch", "voice", "digests"],
    icon: "M12 4l8 4v8l-8 4-8-4V8l8-4ZM12 12v8M12 12l8-4M12 12L4 8",
  },
  {
    id: "code", label: "Code", hint: "Dateien bearbeiten und sichern", routes: ["code"],
    icon: "M8 6 3 12l5 6M16 6l5 6-5 6M13.5 4l-3 16",
  },
  {
    id: "tools", label: "Werkzeuge", hint: "Skills, Vorlagen und mehr", routes: ["rubric", "focus", "templates", "skills", "connectors"],
    icon: "M14.7 6.3a4 4 0 0 0-5.4 5.4L4 17v3h3l5.3-5.3a4 4 0 0 0 5.4-5.4l-2.5 2.5-2.1-.4-.4-2.1 2.5-2.5Z",
  },
  {
    id: "calendar", label: "Kalender", hint: "Aufgaben planen", routes: ["calendar"],
    icon: "M7 3v3M17 3v3M4 8h16M5 6h14a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1Z",
  },
  {
    id: "files", label: "Dateien", hint: "Ordner auf dem Stick", routes: ["files"],
    icon: "M4 5a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5Z",
  },
  {
    id: "flow", label: "Flow Version", hint: "Agent Flow und Workflows", routes: ["flow"], hidden: true,
    icon: "M6 6a2 2 0 1 0 0 .01M18 6a2 2 0 1 0 0 .01M12 18a2 2 0 1 0 0 .01M6 8v2a4 4 0 0 0 4 4h4a4 4 0 0 0 4-4V8M12 14v2",
  },
  {
    id: "settings", label: "Einstellungen", hint: "Darstellung und Modell", routes: ["settings", "updates", "logs"],
    icon: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Zm0-4 1.5 2.5L16 5l1 2.5 2.5 1L18 11l1 2.5-2.5 1L15.5 17 12 15l-3.5 2-1.5-2.5L4.5 14 6 11 4.5 8.5 7 7l1.5-2.5L11 6l1-2Z",
  },
];

/** Findet das Navigationsziel, zu dem eine Route gehört. */
export function destinationOf(route: Route): Destination {
  return DESTINATIONS.find((destination) => destination.routes.includes(route)) ?? DESTINATIONS[0];
}

/** Alle Routen mit Ziel, für die Befehlssuche. */
export const ROUTE_INDEX = DESTINATIONS.flatMap((destination) =>
  destination.routes.map((route) => ({ id: route, label: ROUTE_LABELS[route], hint: ROUTE_HINTS[route], group: destination.label, icon: ROUTE_ICONS[route] })),
);
