<script lang="ts">
  // Aufgabe anlegen oder vollständig bearbeiten: Dauer, Wichtigkeit, Energie, Projekt,
  // Fälligkeit, Status und Abhängigkeiten.
  import { untrack } from "svelte";
  import { t, tk } from "../../i18n/index.svelte";
  import { dueFromDateInput, newId, toDateInput } from "../../calendar";
  import type { SchedulerEnergy, SchedulerTask, SchedulerTaskStatus } from "../../types";
  import Modal from "./Modal.svelte";

  interface Props {
    /** `null` = neue Aufgabe. */
    task: SchedulerTask | null;
    /** Alle Aufgaben, aus denen Abhängigkeiten gewählt werden können. */
    tasks: SchedulerTask[];
    onSave: (task: SchedulerTask) => void;
    onDelete: (id: string) => void;
    onClose: () => void;
  }
  let { task, tasks, onSave, onDelete, onClose }: Props = $props();

  const PRIORITY: Record<number, string> = { 1: tk("Niedrig"), 2: tk("Eher niedrig"), 3: tk("Normal"), 4: tk("Wichtig"), 5: tk("Dringend") };
  const ENERGY: Record<SchedulerEnergy, string> = { low: tk("Leicht"), medium: tk("Mittel"), high: tk("Fordernd") };
  const STATUS: Record<SchedulerTaskStatus, string> = { open: tk("Offen"), in_progress: tk("In Arbeit"), done: tk("Erledigt"), cancelled: tk("Verworfen") };

  let title = $state(untrack(() => task?.title ?? ""));
  let duration = $state(untrack(() => task?.duration_minutes ?? 30));
  let priority = $state(untrack(() => task?.priority ?? 3));
  let energy = $state<SchedulerEnergy>(untrack(() => task?.energy ?? "medium"));
  let status = $state<SchedulerTaskStatus>(untrack(() => task?.status ?? "open"));
  let project = $state(untrack(() => task?.project ?? ""));
  let due = $state(untrack(() => (task?.due_unix_ms ? toDateInput(task.due_unix_ms) : "")));
  let dependsOn = $state<string[]>(untrack(() => [...(task?.depends_on ?? [])]));

  // Eine Aufgabe darf nicht von sich selbst abhängen; erledigte und verworfene sind kein Hindernis.
  const candidates = $derived(tasks.filter((other) => other.id !== task?.id && other.status !== "done" && other.status !== "cancelled"));
  const problem = $derived(
    !title.trim() ? "Bitte einen Titel eingeben."
      : !Number.isFinite(duration) || duration < 5 || duration > 480 ? "Die Dauer muss zwischen 5 und 480 Minuten liegen."
      : null,
  );

  function toggle(id: string) {
    dependsOn = dependsOn.includes(id) ? dependsOn.filter((entry) => entry !== id) : [...dependsOn, id];
  }

  function save(submit: SubmitEvent) {
    submit.preventDefault();
    if (problem) return;
    onSave({
      id: task?.id ?? newId("t"),
      title: title.trim(),
      duration_minutes: Math.round(duration),
      priority,
      energy,
      status,
      project: project.trim() || null,
      due_unix_ms: dueFromDateInput(due),
      depends_on: dependsOn.filter((id) => candidates.some((c) => c.id === id)),
    });
  }
</script>

<Modal title={t(task ? "Aufgabe bearbeiten" : "Neue Aufgabe")} {onClose}>
  <form class="v-cal-form" onsubmit={save}>
    <label class="v-field"><span class="v-label">{t("Titel")}</span><input bind:value={title} maxlength="120" placeholder={t("Zum Beispiel: Präsentation vorbereiten")} /></label>
    <div class="v-cal-two">
      <label class="v-field"><span class="v-label">{t("Dauer in Minuten")}</span><input type="number" min="5" max="480" step="5" bind:value={duration} /></label>
      <label class="v-field"><span class="v-label">{t("Fällig bis")}</span><input type="date" bind:value={due} /></label>
      <label class="v-field"><span class="v-label">{t("Wichtigkeit")}</span><select bind:value={priority}>{#each [1, 2, 3, 4, 5] as p}<option value={p}>{t(PRIORITY[p])}</option>{/each}</select></label>
      <label class="v-field"><span class="v-label">{t("Anstrengung")}</span><select bind:value={energy}>{#each ["low", "medium", "high"] as e}<option value={e}>{t(ENERGY[e as SchedulerEnergy])}</option>{/each}</select></label>
      <label class="v-field"><span class="v-label">{t("Status")}</span><select bind:value={status}>{#each ["open", "in_progress", "done", "cancelled"] as s}<option value={s}>{t(STATUS[s as SchedulerTaskStatus])}</option>{/each}</select></label>
      <label class="v-field"><span class="v-label">{t("Projekt")}</span><input bind:value={project} maxlength="80" /></label>
    </div>
    {#if candidates.length > 0}
      <fieldset class="v-cal-deps">
        <legend class="v-label">{t("Erst nach")}</legend>
        {#each candidates as other (other.id)}
          <label class="v-cal-dep"><input type="checkbox" checked={dependsOn.includes(other.id)} onchange={() => toggle(other.id)} /> {other.title}</label>
        {/each}
        <span data-hint class="v-help">{t("Der Planer legt diese Aufgabe erst nach den gewählten Aufgaben ein.")}</span>
      </fieldset>
    {/if}
    {#if problem && (title || task)}<p class="v-cal-problem" role="alert">{t(problem)}</p>{/if}
    <div class="v-row v-cal-actions">
      {#if task}<button type="button" class="v-btn v-btn-ghost v-cal-delete" onclick={() => onDelete(task.id)}>{t("Löschen")}</button>{/if}
      <span class="v-cal-spacer"></span>
      <button type="button" class="v-btn v-btn-ghost" onclick={onClose}>{t("Abbrechen")}</button>
      <button type="submit" class="v-btn v-btn-primary" disabled={problem !== null}>{t("Speichern")}</button>
    </div>
  </form>
</Modal>

<style>
  .v-cal-form { display: flex; flex-direction: column; gap: var(--v-space-4); }
  .v-cal-two { display: grid; grid-template-columns: 1fr 1fr; gap: var(--v-space-3); }
  @media (max-width: 480px) { .v-cal-two { grid-template-columns: 1fr; } }
  .v-cal-deps { display: flex; flex-direction: column; gap: var(--v-space-1); margin: 0; padding: var(--v-space-3); border: 1px solid var(--v-line); border-radius: var(--v-radius-card); max-height: 9rem; overflow: auto; }
  .v-cal-dep { display: flex; align-items: center; gap: var(--v-space-2); color: var(--v-text-secondary); font-size: var(--v-text-sm); }
  .v-cal-actions { gap: var(--v-space-2); }
  .v-cal-spacer { flex: 1; }
  .v-cal-delete { color: var(--v-danger); }
  .v-cal-problem { margin: 0; color: var(--v-danger); font-size: var(--v-text-sm); }
</style>
