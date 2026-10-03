// Läuft ohne Zusatzpaket: node --experimental-strip-types --test scripts/calendar.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import {
  addDays, addMonths, dueFromDateInput, fromLocalInput, layoutOverlaps, mergeBusy, monthGrid,
  spansOnDay, startOfWeek, toLocalInput, weekDays,
} from "../src/lib/calendar.ts";

const at = (y: number, m: number, d: number, h = 0, min = 0) => new Date(y, m - 1, d, h, min).getTime();
const span = (id: string, s: number, e: number) => ({ id, title: id, start_unix_ms: s, end_unix_ms: e });

test("die Woche beginnt am Montag", () => {
  // 2026-10-01 ist ein Donnerstag
  assert.equal(startOfWeek(at(2026, 10, 1, 15)), at(2026, 9, 28));
  assert.equal(startOfWeek(at(2026, 10, 4, 23, 59)), at(2026, 9, 28)); // Sonntag
  assert.equal(startOfWeek(at(2026, 9, 28)), at(2026, 9, 28));
  assert.equal(weekDays(at(2026, 10, 1)).length, 7);
});

test("das Monatsraster hat 42 Tage und beginnt am Montag", () => {
  const grid = monthGrid(at(2026, 10, 15));
  assert.equal(grid.length, 42);
  assert.equal(grid[0], at(2026, 9, 28));
  assert.ok(grid.includes(at(2026, 10, 1)) && grid.includes(at(2026, 10, 31)));
});

test("Monate und Tage verschieben sich über Jahresgrenzen", () => {
  assert.equal(addMonths(at(2026, 12, 20), 1), at(2027, 1, 1));
  assert.equal(addMonths(at(2026, 1, 31), -1), at(2025, 12, 1));
  assert.equal(addDays(at(2026, 10, 25, 12), 1), at(2026, 10, 26, 12)); // Sommerzeitende, gleiche Uhrzeit
});

test("Eingabefelder rechnen hin und zurück", () => {
  const ms = at(2026, 10, 1, 9, 30);
  assert.equal(toLocalInput(ms), "2026-10-01T09:30");
  assert.equal(fromLocalInput("2026-10-01T09:30"), ms);
  assert.equal(fromLocalInput(""), null);
  assert.equal(fromLocalInput("kaputt"), null);
  assert.equal(dueFromDateInput("2026-10-01"), at(2026, 10, 1, 23, 59));
  assert.equal(dueFromDateInput(""), null);
});

test("Termine über Mitternacht erscheinen an beiden Tagen", () => {
  const night = span("n", at(2026, 10, 1, 22), at(2026, 10, 2, 2));
  assert.equal(spansOnDay([night], at(2026, 10, 1)).length, 1);
  assert.equal(spansOnDay([night], at(2026, 10, 2)).length, 1);
  assert.equal(spansOnDay([night], at(2026, 10, 3)).length, 0);
  // Ende genau um Mitternacht gehört nicht mehr zum Folgetag.
  const edge = span("e", at(2026, 10, 1, 22), at(2026, 10, 2));
  assert.equal(spansOnDay([edge], at(2026, 10, 2)).length, 0);
});

test("überlappende Termine werden für den Planer zusammengefasst", () => {
  const merged = mergeBusy([
    span("b", at(2026, 10, 1, 10), at(2026, 10, 1, 12)),
    span("a", at(2026, 10, 1, 9), at(2026, 10, 1, 11)),
    span("c", at(2026, 10, 1, 13), at(2026, 10, 1, 14)),
    span("d", at(2026, 10, 1, 13, 30), at(2026, 10, 1, 13, 45)),
  ]);
  assert.equal(merged.length, 2);
  assert.equal(merged[0].start_unix_ms, at(2026, 10, 1, 9));
  assert.equal(merged[0].end_unix_ms, at(2026, 10, 1, 12));
  assert.equal(merged[1].end_unix_ms, at(2026, 10, 1, 14));
});

test("mergeBusy verändert die Eingabe nicht", () => {
  const a = span("a", 0, 10);
  const b = span("b", 5, 20);
  mergeBusy([a, b]);
  assert.equal(a.end_unix_ms, 10);
});

test("überlappende Einträge bekommen eigene Spalten", () => {
  const placed = layoutOverlaps([
    span("a", at(2026, 10, 1, 9), at(2026, 10, 1, 11)),
    span("b", at(2026, 10, 1, 10), at(2026, 10, 1, 12)),
    span("c", at(2026, 10, 1, 11), at(2026, 10, 1, 12)),
    span("d", at(2026, 10, 1, 15), at(2026, 10, 1, 16)),
  ]);
  const by = Object.fromEntries(placed.map((p) => [p.item.id, p]));
  assert.equal(by.a.column, 0);
  assert.equal(by.b.column, 1);
  assert.equal(by.c.column, 0); // a endet um 11 Uhr, die Spalte ist wieder frei
  assert.equal(by.a.columns, 2);
  assert.equal(by.d.columns, 1);
});
