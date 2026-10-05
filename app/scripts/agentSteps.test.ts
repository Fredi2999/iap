// Läuft ohne Zusatzpaket: node --experimental-strip-types --test scripts/agentSteps.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { ENVELOPE_ERROR_TEXT, groupAgentLines, splitPath, stepLabel, stepTarget } from "../src/lib/agentSteps.ts";

test("bekannte Werkzeuge bekommen eine verständliche Beschriftung, unbekannte behalten ihren Namen", () => {
  assert.equal(stepLabel("list_dir"), "Ordner ansehen");
  assert.equal(stepLabel("read_file"), "Datei lesen");
  assert.equal(stepLabel("search"), "Suchen");
  assert.equal(stepLabel("propose_edit"), "Änderung vorgeschlagen");
  assert.equal(stepLabel("run_command"), "Befehl");
  assert.equal(stepLabel("etwas_neues"), "etwas_neues");
});

test("das Ziel eines Schritts kommt aus den Argumenten", () => {
  assert.equal(stepTarget("list_dir", '{"path":"src"}'), "src");
  assert.equal(stepTarget("list_dir", "{}"), ".");
  assert.equal(stepTarget("read_file", '{"path":"src/main.rs","start_line":1}'), "src/main.rs");
  assert.equal(stepTarget("search", '{"query":"TODO"}'), "TODO");
  assert.equal(stepTarget("run_command", '{"program":"cargo","args":["test","-q"]}'), "cargo test -q");
  assert.equal(stepTarget("propose_edit", '{"path":"a.txt","old":"x","new":"y"}'), "a.txt");
});

test("abgeschnittene Argumente (die Vorschau ist auf 240 Zeichen gekürzt) liefern trotzdem das Ziel", () => {
  const cut = '{"path":"src/lib.rs","old":"fn main() { println!(\\"hallo welt\\"); } und noch viel mehr Text bis zur Kür …';
  assert.equal(stepTarget("propose_edit", cut), "src/lib.rs");
  assert.equal(stepTarget("search", '{"query":"abgeschnitten'), "abgeschnitten");
});

test("ohne erkennbares Ziel bleibt der Rohtext, begrenzt auf eine Zeile", () => {
  assert.equal(stepTarget("etwas_neues", '{"a":1}'), '{"a":1}');
  assert.equal(stepTarget("etwas_neues", "kein json"), "kein json");
});

test("Pfade werden in Ordner und Dateiname geteilt", () => {
  assert.deepEqual(splitPath("src/lib/main.rs"), { dir: "src/lib/", name: "main.rs" });
  assert.deepEqual(splitPath("README.md"), { dir: "", name: "README.md" });
  assert.deepEqual(splitPath("a\\b\\c.txt"), { dir: "a/b/", name: "c.txt" });
});

test("aufeinanderfolgende Schritte werden zu einer Gruppe, alles andere bleibt in der Reihenfolge", () => {
  const blocks = groupAgentLines([
    { kind: "user", text: "Räume auf" },
    { kind: "step", text: "list_dir {}", tool: "list_dir", args: "{}" },
    { kind: "step", text: "read_file", tool: "read_file", args: '{"path":"a"}', detail: "inhalt" },
    { kind: "answer", text: "Fertig." },
    { kind: "user", text: "Noch etwas" },
    { kind: "error", text: "Das hat nicht geklappt." },
  ]);
  assert.deepEqual(blocks.map((block) => block.kind), ["user", "steps", "answer", "user", "error"]);
  const steps = blocks[1];
  assert.equal(steps.kind === "steps" ? steps.steps.length : -1, 2);
});

test("eine Gruppe ist nur offen, solange sie die letzte ist und der Agent arbeitet", () => {
  const lines = [
    { kind: "user" as const, text: "a" },
    { kind: "step" as const, text: "x", tool: "list_dir", args: "{}" },
  ];
  const running = groupAgentLines(lines, true);
  assert.equal(running[1].kind === "steps" && running[1].open, true);
  const finished = groupAgentLines(lines, false);
  assert.equal(finished[1].kind === "steps" && finished[1].open, false);
});

test("ein Werkzeugaufruf als Antworttext wird als Fehler gekennzeichnet, nicht als Antwort", () => {
  const blocks = groupAgentLines([{ kind: "answer", text: '{"action":"call","tool":""list_dir"","arguments":{}}' }]);
  assert.equal(blocks[0].kind, "error");
  assert.equal(blocks[0].kind === "error" ? blocks[0].text : "", ENVELOPE_ERROR_TEXT);
});

test("der Hinweis des Backends für einen kaputten Aufruf gilt ebenfalls als Fehler und wird übersetzbar angezeigt", () => {
  const blocks = groupAgentLines([{ kind: "answer", text: ENVELOPE_ERROR_TEXT }]);
  assert.deepEqual(blocks, [{ kind: "error", text: ENVELOPE_ERROR_TEXT }]);
});

test("eine normale Antwort mit geschweiften Klammern bleibt eine Antwort", () => {
  const blocks = groupAgentLines([{ kind: "answer", text: "Die Funktion nimmt {a, b} entgegen." }]);
  assert.equal(blocks[0].kind, "answer");
});
