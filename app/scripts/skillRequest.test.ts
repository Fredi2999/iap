// Läuft ohne Zusatzpaket: node --experimental-strip-types --test scripts/skillRequest.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { codeSkillItems, parseSkillRequest } from "../src/lib/skillRequest.ts";

const skills = [
  { id: "code-review", name: "Code Review", kind: "instructions" as const, description: "Prüft Änderungen." },
  { id: "brainstorming", name: "Brainstorming", kind: "instructions" as const, description: "" },
  { id: "rechner", name: "Rechner", kind: "wasm" as const, description: "Programm" },
];

test("„/skill Auftrag“ liefert Skill und Auftrag ohne den Skill-Namen", () => {
  assert.deepEqual(parseSkillRequest("/code-review prüfe src/main.rs", skills), { id: "code-review", rest: "prüfe src/main.rs" });
});

test("der Auftrag darf mehrere Zeilen haben", () => {
  assert.deepEqual(parseSkillRequest("/brainstorming Idee:\n- eins\n- zwei", skills), { id: "brainstorming", rest: "Idee:\n- eins\n- zwei" });
});

test("führende und nachfolgende Leerzeichen stören nicht", () => {
  assert.deepEqual(parseSkillRequest("  /code-review   Auftrag  ", skills), { id: "code-review", rest: "Auftrag" });
});

test("ein unbekannter Skill oder ein Pfad wird nicht als Skill gelesen", () => {
  assert.equal(parseSkillRequest("/gibt-es-nicht tu etwas", skills), null);
  assert.equal(parseSkillRequest("/src/main.rs ändern", skills), null);
  assert.equal(parseSkillRequest("kein Slash am Anfang", skills), null);
});

test("ohne Auftrag nach dem Namen gibt es keine Anfrage", () => {
  assert.equal(parseSkillRequest("/code-review", skills), null);
  assert.equal(parseSkillRequest("/code-review   ", skills), null);
});

test("Programm-Skills (WASM) gelten im Code-Agenten nicht, dort gibt es nur Anleitungen", () => {
  assert.equal(parseSkillRequest("/rechner 1+1", skills), null);
});

test("das Slash-Menü des Code-Agenten zeigt nur Anleitungs-Skills, mit Name und Beschreibung", () => {
  const items = codeSkillItems(skills);
  assert.deepEqual(items.map((item) => item.name), ["/code-review", "/brainstorming"]);
  assert.ok(items.every((item) => item.group === "skill" && item.id.startsWith("skill:")));
  assert.match(items[0].description, /Code Review/);
  assert.match(items[0].description, /Prüft Änderungen\./);
  assert.doesNotMatch(items[1].description, / · $/);
});
