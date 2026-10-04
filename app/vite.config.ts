import { existsSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

const srcDir = resolve(dirname(fileURLToPath(import.meta.url)), "src");
const privateDir = join(srcDir, "private");

/**
 * Legt einen optionalen, nicht eingecheckten Ordner `src/private/` über den Quelltext: liegt dort
 * eine Datei mit demselben relativen Pfad, wird sie statt der öffentlichen benutzt. So lassen sich
 * Oberflächen-Effekte mit eigener Lizenz privat ersetzen, ohne sie ins öffentliche Repo zu legen.
 * Ohne den Ordner ändert sich nichts.
 */
function privateOverlay(): Plugin {
  return {
    name: "iap-private-overlay",
    enforce: "pre",
    resolveId(source, importer) {
      if (!importer || !source.startsWith(".") || !existsSync(privateDir)) return null;
      const from = importer.split("?")[0];
      if (from.startsWith(privateDir)) return null;
      const target = resolve(dirname(from), source);
      if (!target.startsWith(srcDir)) return null;
      const candidate = join(privateDir, relative(srcDir, target));
      return existsSync(candidate) ? candidate : null;
    },
  };
}

// `--mode mock` schaltet die Browser-Vorschau mit Mock-Backend ein (src/lib/mock). Die Variable wird
// hier gesetzt, weil .env-Dateien ignoriert sind und `VAR=1 befehl` unter Windows nicht überall geht.
export default defineConfig(({ mode }) => {
  if (mode === "mock") process.env.VITE_MOCK = "1";
  return {
    plugins: [privateOverlay(), tailwindcss(), svelte()],
    resolve: { alias: { "@app": srcDir } },
    clearScreen: false,
    server: {
      port: 1420,
      strictPort: true,
      watch: {
        ignored: ["**/src-tauri/**"],
      },
    },
  };
});
