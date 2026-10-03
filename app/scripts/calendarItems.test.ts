// Läuft ohne Zusatzpaket: node --experimental-strip-types --test scripts/calendarItems.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import {
  calendarError, localDayStart, planningEvents, remoteToItem, upcomingEvents, writableTargets,
} from "../src/lib/calendarItems.ts";

const at = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m - 1, d, h, min).getTime();
const DAY = 86_400_000;

const remote = (over: Record<string, unknown> = {}) => ({
  id: "src-1|0|a|1", source_id: "src-1", source_label: "Apple", calendar_name: "Privat", color: "#FF2968",
  title: "Zahnarzt", start_unix_ms: at(2026, 10, 8, 9), end_unix_ms: at(2026, 10, 8, 10),
  all_day: false, start_date: null, end_date: null, location: "Linz", notes: null, recurring: false, ...over,
});

test("ein Tag aus YYYY-MM-DD beginnt um lokale Mitternacht, auch rund um die Zeitumstellung", () => {
  assert.equal(localDayStart("2026-10-08"), at(2026, 10, 8));
  assert.equal(localDayStart("2026-03-29"), at(2026, 3, 29)); // Tag der Umstellung auf Sommerzeit
  assert.equal(localDayStart("2026-10-25"), at(2026, 10, 25)); // Tag der Umstellung auf Winterzeit
  for (const bad of ["", "2026-1-8", "08.10.2026", "2026-10-08T00:00", "abc"]) assert.equal(localDayStart(bad), null, bad);
});

test("ein Termin mit Uhrzeit bleibt wie er ist und bekommt mindestens 15 Minuten", () => {
  const item = remoteToItem(remote());
  assert.equal(item.start_unix_ms, at(2026, 10, 8, 9));
  assert.equal(item.end_unix_ms, at(2026, 10, 8, 10));
  assert.equal(item.project, "Privat");
  assert.equal(item.remote?.source_label, "Apple");
  assert.equal(item.all_day, false);
  const instant = remoteToItem(remote({ end_unix_ms: at(2026, 10, 8, 9) }));
  assert.equal(instant.end_unix_ms - instant.start_unix_ms, 15 * 60_000);
});

test("ein Ganztagstermin wird über seine Daten auf lokale Tage gelegt (Ende exklusiv)", () => {
  const item = remoteToItem(remote({
    all_day: true, start_date: "2026-10-12", end_date: "2026-10-15",
    // Näherung des Backends mit falschem Versatz: wird ignoriert.
    start_unix_ms: at(2026, 10, 11, 23), end_unix_ms: at(2026, 10, 14, 23),
  }));
  assert.equal(item.start_unix_ms, at(2026, 10, 12));
  assert.equal(item.end_unix_ms, at(2026, 10, 15));
  assert.equal(item.all_day, true);
  // Ohne Daten bleibt es mindestens ein Tag lang.
  const odd = remoteToItem(remote({ all_day: true, start_date: null, end_date: null, end_unix_ms: at(2026, 10, 8, 9) }));
  assert.ok(odd.end_unix_ms - odd.start_unix_ms >= DAY);
});

test("der Planer bekommt nur Termine mit Uhrzeit und keine Anzeigefelder", () => {
  const timed = remoteToItem(remote());
  const allDay = remoteToItem(remote({ id: "x", all_day: true, start_date: "2026-10-12", end_date: "2026-10-13" }));
  const planned = planningEvents([timed, allDay]);
  assert.equal(planned.length, 1);
  assert.deepEqual(Object.keys(planned[0]).sort(), ["end_unix_ms", "external_uid", "id", "location", "project", "start_unix_ms", "title"]);
});

test("die nächsten Termine: laufende zählen, vergangene und ferne nicht, sortiert und begrenzt", () => {
  const now = at(2026, 10, 2, 12);
  const ev = (id: string, s: number, e: number) => ({ id, title: id, start_unix_ms: s, end_unix_ms: e });
  const items = [
    ev("vorbei", now - 3 * 3_600_000, now - 2 * 3_600_000),
    ev("laeuft", now - 3_600_000, now + 3_600_000),
    ev("morgen", now + DAY, now + DAY + 3_600_000),
    ev("heute-spaeter", now + 3_600_000, now + 7_200_000),
    ev("in-8-tagen", now + 8 * DAY, now + 8 * DAY + 3_600_000),
    ev("t3", now + 2 * DAY, now + 2 * DAY + 1),
    ev("t4", now + 3 * DAY, now + 3 * DAY + 1),
  ];
  assert.deepEqual(upcomingEvents(items, now, 7, 10).map((e) => e.id), ["laeuft", "heute-spaeter", "morgen", "t3", "t4"]);
  assert.deepEqual(upcomingEvents(items, now, 7, 2).map((e) => e.id), ["laeuft", "heute-spaeter"]);
  assert.deepEqual(upcomingEvents([], now, 7, 4), []);
});

test("nur beschreibbare Apple-Kalender sind Ziele für neue Termine", () => {
  const cal = (href: string, can_write: boolean) => ({ host: "p1-caldav.icloud.com", href, name: href, color: null, can_write });
  const sources = [
    { id: "a", kind: "icloud", label: "Apple", account: "x", calendars: [cal("/1/", true), cal("/2/", false)], unsupported_rules: 0 },
    { id: "g", kind: "google_ics", label: "Google", account: "", calendars: [cal("", true)], unsupported_rules: 0 },
  ] as const;
  const targets = writableTargets(sources as never);
  assert.deepEqual(targets, [{ sourceId: "a", href: "/1/", label: "Apple · /1/" }]);
});

test("Fehlertexte: Klartext des Backends bleibt, Technisches wird allgemein", () => {
  assert.equal(calendarError("Ungültige Eingabe: Anmeldung abgelehnt. Prüfe Apple-ID und das Passwort."), "Anmeldung abgelehnt. Prüfe Apple-ID und das Passwort.");
  assert.equal(calendarError(new Error("Keine Verbindung zum Kalender-Server: Zeitüberschreitung")), "Keine Verbindung zum Kalender-Server: Zeitüberschreitung");
  const generic = "Das hat nicht geklappt. Bitte versuche es noch einmal.";
  for (const bad of [undefined, "", new Error("window.__TAURI_INTERNALS__ is undefined"), "Internal: panic at x", "x".repeat(401)]) {
    assert.equal(calendarError(bad), generic, String(bad));
  }
});
