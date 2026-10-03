/// <reference types="svelte" />
/// <reference types="vite/client" />

declare module "*.png" {
  const src: string;
  export default src;
}
declare module "*.svg" {
  const src: string;
  export default src;
}

interface ImportMetaEnv {
  /** "1" baut die Browser-Vorschau mit Mock-Backend (nur für Entwicklung und Screenshot-Prüfung). */
  readonly VITE_MOCK?: string;
}
