<script lang="ts">
  // Aktionen unter einer fertigen Antwort von IAP: kopieren, als Fakt merken, als Aufgabe anlegen.
  // Fakt und Aufgabe zeigen vor dem Speichern ein kurzes, änderbares Feld: Eine ganze Antwort ist
  // selten ein guter Fakt oder Aufgabentitel, und gespeichert wird nur, was der Nutzer bestätigt.
  import { t, tk } from "../i18n/index.svelte";
  import { loadUiState, saveUiState, upsertFact } from "../ipc";
  import { errorText } from "../errors";
  import { notify } from "../notifications";
  import { newId } from "../calendar";
  import type { FactCategory, SchedulerTask } from "../types";

  interface Props { content: string }
  let { content }: Props = $props();

  const TASKS_KEY = "ui.scheduler.tasks";
  const CATEGORIES: [FactCategory, string][] = [
    ["other", tk("Sonstiges")], ["preference", tk("Vorliebe")], ["project", tk("Projekt")],
    ["person", tk("Person")], ["skill", tk("Fähigkeit")], ["constraint", tk("Einschränkung")],
  ];

  let mode = $state<"fact" | "task" | null>(null);
  let draft = $state("");
  let category = $state<FactCategory>("other");
  let busy = $state(false);
  let copied = $state(false);

  /** Erster sinnvoller Absatz ohne Markdown-Zeichen, auf Länge gekürzt, als Vorschlag für das Feld. */
  function suggestion(max: number): string {
    const first = content.split(/\n\s*\n/).map((part) => part.trim()).find(Boolean) ?? "";
    const plain = first.replace(/[*_`#>]/g, "").replace(/^\s*[-\d.]+\s+/, "").replace(/\s+/g, " ").trim();
    return plain.length > max ? `${plain.slice(0, max - 1).trimEnd()}…` : plain;
  }

  function open(next: "fact" | "task") {
    mode = next;
    draft = suggestion(next === "fact" ? 280 : 80);
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(content);
      copied = true;
      setTimeout(() => { copied = false; }, 1800);
    } catch (reason) {
      notify("Kopieren nicht möglich", errorText(reason), "error");
    }
  }

  async function save() {
    const text = draft.trim();
    if (!text || busy) return;
    busy = true;
    try {
      if (mode === "fact") {
        await upsertFact({ id: null, text, category, user_verified: true });
        notify("Fakt gemerkt", text.length > 70 ? `${text.slice(0, 69)}…` : text, "info");
      } else if (mode === "task") {
        const raw = await loadUiState(TASKS_KEY);
        const tasks: SchedulerTask[] = raw ? (JSON.parse(raw) as SchedulerTask[]) : [];
        tasks.push({ id: newId("t"), title: text, duration_minutes: 30, energy: "medium", priority: 3, depends_on: [], status: "open" });
        await saveUiState(TASKS_KEY, JSON.stringify(tasks));
        notify("Aufgabe angelegt", text, "info");
      }
      mode = null;
    } catch (reason) {
      notify(mode === "fact" ? "Fakt nicht gespeichert" : "Aufgabe nicht gespeichert", errorText(reason), "error");
    } finally {
      busy = false;
    }
  }
</script>

<div class="actions" role="group" aria-label={t("Aktionen für diese Antwort")}>
  <button type="button" onclick={copy}>{copied ? t("Kopiert") : t("Kopieren")}</button>
  <button type="button" aria-expanded={mode === "fact"} onclick={() => (mode === "fact" ? (mode = null) : open("fact"))}>{t("Als Fakt merken")}</button>
  <button type="button" aria-expanded={mode === "task"} onclick={() => (mode === "task" ? (mode = null) : open("task"))}>{t("Als Aufgabe")}</button>
</div>

{#if mode}
  <form class="draft" onsubmit={(event) => { event.preventDefault(); void save(); }}>
    <label>
      <span>{mode === "fact" ? t("Was soll sich IAP merken?") : t("Titel der Aufgabe")}</span>
      <textarea rows={mode === "fact" ? 3 : 2} bind:value={draft} maxlength={mode === "fact" ? 600 : 120}></textarea>
    </label>
    {#if mode === "fact"}
      <label class="inline">
        <span>{t("Art")}</span>
        <select bind:value={category}>{#each CATEGORIES as [value, label] (value)}<option value={value}>{t(label)}</option>{/each}</select>
      </label>
    {/if}
    <div class="row">
      <button type="button" onclick={() => (mode = null)}>{t("Abbrechen")}</button>
      <button type="submit" class="primary" disabled={busy || !draft.trim()}>{t("Speichern")}</button>
    </div>
  </form>
{/if}

<style>
  .actions { display: flex; flex-wrap: wrap; gap: .4rem; margin-top: .6rem; }
  .actions button, .draft button { min-height: 1.9rem; padding: 0 .7rem; border: 1px solid var(--v-line); border-radius: var(--v-radius-control); background: transparent; color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .actions button:hover, .actions button[aria-expanded="true"] { color: var(--v-text-primary); border-color: var(--v-line-strong); }
  .draft { display: grid; gap: .5rem; margin-top: .5rem; padding: .7rem; border: 1px solid var(--v-line); border-radius: var(--v-radius-field); background: rgb(var(--v-tint) / .04); }
  .draft label { display: grid; gap: .25rem; font-size: var(--v-text-xs); color: var(--v-text-muted); }
  .draft textarea { resize: vertical; font-size: var(--v-text-sm); }
  .inline { grid-template-columns: auto 1fr; align-items: center; }
  .row { display: flex; justify-content: flex-end; gap: .4rem; }
  .draft .primary { background: var(--v-cta-bg); color: var(--v-cta-text); border-color: transparent; }
  .draft .primary:disabled { opacity: .5; }
</style>
