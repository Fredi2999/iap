// Datumslogik für die Kalenderansichten. Bewusst ohne Bibliothek: alles in lokaler
// Zeit über `Date`, Wochenbeginn Montag. Zeitstempel sind wie im Planer Unix-Millisekunden.

export const DAY_MS = 86_400_000;

/** Beginn des lokalen Tages (0:00 Uhr). */
export function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** Addiert Kalendertage; Sommerzeitwechsel verschieben dabei nicht die Uhrzeit. */
export function addDays(ms: number, days: number): number {
  const d = new Date(ms);
  d.setDate(d.getDate() + days);
  return d.getTime();
}

/** Montag 0:00 der Woche, in der `ms` liegt. */
export function startOfWeek(ms: number): number {
  const d = new Date(startOfDay(ms));
  const sinceMonday = (d.getDay() + 6) % 7;
  d.setDate(d.getDate() - sinceMonday);
  return d.getTime();
}

export function startOfMonth(ms: number): number {
  const d = new Date(startOfDay(ms));
  d.setDate(1);
  return d.getTime();
}

/** Verschiebt um ganze Monate und bleibt immer auf dem 1. des Monats. */
export function addMonths(ms: number, months: number): number {
  const d = new Date(startOfMonth(ms));
  d.setMonth(d.getMonth() + months);
  return d.getTime();
}

export function isSameDay(a: number, b: number): boolean {
  return startOfDay(a) === startOfDay(b);
}

/** 42 Tage (6 Wochen ab Montag), die das Raster eines Monats füllen. */
export function monthGrid(monthMs: number): number[] {
  const first = startOfWeek(startOfMonth(monthMs));
  return Array.from({ length: 42 }, (_, i) => addDays(first, i));
}

/** Sieben Tage ab dem Montag der Woche von `ms`. */
export function weekDays(ms: number): number[] {
  const first = startOfWeek(ms);
  return Array.from({ length: 7 }, (_, i) => addDays(first, i));
}

interface Span {
  start_unix_ms: number;
  end_unix_ms: number;
}

/** Alles, was den Tag `dayMs` berührt, nach Beginn sortiert. */
export function spansOnDay<T extends Span>(items: T[], dayMs: number): T[] {
  const from = startOfDay(dayMs);
  const to = addDays(from, 1);
  return items
    .filter((item) => item.start_unix_ms < to && item.end_unix_ms > from)
    .sort((a, b) => a.start_unix_ms - b.start_unix_ms || a.end_unix_ms - b.end_unix_ms);
}

/**
 * Fasst sich überschneidende Termine zu belegten Blöcken zusammen. Der Planer
 * lehnt überlappende Termine ab, im echten Kalender kommen sie aber vor; für die
 * Planung zählt nur, dass die Zeit belegt ist.
 */
export function mergeBusy<T extends Span & { id: string; title: string }>(events: T[]): T[] {
  const sorted = [...events].sort((a, b) => a.start_unix_ms - b.start_unix_ms);
  const merged: T[] = [];
  for (const event of sorted) {
    const last = merged[merged.length - 1];
    if (last && event.start_unix_ms < last.end_unix_ms) {
      if (event.end_unix_ms > last.end_unix_ms) last.end_unix_ms = event.end_unix_ms;
    } else {
      merged.push({ ...event });
    }
  }
  return merged;
}

export interface Placed<T> {
  item: T;
  /** Spalte innerhalb einer Gruppe sich überlappender Einträge. */
  column: number;
  /** Anzahl der Spalten dieser Gruppe. */
  columns: number;
}

/** Legt Einträge eines Tages nebeneinander, wenn sie sich zeitlich überlappen. */
export function layoutOverlaps<T extends Span>(items: T[]): Placed<T>[] {
  const sorted = [...items].sort((a, b) => a.start_unix_ms - b.start_unix_ms || b.end_unix_ms - a.end_unix_ms);
  const result: Placed<T>[] = [];
  let group: Placed<T>[] = [];
  let groupEnd = -Infinity;
  const flush = () => {
    const columns = group.reduce((max, p) => Math.max(max, p.column + 1), 1);
    for (const p of group) p.columns = columns;
    result.push(...group);
    group = [];
  };
  for (const item of sorted) {
    if (item.start_unix_ms >= groupEnd) flush();
    const taken = new Set(group.filter((p) => p.item.end_unix_ms > item.start_unix_ms).map((p) => p.column));
    let column = 0;
    while (taken.has(column)) column += 1;
    group.push({ item, column, columns: 1 });
    groupEnd = Math.max(groupEnd, item.end_unix_ms);
  }
  flush();
  return result;
}

/** Wert für `<input type="datetime-local">` (lokale Zeit, ohne Sekunden). */
export function toLocalInput(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** Gegenstück zu `toLocalInput`; `null` bei ungültiger Eingabe. */
export function fromLocalInput(value: string): number | null {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(value)) return null;
  const ms = new Date(value).getTime();
  return Number.isNaN(ms) ? null : ms;
}

/** Wert für `<input type="date">`. */
export function toDateInput(ms: number): string {
  return toLocalInput(ms).slice(0, 10);
}

/** Fälligkeit „Ende des gewählten Tages“; `null` bei leerer oder ungültiger Eingabe. */
export function dueFromDateInput(value: string): number | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  const ms = new Date(`${value}T23:59`).getTime();
  return Number.isNaN(ms) ? null : ms;
}

/** Neue Kennung für lokal angelegte Termine und Aufgaben. */
export function newId(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
}
