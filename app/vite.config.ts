import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

// `--mode mock` schaltet die Browser-Vorschau mit Mock-Backend ein (src/lib/mock). Die Variable wird
// hier gesetzt, weil .env-Dateien ignoriert sind und `VAR=1 befehl` unter Windows nicht überall geht.
export default defineConfig(({ mode }) => {
  if (mode === "mock") process.env.VITE_MOCK = "1";
  return {
    plugins: [tailwindcss(), svelte()],
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
