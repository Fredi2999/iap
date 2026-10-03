// Termine aus verbundenen Kalendern (Apple, Google) in die Form der Kalenderansichten bringen
// und die kommenden Termine für die Startseite auswählen. Alles lokal, ohne Netz.

// Bewusst ohne Wert-Importe, damit `node --experimental-strip-types --test` die Datei direkt lädt.
import type { CalendarSource, RemoteEvent, SchedulerEvent } from "./types";

const DAY_MS = 86_400_000;

/** Ein Kalender, in den IAP Termine schreiben darf. */
export interface CalendarTarget {
  sourceId: string;
  href: string;
  label: string;
}

/** Alle beschreibbaren Kalender der verbundenen Konten, mit Anzeigename. */
export function writableTargets(sources: CalendarSource[]): CalendarTarget[] {
  return sources
    .filter((source) => source.kind === "icloud")
    .flatMap((source) => source.calendars.filter((calendar) => calendar.can_write).map((calendar) => ({
      sourceId: source.id,
      href: calendar.href,
      label: `${source.label} · ${calendar.name}`,
    })));
}

/** Kürzeste angezeigte Länge eines Termins ohne Dauer. */
const MIN_MS = 15 * 60_000;

/** Beginn des lokalen Tages für `YYYY-MM-DD`; `null` bei ungültiger Eingabe. Sommerzeit-sicher. */
export function localDayStart(iso: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  if (!match) return null;
  const ms = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]), 0, 0, 0, 0).getTime();
  return Number.isNaN(ms) ? null : ms;
}

/**
 * Ein verbundener Termin als Eintrag der Kalenderansichten. Ganztagstermine werden über ihre
 * Daten in lokale Mitternacht umgerechnet (nicht über die Näherung des Backends), damit sie in
 * jeder Jahreszeit am richtigen Tag stehen.
 */
export function remoteToItem(event: RemoteEvent): SchedulerEvent {
  let start = event.start_unix_ms;
  let end = event.end_unix_ms;
  if (event.all_day) {
    const from = event.start_date ? localDayStart(event.start_date) : null;
    const to = event.end_date ? localDayStart(event.end_date) : null;
    if (from !== null) start = from;
    if (to !== null && to > start) end = to;
    else end = Math.max(end, start + DAY_MS);
  } else {
    end = Math.max(end, start + MIN_MS);
  }
  return {
    id: event.id,
    title: event.title,
    start_unix_ms: start,
    end_unix_ms: end,
    location: event.location ?? null,
    project: event.calendar_name,
    external_uid: null,
    all_day: event.all_day,
    remote: event,
  };
}

/** Was der Wochenplaner wissen muss: nur Termine mit Uhrzeit (Ganztag blockiert den Tag nicht). */
export function planningEvents(items: SchedulerEvent[]): SchedulerEvent[] {
  return items
    .filter((item) => !item.all_day)
    .map(({ id, title, start_unix_ms, end_unix_ms, location, project, external_uid }) => ({
      id, title, start_unix_ms, end_unix_ms, location, project, external_uid,
    }));
}

/** Die nächsten Termine ab `now` innerhalb von `days` Tagen, nach Beginn sortiert. */
export function upcomingEvents(items: SchedulerEvent[], now: number, days: number, limit: number): SchedulerEvent[] {
  const until = now + days * DAY_MS;
  return items
    .filter((item) => item.end_unix_ms > now && item.start_unix_ms < until)
    .sort((a, b) => a.start_unix_ms - b.start_unix_ms || a.title.localeCompare(b.title))
    .slice(0, limit);
}

/**
 * Fehlertext für die Kalender-Oberfläche. Die Meldungen des Backends sind schon verständliches
 * Deutsch (und enthalten nie Zugangsdaten); das Präfix der Eingabeprüfung wird entfernt. Alles
 * Technische (Aufruffehler, Abstürze) wird durch einen allgemeinen Satz ersetzt.
 */
export function calendarError(reason: unknown): string {
  const text = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "";
  const stripped = text.replace(/^Ungültige Eingabe:\s*/, "").replace(/^Vault-Fehler:\s*/, "").trim();
  if (stripped && stripped.length < 400 && !/__TAURI|invoke|panic|exception|internal/i.test(stripped)) return stripped;
  return "Das hat nicht geklappt. Bitte versuche es noch einmal.";
}

/** Versatz der lokalen Zeit zu UTC in Minuten (Wien im Winter: 60). */
export function localOffsetMinutes(): number {
  return -new Date().getTimezoneOffset();
}
