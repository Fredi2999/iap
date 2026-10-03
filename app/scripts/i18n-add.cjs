// Ergänzt Übersetzungen in en/es/fr/ja. Aufruf: node scripts/i18n-add.cjs <datei.json>
// Die JSON-Datei ist { "deutscher Schlüssel": { "en": "...", "es": "...", "fr": "...", "ja": "..." } }.
// Vorhandene Schlüssel werden überschrieben, neue vor dem schließenden `};` angehängt.
const fs = require("fs");
const path = require("path");
const entries = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
for (const lang of ["en", "es", "fr", "ja"]) {
  const file = path.join(__dirname, "..", "src", "lib", "i18n", `${lang}.ts`);
  let text = fs.readFileSync(file, "utf8");
  const nl = text.includes("\r\n") ? "\r\n" : "\n";
  const lines = text.split(nl);
  let add = "";
  for (const [key, value] of Object.entries(entries)) {
    if (typeof value[lang] !== "string") throw new Error(`${lang} fehlt für ${key}`);
    const line = `  ${JSON.stringify(key)}: ${JSON.stringify(value[lang])},`;
    const prefix = `  ${JSON.stringify(key)}: `;
    const index = lines.findIndex((l) => l.startsWith(prefix));
    if (index >= 0) lines[index] = line;
    else add += line + nl;
  }
  text = lines.join(nl);
  const end = text.lastIndexOf("};");
  text = text.slice(0, end) + add + text.slice(end);
  fs.writeFileSync(file, text);
}
console.log("ok");
