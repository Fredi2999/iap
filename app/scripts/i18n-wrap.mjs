// Einmaliger Umbau-Helfer: packt statische deutsche Texte im Svelte-Markup in t("…").
// Nur Markup außerhalb von <script> und <style> wird verändert.
// Aufruf: node scripts/i18n-wrap.mjs <datei.svelte> [...]
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, relative, join, sep } from "node:path";
import { fileURLToPath } from "node:url";

const i18nModule = join(dirname(fileURLToPath(import.meta.url)), "..", "src", "lib", "i18n", "index.svelte");
const hasLetters = /[A-Za-zÄÖÜäöüß]/;

function wrapMarkup(markup) {
  let changed = 0;
  // Textknoten ohne Svelte-Ausdrücke.
  markup = markup.replace(/>([^<>{}]*)</g, (whole, text) => {
    const trimmed = text.trim();
    if (!trimmed || !hasLetters.test(trimmed) || trimmed.startsWith("&")) return whole;
    const lead = text.slice(0, text.indexOf(trimmed));
    const tail = text.slice(text.indexOf(trimmed) + trimmed.length);
    changed++;
    return `>${lead}{t(${JSON.stringify(trimmed)})}${tail}<`;
  });
  // Statische Attribute mit sichtbarem oder vorgelesenem Text.
  markup = markup.replace(/\b(placeholder|aria-label|title|alt)="([^"{}]*)"/g, (whole, attr, value) => {
    if (!value.trim() || !hasLetters.test(value)) return whole;
    changed++;
    return `${attr}={t(${JSON.stringify(value)})}`;
  });
  return { markup, changed };
}

for (const file of process.argv.slice(2)) {
  const source = readFileSync(file, "utf8");
  // Blöcke, die nicht angefasst werden dürfen, heraustrennen.
  const protectedBlocks = [];
  const masked = source.replace(/<script\b[\s\S]*?<\/script>|<style\b[\s\S]*?<\/style>|<!--[\s\S]*?-->|<svg\b[\s\S]*?<\/svg>/g, (block) => {
    protectedBlocks.push(block);
    return `\u0000${protectedBlocks.length - 1}\u0000`;
  });
  const { markup, changed } = wrapMarkup(masked);
  let result = markup.replace(/\u0000(\d+)\u0000/g, (_, index) => protectedBlocks[Number(index)]);
  if (changed > 0 && !/import \{[^}]*\bt\b[^}]*\} from "[^"]*i18n\/index\.svelte"/.test(result)) {
    let rel = relative(dirname(file), i18nModule).split(sep).join("/");
    if (!rel.startsWith(".")) rel = `./${rel}`;
    result = result.replace(/<script lang="ts"( generics="[^"]*")?>\n/, (open) => `${open}  import { t } from "${rel}";\n`);
  }
  if (result !== source) writeFileSync(file, result);
  console.log(`${file.split(/[\\/]/).slice(-2).join("/")}: ${changed} Texte`);
}
