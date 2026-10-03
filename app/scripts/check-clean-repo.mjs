// Prüft, dass das Repository die Standardversion enthält und nichts Persönliches.
//
// Aufruf (im Projektordner): node app/scripts/check-clean-repo.mjs
// Beendet sich mit Fehlercode 1 und nennt die Dateien, wenn etwas gefunden wird.
//
// Warum: Wer das Repository herunterlädt, soll das Originalprodukt bekommen, ohne Tresor, ohne
// gespeicherte Einstellungen, ohne Zugangsdaten und ohne persönliche Pfade. Die Prüfung arbeitet
// absichtlich ohne Namen aus der Entwicklung: Sie sucht Arten von Dateien und Pfadmuster.
//
// Geprüft wird, was Git verfolgt (`git ls-files`), und der gesamte Verlauf auf verbotene Dateiarten.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const git = (...args) => execFileSync("git", args, { encoding: "utf8", maxBuffer: 1 << 28 });

/** Dateien, die beim Benutzen entstehen oder Geheimnisse sind. */
const FORBIDDEN_PATHS = [
  [/(^|\/)AI\/data\//, "Benutzerdaten (Tresor, Arbeitsordner, WebView)"],
  [/(^|\/)AI\/workspace\//, "Arbeitsordner"],
  [/(^|\/)AI\/packs\//, "Zusatzpakete (gehören nicht ins Repository)"],
  [/\.(db|sqlite|sqlite3)$/i, "Datenbank/Tresor"],
  [/\.(db-wal|db-shm|db\.partial|db\.next)$/i, "Begleitdatei eines Tresors"],
  [/\.meta(\.partial)?$/i, "Tresor-Metadaten (vault.meta)"],
  [/(^|\/)ui-language$/, "gespeicherte Sprachwahl des Sticks"],
  [/\.gguf$/i, "Modellgewichte"],
  [/(^|\/)\.env(\.(?!example$).*)?$/i, "Umgebungsdatei mit Zugangsdaten"],
  [/\.(pem|key|p12|pfx|kdbx|keystore|jks)$/i, "Schlüssel oder Zertifikat"],
  [/(^|\/)id_(rsa|dsa|ecdsa|ed25519)/i, "SSH-Schlüssel"],
  [/(^|\/)\.claude\//, "persönliche Werkzeugkonfiguration (.claude)"],
  [/(^|\/)\.mcp\.json$/, "persönliche MCP-Konfiguration"],
  [/(^|\/)(node_modules|target|dist)\//, "Build-Ausgabe"],
];

/** Inhalte: absolute Benutzerpfade und Geheimnisse. Platzhalter wie `<Projektordner>` sind erlaubt. */
const FORBIDDEN_CONTENT = [
  [/[A-Za-z]:[\\/]+Users[\\/]+(?![<%{$])[A-Za-z0-9._ -]{2,}[\\/]/, "absoluter Benutzerpfad"],
  [/\/home\/(?!user\b|name\b|anna\b|runner\b)[a-z0-9._-]{2,}\//, "absoluter Benutzerpfad (Linux)"],
  [/\/Users\/(?!user\b|name\b|anna\b)[A-Za-z0-9._-]{2,}\//, "absoluter Benutzerpfad (macOS)"],
  [/-----BEGIN [A-Z ]*PRIVATE KEY-----/, "privater Schlüssel"],
  [/\b(sk-[A-Za-z0-9]{20,}|AKIA[0-9A-Z]{16}|ghp_[A-Za-z0-9]{30,}|xox[bp]-[A-Za-z0-9-]{20,})\b/, "Zugangsschlüssel"],
];

/** Dateien, die weder Text noch relevant sind. */
const SKIP_CONTENT = /(package-lock\.json|Cargo\.lock|\.(png|ico|icns|wasm|woff2?|ttf|zip|gz|pdf|svg)$)/i;

const problems = [];

const tracked = git("ls-files").split("\n").filter(Boolean);
for (const file of tracked) {
  for (const [pattern, reason] of FORBIDDEN_PATHS) {
    if (pattern.test(file)) problems.push(`${file}: ${reason}`);
  }
}

for (const file of tracked) {
  if (SKIP_CONTENT.test(file)) continue;
  let text;
  try {
    const bytes = readFileSync(file);
    if (bytes.includes(0)) continue;
    text = bytes.toString("utf8");
  } catch {
    continue;
  }
  const lines = text.split("\n");
  lines.forEach((line, index) => {
    for (const [pattern, reason] of FORBIDDEN_CONTENT) {
      const hit = line.match(pattern);
      if (hit) problems.push(`${file}:${index + 1}: ${reason} (${hit[0].slice(0, 60)})`);
    }
  });
}

// Verlauf: Hat je ein Tresor oder ein Schlüssel im Repository gelegen? Das lässt sich nicht
// nachträglich durch Löschen beheben (nur durch Umschreiben des Verlaufs oder ein neues Repository).
const history = git("log", "--all", "--name-only", "--format=").split("\n").filter(Boolean);
const historyHits = new Set();
for (const file of new Set(history)) {
  for (const [pattern, reason] of FORBIDDEN_PATHS.filter(([, r]) => !/Build-Ausgabe|Werkzeugkonfiguration|MCP|Zusatzpakete/.test(r))) {
    if (pattern.test(file)) historyHits.add(`${file}: ${reason}`);
  }
}
for (const hit of historyHits) problems.push(`VERLAUF ${hit}`);

if (problems.length === 0) {
  console.log(`Sauber: ${tracked.length} Dateien geprüft, nichts Persönliches, kein Tresor, keine Zugangsdaten (auch nicht im Verlauf).`);
} else {
  console.error(`${problems.length} Fund(e):`);
  for (const problem of problems) console.error(`  ${problem}`);
  process.exit(1);
}
