// Läuft ohne Zusatzpaket: node --experimental-strip-types --test scripts/errors.test.ts
import test from "node:test";
import assert from "node:assert/strict";
import { errorText, friendlyError, isBackendError } from "../src/lib/errors.ts";

test("a backend error with a code is recognised, an Error or string is not", () => {
  assert.equal(isBackendError({ code: "vault_locked", message: "Der Tresor ist gesperrt." }), true);
  assert.equal(isBackendError(new Error("x")), false);
  assert.equal(isBackendError("text"), false);
  assert.equal(isBackendError({ code: 5, message: "x" }), false);
  assert.equal(isBackendError(null), false);
});

test("known codes give the translated sentence and keep code and text as detail", () => {
  const result = friendlyError({ code: "wrong_passphrase", message: "Vault-Fehler: Authentifizierung" });
  assert.equal(result.message, "Das Passwort passt nicht zu diesem Tresor.");
  assert.equal(result.detail, "wrong_passphrase: Vault-Fehler: Authentifizierung");
});

test("an unknown code falls back to the backend text", () => {
  assert.equal(friendlyError({ code: "neu", message: "Etwas Neues ist passiert." }).message, "Etwas Neues ist passiert.");
});

test("plain string errors still use the old patterns", () => {
  assert.equal(friendlyError("Vault ist gesperrt").message, "Der Tresor ist gesperrt. Bitte zuerst entsperren.");
});

test("errorText never produces [object Object]", () => {
  assert.equal(errorText({ code: "vault_locked", message: "Der Tresor ist gesperrt." }), "Der Tresor ist gesperrt.");
  assert.equal(errorText(new Error("kaputt")), "kaputt");
  assert.equal(errorText("roh"), "roh");
  assert.equal(errorText(undefined), "");
});
