// Karten bekommen einen Leuchtfleck, der dem Zeiger folgt. Die Aktion setzt nur die Position als
// CSS-Variablen; das Aussehen steht in redesign.css (`.pointer-glow`).
const SURFACE_CLASSES = ["v-bezel-outer", "v-card-surface"];

function isSurface(node: HTMLElement): boolean {
  return SURFACE_CLASSES.some((name) => node.classList.contains(name)) || Array.from(node.classList).some((name) => name.startsWith("bg-[var(--v-bg-panel)]"));
}

function place(surface: HTMLElement, x: string, y: string) {
  surface.classList.add("pointer-glow");
  surface.style.setProperty("--glow-x", x);
  surface.style.setProperty("--glow-y", y);
}

/** Für eine einzelne Karte. */
export function pointerGlow(node: HTMLElement) {
  const move = (event: PointerEvent) => {
    const box = node.getBoundingClientRect();
    place(node, `${event.clientX - box.left}px`, `${event.clientY - box.top}px`);
  };
  node.classList.add("pointer-glow");
  node.addEventListener("pointermove", move, { passive: true });
  return { destroy() { node.removeEventListener("pointermove", move); node.classList.remove("pointer-glow"); } };
}

/** Für alle Karten unterhalb von `root`, auch nachträglich gerenderte, mit einem einzigen Listener. */
export function pointerGlowSurfaces(root: HTMLElement) {
  const surfaceOf = (target: EventTarget | null): HTMLElement | null => {
    let node = target instanceof HTMLElement ? target : null;
    while (node && node !== root) {
      if (isSurface(node)) return node;
      node = node.parentElement;
    }
    return null;
  };
  const move = (event: PointerEvent) => {
    const surface = surfaceOf(event.target);
    if (!surface) return;
    const box = surface.getBoundingClientRect();
    place(surface, `${event.clientX - box.left}px`, `${event.clientY - box.top}px`);
  };
  const focus = (event: FocusEvent) => {
    const surface = surfaceOf(event.target);
    if (surface) place(surface, "50%", "50%");
  };
  root.addEventListener("pointermove", move, { passive: true });
  root.addEventListener("focusin", focus);
  return { destroy() { root.removeEventListener("pointermove", move); root.removeEventListener("focusin", focus); } };
}
