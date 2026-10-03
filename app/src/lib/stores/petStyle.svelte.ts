// Gemeinsamer Pet-Stil für Hauptfenster und Pet-Fenster. Die Wahrheit liegt im Backend
// (geprüft und im Vault gespeichert); hier steht nur eine Kopie, die über das Ereignis
// `pet-style-changed` aktuell bleibt.
import { getPetStyle, onPetStyleChanged, setPetStyle } from "../ipc";
import type { PetStyle } from "../types";

export const DEFAULT_PET_STYLE: PetStyle = {
  shape: "galet",
  color: "auto",
  size: 124,
  show_cpu: false,
  show_ram: false,
  show_storage: false,
  show_task: false,
  refresh_secs: 3,
};

export const petStyle = $state<PetStyle>({ ...DEFAULT_PET_STYLE });

let started = false;

/** Lädt den gespeicherten Stil und hört auf Änderungen aus dem anderen Fenster. */
export async function initPetStyle(): Promise<void> {
  if (started) return;
  started = true;
  try { Object.assign(petStyle, await getPetStyle()); } catch (reason) { console.error(reason); }
  try { await onPetStyleChanged((style) => Object.assign(petStyle, style)); } catch (reason) { console.error(reason); }
}

/** Zeilen unter der Figur; bestimmt die Fensterhöhe des Pets. */
export function petRows(style: PetStyle): number {
  return [style.show_cpu, style.show_ram, style.show_storage, style.show_task].filter(Boolean).length;
}

// Alle Änderungen laufen nacheinander durch dieselbe Kette. Ohne das würden zwei schnell
// hintereinander angeklickte Kästchen beide vom selben alten Stand ausgehen (jedes baut sein
// Patch auf `petStyle` zum Zeitpunkt des Klicks auf); die später zurückkommende Antwort
// überschreibt dann die Änderung des anderen Klicks wieder. Deshalb wartet jeder Aufruf erst
// die vorherige Anfrage ab und baut auf deren bestätigtem Ergebnis auf.
let chain: Promise<void> = Promise.resolve();

/** Ändert einzelne Felder. Das Backend prüft; bei einem Fehler bleibt der alte Stil. */
export function updatePetStyle(patch: Partial<PetStyle>): Promise<void> {
  const next = chain
    .catch(() => {}) // ein Fehler im vorherigen Schritt darf die Kette nicht abreißen lassen
    .then(async () => {
      const style = await setPetStyle({ ...petStyle, ...patch });
      Object.assign(petStyle, style);
    });
  chain = next;
  return next;
}
