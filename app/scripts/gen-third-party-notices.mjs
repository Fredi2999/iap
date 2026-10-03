// Erzeugt die Paketlisten in THIRD-PARTY-NOTICES.md aus den Lockfiles.
// Aufruf: node app/scripts/gen-third-party-notices.mjs [--check]
//   ohne Option: schreibt die Abschnitte zwischen den BEGIN/END-Markierungen neu
//   --check:     ändert nichts und endet mit Fehler, wenn die Datei veraltet ist (für die CI)
//
// Warum: Die Liste von Hand zu pflegen schlägt fehl, sobald sich ein Lockfile ändert. Erfasst werden
// Laufzeitabhängigkeiten für Windows (Cargo: Workspace und Tauri-App; npm: ohne devDependencies).
// Die Lizenz ist das Feld `license` des Pakets, nicht der geprüfte Lizenztext.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const file = join(repo, "THIRD-PARTY-NOTICES.md");

function cargoPackages(manifest) {
  const json = execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--filter-platform", "x86_64-pc-windows-msvc", "--manifest-path", manifest], {
    cwd: repo, maxBuffer: 1 << 28,
  }).toString();
  const meta = JSON.parse(json);
  const packages = new Map(meta.packages.map((entry) => [entry.id, entry]));
  const nodes = new Map(meta.resolve.nodes.map((node) => [node.id, node]));
  const members = new Set(meta.workspace_members);
  const seen = new Set();
  const stack = [...members];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of nodes.get(id).deps) if (dep.dep_kinds.some((kind) => kind.kind === null)) stack.push(dep.pkg);
  }
  return [...seen].filter((id) => !members.has(id)).map((id) => packages.get(id));
}

const rust = new Map();
for (const manifest of ["Cargo.toml", "app/src-tauri/Cargo.toml"]) {
  for (const entry of cargoPackages(manifest)) rust.set(`${entry.name}@${entry.version}`, [entry.name, entry.version, entry.license ?? "?"]);
}
const rustRows = [...rust.values()].sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));

const lock = JSON.parse(readFileSync(join(repo, "app", "package-lock.json"), "utf8"));
const npmRows = Object.entries(lock.packages)
  .filter(([path, info]) => path && !info.dev)
  .map(([path, info]) => {
    const name = path.split("node_modules/").pop();
    let license = info.license;
    const manifest = join(repo, "app", path, "package.json");
    if (!license && existsSync(manifest)) license = JSON.parse(readFileSync(manifest, "utf8")).license;
    return [name, info.version ?? "", String(license ?? "?")];
  })
  .sort((a, b) => a[0].localeCompare(b[0]));

const table = (rows) => ["| Paket | Version | Lizenz |", "|---|---|---|", ...rows.map(([name, version, license]) => `| ${name} | ${version} | ${license} |`)].join("\n");
const sections = {
  rust: `${rustRows.length} Pakete (Workspace und Tauri-App). Lizenz laut \`Cargo.toml\` des Pakets; wo zwei Lizenzen mit \`OR\` stehen, darf man wählen.\n\n${table(rustRows)}`,
  npm: `${npmRows.length} Pakete aus \`app/package-lock.json\` ohne Entwicklungsabhängigkeiten.\n\n${table(npmRows)}`,
};

// Unter Windows checkt Git Textdateien oft als CRLF aus; die Markierungen und der Vergleich
// arbeiten deshalb mit LF.
const normalize = (value) => value.replace(/\r\n/g, "\n");
let text = normalize(readFileSync(file, "utf8"));
for (const [name, body] of Object.entries(sections)) {
  const pattern = new RegExp(`(<!-- BEGIN:${name} -->\\n)[\\s\\S]*?(\\n<!-- END:${name} -->)`);
  if (!pattern.test(text)) throw new Error(`Markierung ${name} fehlt in THIRD-PARTY-NOTICES.md`);
  text = text.replace(pattern, (_, begin, end) => `${begin}${body}${end}`);
}
if (process.argv.includes("--check")) {
  if (normalize(readFileSync(file, "utf8")) !== text) {
    console.error("THIRD-PARTY-NOTICES.md ist veraltet: node app/scripts/gen-third-party-notices.mjs");
    process.exit(1);
  }
  console.log("THIRD-PARTY-NOTICES.md ist aktuell.");
} else {
  writeFileSync(file, text);
  console.log(`Geschrieben: ${rustRows.length} Rust-Pakete, ${npmRows.length} npm-Pakete.`);
}
