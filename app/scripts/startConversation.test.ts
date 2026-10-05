// Läuft ohne Zusatzpaket: node --experimental-strip-types --test scripts/startConversation.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { conversationToOpenAtStart } from "../src/lib/startConversation.ts";

const list = [{ id: "neu" }, { id: "alt" }];

test("ohne gemeinsame aktive Unterhaltung startet der Chat leer, auch wenn es Verlauf gibt", () => {
  assert.equal(conversationToOpenAtStart(list, null, null), null);
});

test("eine bereits aktive gemeinsame Unterhaltung (Startseite, Pet) wird übernommen", () => {
  assert.equal(conversationToOpenAtStart(list, "alt", null), "alt");
});

test("eine aktive Unterhaltung, die es nicht mehr gibt, führt in den leeren Chat", () => {
  assert.equal(conversationToOpenAtStart(list, "gelöscht", null), null);
});

test("ist schon eine Unterhaltung offen, wird nichts umgeschaltet", () => {
  assert.equal(conversationToOpenAtStart(list, "alt", "neu"), null);
});

test("ein leerer Verlauf bleibt ein leerer Chat", () => {
  assert.equal(conversationToOpenAtStart([], null, null), null);
});
