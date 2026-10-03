// Sammelt alle t("…")-Schlüssel aus dem Frontend und meldet fehlende Übersetzungen.
// Aufruf: node scripts/i18n-keys.mjs            -> Liste aller Schlüssel
//         node scripts/i18n-keys.mjs --missing  -> fehlende Übersetzungen je Sprache
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "src");
const files = [];
(function walk(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      if (name !== "i18n") walk(path);
    } else if (/\.(svelte|ts)$/.test(name)) {
      files.push(path);
    }
  }
})(root);

const keys = new Set();
const add = (raw) => { const value = JSON.parse(`"${raw}"`); if (value.trim()) keys.add(value); };
const pattern = /\btk?\(\s*"((?:[^"\\]|\\.)*)"/g;
// Bausteine, die ihre Texte selbst übersetzen.
const componentProps = /<(?:PageHeader|EmptyState|InfoTooltip)\b[^>]*?>/gs;
const propValue = /\b(?:title|description|text|tip|badge)="([^"{]+)"/g;
const notifyCall = /\bnotify\(\s*"((?:[^"\\]|\\.)*)"(?:\s*,\s*"((?:[^"\\]|\\.)*)")?/g;
// SVG-Pfaddaten enthalten immer Ziffern; so fallen Wörter wie „Chat“ nicht heraus.
const pathLike = /^(?=.*\d)[MmLlHhVvCcSsQqTtAaZz0-9 .,\-]+$/;
// Erstes Argument jedes t(…)/tk(…)-Aufrufs mit Klammerzählung auslesen, damit auch
// Ternärausdrücke wie t(a ? "Ja" : "Nein") erfasst werden.
function firstArguments(text) {
  const result = [];
  for (const match of text.matchAll(/\btk?\(/g)) {
    let depth = 0;
    let i = match.index + match[0].length;
    const start = i;
    let quote = null;
    for (; i < text.length && i - start < 600; i++) {
      const c = text[i];
      if (quote) { if (c === "\\") i++; else if (c === quote) quote = null; continue; }
      if (c === '"' || c === "'" || c === "`") { quote = c; continue; }
      if (c === "(" || c === "{" || c === "[") depth++;
      else if (c === ")" || c === "}" || c === "]") { if (depth === 0) break; depth--; }
      else if (c === "," && depth === 0) break;
    }
    result.push(text.slice(start, i));
  }
  return result;
}

for (const file of files) {
  const text = readFileSync(file, "utf8");
  for (const match of text.matchAll(pattern)) add(match[1]);
  for (const argument of firstArguments(text)) {
    for (const literal of argument.matchAll(/"((?:[^"\\]|\\.)*)"/g)) {
      if (/[A-Za-zÄÖÜäöüß]{2}/.test(literal[1]) && !/^[a-z_]+$/.test(literal[1])) add(literal[1]);
    }
  }
  for (const tag of text.matchAll(componentProps)) for (const prop of tag[0].matchAll(propValue)) add(prop[1]);
  for (const match of text.matchAll(notifyCall)) { add(match[1]); if (match[2]) add(match[2]); }
  // Tabellen mit Anzeigetexten (Navigation, Beschriftungen, Fehlermeldungen).
  if (/[\\/]lib[\\/](navigation|labels|errors)\.ts$/.test(file)) {
    for (const match of text.matchAll(/(?:^|[\s{,])(?:[a-z_]+|"[^"]+")\s*:\s*"((?:[^"\\]|\\.)*)"/gm)) {
      if (!pathLike.test(match[1]) && /[a-zäöüß]/i.test(match[1]) && /\s|[A-ZÄÖÜ]/.test(match[1])) add(match[1]);
    }
  }
}

if (process.argv.includes("--missing")) {
  let missingTotal = 0;
  for (const lang of ["en", "es", "fr", "ja"]) {
    const source = readFileSync(join(root, "lib", "i18n", `${lang}.ts`), "utf8");
    const present = new Set([...source.matchAll(/^\s*"((?:[^"\\]|\\.)*)"\s*:/gm)].map((m) => JSON.parse(`"${m[1]}"`)));
    const missing = [...keys].filter((key) => !present.has(key));
    missingTotal += missing.length;
    const sample = missing.slice(0, 8).map((k) => JSON.stringify(k)).join(", ");
    console.log(`${lang}: ${keys.size - missing.length}/${keys.size} übersetzt${missing.length ? `, fehlt: ${sample}${missing.length > 8 ? " …" : ""}` : ""}`);
  }
  process.exit(missingTotal ? 1 : 0);
} else {
  console.log(JSON.stringify([...keys].sort(), null, 1));
}
