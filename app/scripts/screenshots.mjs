// Baut die Oberfläche mit Mock-Backend und legt Screenshots der Hauptseiten ab.
// Aufruf: node scripts/screenshots.mjs [Zielordner] [--help-compact]   (Standard: ../screenshots, ignoriert von Git)
//
// Warum: Ohne Stick lässt sich keine Seite ansehen. Das Skript prüft nur die Darstellung mit
// festen Antworten (src/lib/mock), nicht das Rust-Backend. Es meldet außerdem Konsolenfehler und
// Befehle, für die der Mock keine Antwort hat, damit neue Seiten nicht unbemerkt leer rendern.
//
// Browser: nutzt Chromium aus PLAYWRIGHT_BROWSERS_PATH oder CHROMIUM_PATH; es wird nichts geladen.
import { execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const compact = args.includes("--help-compact"); // Hilfe-Modus „Kompakt“ für den Vergleich
const outDir = resolve(args.find((arg) => !arg.startsWith("--")) ?? join(root, "..", "screenshots"));
const dist = join(root, "dist-mock");

function chromiumPath() {
  if (process.env.CHROMIUM_PATH) return process.env.CHROMIUM_PATH;
  const base = process.env.PLAYWRIGHT_BROWSERS_PATH ?? "/opt/pw-browsers";
  if (existsSync(base)) {
    for (const name of readdirSync(base).filter((entry) => entry.startsWith("chromium-"))) {
      const candidate = join(base, name, "chrome-linux", "chrome");
      if (existsSync(candidate)) return candidate;
    }
  }
  return undefined; // Playwright sucht selbst (lokale Installation)
}

execFileSync("npx", ["vite", "build", "--outDir", dist, "--emptyOutDir"], {
  cwd: root, stdio: "inherit", env: { ...process.env, VITE_MOCK: "1" }, shell: process.platform === "win32",
});

const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".png": "image/png", ".svg": "image/svg+xml", ".woff2": "font/woff2" };
const server = createServer((request, response) => {
  const path = decodeURIComponent((request.url ?? "/").split("?")[0]);
  const file = join(dist, path === "/" ? "index.html" : path);
  if (!file.startsWith(dist) || !existsSync(file) || !statSync(file).isFile()) { response.writeHead(404).end(); return; }
  response.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" }).end(readFileSync(file));
});
await new Promise((done) => server.listen(0, "127.0.0.1", done));
const base = `http://127.0.0.1:${server.address().port}/`;

// Zielseiten: Anzeigename in der Seitenleiste (Deutsch, Standardsprache) und Dateiname.
const PAGES = [
  ["Startseite", "home"], ["Chat", "chat"], ["Code", "code"], ["Kalender", "calendar"], ["Einstellungen", "settings"],
  ["Werkzeuge", "tools"], ["Gedächtnis", "knowledge"], ["Dateien", "files"],
];

mkdirSync(outDir, { recursive: true });
const browser = await chromium.launch({ executablePath: chromiumPath(), args: ["--no-sandbox"] });
const problems = [];
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  page.on("console", (message) => {
    const text = message.text();
    if (message.type() === "error" || text.startsWith("[mock]")) problems.push(`${message.type()}: ${text.slice(0, 200)}`);
  });
  page.on("pageerror", (error) => problems.push(`pageerror: ${String(error).slice(0, 200)}`));
  await page.goto(base);
  await page.waitForSelector('input[type="password"]', { timeout: 15000 });
  await page.waitForTimeout(1500); // Einblend-Animationen abwarten, sonst ist der Entsperrbildschirm halb durchsichtig
  await page.screenshot({ path: join(outDir, "00-unlock.png") });
  await page.fill('input[type="password"]', "mock-passwort");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(1200);
  if (compact) await page.evaluate(() => document.documentElement.setAttribute("data-help", "compact"));
  let index = 1;
  for (const [label, name] of PAGES) {
    const target = page.getByRole("button", { name: new RegExp(`^${label}$`) }).or(page.getByRole("link", { name: new RegExp(`^${label}$`) })).first();
    if (await target.count()) {
      await target.click();
      await page.waitForTimeout(500);
    } else {
      problems.push(`Navigation: kein Eintrag „${label}“ gefunden`);
    }
    await page.screenshot({ path: join(outDir, `${String(index++).padStart(2, "0")}-${name}.png`) });
  }
} finally {
  await browser.close();
  server.close();
}
const unique = [...new Set(problems)];
console.log(`Screenshots in ${outDir}`);
if (unique.length) {
  console.log(`${unique.length} Hinweise:`);
  for (const line of unique) console.log("  " + line);
}
