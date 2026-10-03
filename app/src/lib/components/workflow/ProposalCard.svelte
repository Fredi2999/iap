<script lang="ts">
  // Ein Vorschlag aus „Ablegen“. Der Lauf hat nichts geschrieben; erst „Übernehmen“ legt es ab,
  // und bei Dateien siehst du vorher den Unterschied (das Modell schreibt nie selbst).
  import { onMount, untrack } from "svelte";
  import { t, tk } from "../../i18n/index.svelte";
  import { diffCodeFile, loadUiState, saveUiState, upsertFact, writeCodeFile } from "../../ipc";
  import { friendlyError } from "../../errors";
  import { notify } from "../../notifications";
  import type { SchedulerTask, UnifiedDiff, WorkflowProposal } from "../../types";

  interface Props {
    proposal: WorkflowProposal;
    onDone: () => void;
  }
  let { proposal, onDone }: Props = $props();

  // Der Nutzer darf den Text vor dem Ablegen anpassen.
  let text = $state(untrack(() => proposal.text));
  let busy = $state(false);
  let error = $state<string | null>(null);
  let diff = $state<UnifiedDiff | null>(null);

  const TITLE: Record<string, string> = { memory: tk("Im Gedächtnis merken"), task: tk("Als Aufgabe anlegen"), file: tk("In Datei schreiben") };
  const path = $derived(proposal.target.kind === "file" ? proposal.target.relative_path : "");

  onMount(async () => {
    if (proposal.target.kind !== "file") return;
    try { diff = await diffCodeFile(proposal.target.relative_path, proposal.text); }
    catch (reason) { error = friendlyError(reason).message; }
  });

  async function refreshDiff() {
    if (proposal.target.kind !== "file") return;
    try { diff = await diffCodeFile(proposal.target.relative_path, text); error = null; }
    catch (reason) { error = friendlyError(reason).message; }
  }

  async function apply() {
    busy = true;
    error = null;
    try {
      const target = proposal.target;
      if (target.kind === "memory") {
        await upsertFact({ id: null, text: text.trim(), category: "other", user_verified: true });
        notify("Im Gedächtnis gemerkt", "", "success");
      } else if (target.kind === "task") {
        const raw = await loadUiState("ui.scheduler.tasks");
        const tasks: SchedulerTask[] = raw ? JSON.parse(raw) : [];
        const title = text.trim().split("\n")[0].slice(0, 120);
        tasks.push({
          id: `t-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`,
          title, duration_minutes: 30, energy: "medium", priority: 3, depends_on: [], status: "open", project: null, due_unix_ms: null,
        });
        await saveUiState("ui.scheduler.tasks", JSON.stringify(tasks));
        notify("Aufgabe angelegt", "Du findest sie im Kalender.", "success");
      } else {
        await writeCodeFile(target.relative_path, text);
        notify("Datei gespeichert", target.relative_path, "success");
      }
      onDone();
    } catch (reason) {
      error = friendlyError(reason).message;
      busy = false;
    }
  }
</script>

<div class="v-prop">
  <strong>{t(TITLE[proposal.target.kind])}{#if path}: <span class="v-num">{path}</span>{/if}</strong>
  <textarea rows="4" bind:value={text} oninput={() => { if (proposal.target.kind === "file") void refreshDiff(); }} aria-label={t("Vorgeschlagener Text")}></textarea>
  {#if proposal.target.kind === "file" && diff}
    {#if diff.hunks.length === 0}
      <p class="v-help">{t("Die Datei hat schon genau diesen Inhalt.")}</p>
    {:else}
      <div class="v-prop-diff" aria-label={t("Änderung an der Datei")}>
        {#each diff.hunks as hunk, index (index)}
          <pre>{#each hunk.lines as line, li (li)}<div class={line.op}>{line.op === "insert" ? "+" : line.op === "delete" ? "-" : " "}{line.text}</div>{/each}</pre>
        {/each}
      </div>
    {/if}
  {/if}
  {#if error}<p class="v-prop-error" role="alert">{t(error)}</p>{/if}
  <div class="v-row v-row-end">
    <button type="button" class="v-btn v-btn-ghost" onclick={onDone} disabled={busy}>{t("Verwerfen")}</button>
    <button type="button" class="v-btn v-btn-primary" onclick={apply} disabled={busy || !text.trim()}>{t(busy ? "Speichere …" : "Übernehmen")}</button>
  </div>
</div>

<style>
  .v-prop { display: grid; gap: var(--v-space-2); padding: var(--v-space-3); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); background: rgb(var(--v-tint) / .04); }
  .v-prop textarea { width: 100%; resize: vertical; }
  .v-prop-diff pre { margin: 0; padding: var(--v-space-2) var(--v-space-3); overflow-x: auto; border: 1px solid var(--v-line); border-radius: var(--v-radius-field); font-family: var(--v-font-mono); font-size: var(--v-text-xs); }
  .v-prop-diff .insert { color: var(--v-success); background: color-mix(in srgb, var(--v-success) 9%, transparent); }
  .v-prop-diff .delete { color: var(--v-danger); background: var(--v-danger-soft); }
  .v-prop-diff .context { color: var(--v-text-muted); }
  .v-prop-error { margin: 0; color: var(--v-danger); font-size: var(--v-text-sm); }
</style>
