/**
 * Welche Unterhaltung der Chat nach dem Laden der Liste von selbst öffnet.
 *
 * Warum: Vectis soll bei jedem Start in einem leeren Chat beginnen, nicht in der zuletzt
 * benutzten Unterhaltung. Übernommen wird nur eine Unterhaltung, die Startseite, Pet oder
 * Bildschirm in dieser Sitzung schon als gemeinsame aktive gesetzt haben; die `id` kommt
 * nie aus dem Verlauf. `null` heißt: nichts öffnen, der leere Chat bleibt stehen. Die
 * Unterhaltung wird erst beim ersten Senden angelegt (`conversation_id = None`).
 */
export function conversationToOpenAtStart(
  conversations: ReadonlyArray<{ id: string }>,
  sharedActiveId: string | null,
  currentId: string | null,
): string | null {
  if (currentId !== null || sharedActiveId === null) return null;
  return conversations.some((entry) => entry.id === sharedActiveId) ? sharedActiveId : null;
}
