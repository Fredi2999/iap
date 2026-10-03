import "./style.css";
import "./redesign.css";
import "./chat-redesign.css";
import "./ui.css";
import "./tokens.css";
import App from "./App.svelte";
import PetApp from "./PetApp.svelte";
import CaptureOverlay from "./CaptureOverlay.svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { mount } from "svelte";

const target = document.getElementById("app");
if (!target) {
  throw new Error("IAP root missing");
}

// Dieselbe Oberfläche in drei Fenstern: Hauptfenster, Pet und Bereichsauswahl.
function windowLabel(): string {
  try {
    return getCurrentWindow().label;
  } catch {
    return "main";
  }
}

// Nur bei VITE_MOCK=1 gebaut: Browser-Vorschau mit festen Antworten statt Backend. Die
// Bedingung ist zur Bauzeit konstant, ein normaler Build enthält das Mock-Modul nicht.
async function start() {
  if (import.meta.env.VITE_MOCK === "1") {
    const { installMockIpc } = await import("./lib/mock");
    installMockIpc();
  }
  const label = windowLabel();
  mount(label === "pet" ? PetApp : label === "capture-overlay" ? CaptureOverlay : App, { target: target! });
}
void start();
