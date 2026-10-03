// Entwicklungswerkzeug: exakte Textersetzungen aus einer JSON-Datei anwenden.
// Format: [["pfad/relativ/zu/app", "alt", "neu"], ...]. Jede Ersetzung muss genau einmal passen.
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const pairs = JSON.parse(readFileSync(process.argv[2], "utf8"));
const cache = new Map();
let failed = 0;
for (const [file, from, to] of pairs) {
  const path = join(appRoot, file);
  const text = cache.get(path) ?? readFileSync(path, "utf8");
  const count = text.split(from).length - 1;
  if (count !== 1) {
    console.log(`NICHT ANGEWENDET (${count}x gefunden) ${file}: ${from.slice(0, 80)}`);
    failed++;
    cache.set(path, text);
    continue;
  }
  cache.set(path, text.replace(from, () => to));
}
for (const [path, text] of cache) writeFileSync(path, text);
console.log(`${pairs.length - failed}/${pairs.length} Ersetzungen angewendet`);
process.exit(failed ? 1 : 0);
