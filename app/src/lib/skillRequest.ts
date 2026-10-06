// „/skill Auftrag“ im Code-Agenten: Erkennung und Menüeinträge. Reine Funktionen ohne Oberfläche,
// damit sie sich ohne Fenster prüfen lassen (`node --experimental-strip-types --test scripts/skillRequest.test.ts`).

/** Das Wenige, das die Erkennung von einem installierten Skill braucht. */
export interface SkillLike {
  id: string;
  name: string;
  kind: "wasm" | "instructions";
  description: string;
}

export interface SkillMenuItem {
  id: string;
  group: "skill";
  name: string;
  description: string;
}

/** Nur Anleitungen: Ihre Texte gehen in den Systemtext des Agenten. Programm-Skills (WASM) bringen
 *  eigene Werkzeuge mit, die der Code-Agent nicht kennt. */
function instructionSkills<T extends SkillLike>(skills: readonly T[]): T[] {
  return skills.filter((skill) => skill.kind === "instructions");
}

/**
 * Erkennt „/skill-id Auftrag“. Der Skill wirkt nur auf diese Anfrage; ohne Auftrag, mit unbekanntem
 * Skill oder mit einem Programm-Skill gibt es keine Anfrage, und der Text geht unverändert an den Agenten
 * (so bleibt „/src/main.rs ändern“ ein normaler Auftrag).
 */
export function parseSkillRequest(text: string, skills: readonly SkillLike[]): { id: string; rest: string } | null {
  const match = /^\/([A-Za-z0-9_-]+)\s+([\s\S]*\S)\s*$/.exec(text.trim());
  if (!match) return null;
  const skill = instructionSkills(skills).find((entry) => entry.id === match[1]);
  return skill ? { id: skill.id, rest: match[2] } : null;
}

/** Einträge für das „/“-Menü der Eingabeleiste. */
export function codeSkillItems(skills: readonly SkillLike[]): SkillMenuItem[] {
  return instructionSkills(skills).map((skill) => ({
    id: `skill:${skill.id}`,
    group: "skill" as const,
    name: `/${skill.id}`,
    description: skill.description ? `${skill.name} · ${skill.description}` : skill.name,
  }));
}
