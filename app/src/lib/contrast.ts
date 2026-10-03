// Kontrast zwischen zwei Hexfarben nach WCAG 2.x, damit eine selbst gewählte Pet-Farbe
// nicht unbemerkt mit dem Hintergrund verschwindet.

function channel(value: number): number {
  const c = value / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

/** Relative Leuchtdichte von `#rrggbb`; ungültige Werte zählen als Schwarz. */
export function luminance(hex: string): number {
  const match = /^#([0-9a-fA-F]{6})$/.exec(hex);
  if (!match) return 0;
  const v = parseInt(match[1], 16);
  return 0.2126 * channel((v >> 16) & 255) + 0.7152 * channel((v >> 8) & 255) + 0.0722 * channel(v & 255);
}

/** Kontrastverhältnis von 1 (gleich) bis 21 (Schwarz auf Weiß). */
export function contrastRatio(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** Grafische Objekte brauchen nach WCAG 1.4.11 mindestens 3:1. */
export const MIN_GRAPHIC_CONTRAST = 3;
