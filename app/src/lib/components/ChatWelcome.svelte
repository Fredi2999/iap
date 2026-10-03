<script lang="ts">
  import { onMount } from "svelte";
  import { t, tk, locale } from "../i18n/index.svelte";
  import { listActiveFacts, loadUiState } from "../ipc";
  import type { Conversation, Fact, SchedulerTask } from "../types";

  interface Props {
    onPick: (prompt: string) => void;
    /** Legt einen fertigen Auftrag in die Eingabe und schickt ihn sofort ab. */
    onSend: (prompt: string) => void;
    onOpenConversation: (id: string) => void;
    /** Zuletzt benutzte Unterhaltung (ohne die aktuelle, leere). */
    lastConversation: Conversation | null;
  }
  let { onPick, onSend, onOpenConversation, lastConversation }: Props = $props();

  // Startvorschläge zeigen, was IAP offline kann; ein Klick legt den Text in die Eingabe.
  const SUGGESTIONS = [
    { title: tk("Text zusammenfassen"), prompt: tk("Fasse den folgenden Text in fünf Stichpunkten zusammen:\n\n"), icon: "M4 6h16M4 12h12M4 18h8" },
    { title: tk("E-Mail formulieren"), prompt: tk("Formuliere eine freundliche, kurze E-Mail zu folgendem Anliegen:\n\n"), icon: "M4 6h16v12H4zM4 7l8 6 8-6" },
    { title: tk("Etwas erklären lassen"), prompt: tk("Erkläre mir einfach und mit einem Beispiel: "), icon: "M12 3a6 6 0 0 0-3.5 10.9V17h7v-3.1A6 6 0 0 0 12 3ZM9.5 20h5" },
    { title: tk("Wochenplan entwerfen"), prompt: tk("Hilf mir, diese Aufgaben sinnvoll auf die Woche zu verteilen:\n\n"), icon: "M7 3v3M17 3v3M4 8h16M5 6h14v14H5z" },
  ];

  // Tagesstart: dieselben Daten, die Kalender und Gedächtnis im Tresor ablegen.
  let tasks = $state<SchedulerTask[]>([]);
  let facts = $state<Fact[]>([]);
  const now = new Date();
  const greeting = now.getHours() < 11 ? tk("Guten Morgen") : now.getHours() < 18 ? tk("Guten Tag") : tk("Guten Abend");
  const hasDay = $derived(tasks.length > 0 || facts.length > 0 || lastConversation !== null);

  onMount(async () => {
    try {
      const saved = await loadUiState("ui.scheduler.tasks");
      const all = saved ? (JSON.parse(saved) as SchedulerTask[]) : [];
      tasks = all
        .filter((task) => task.status === "open" || task.status === "in_progress")
        .sort((a, b) => b.priority - a.priority || (a.due_unix_ms ?? Infinity) - (b.due_unix_ms ?? Infinity))
        .slice(0, 3);
    } catch (reason) { console.error(reason); }
    try {
      facts = (await listActiveFacts()).sort((a, b) => b.valid_from_unix_ms - a.valid_from_unix_ms).slice(0, 3);
    } catch (reason) { console.error(reason); }
  });

  function summarizeDay() {
    const lines = [t("Erstelle mir eine kurze Tagesübersicht mit drei Prioritäten für heute.")];
    if (tasks.length) lines.push(`${t("Offene Aufgaben")}:\n${tasks.map((task) => `- ${task.title} (${t("{n} Minuten", { n: task.duration_minutes })})`).join("\n")}`);
    if (facts.length) lines.push(`${t("Was du über mich weißt")}:\n${facts.map((fact) => `- ${fact.text}`).join("\n")}`);
    onSend(lines.join("\n\n"));
  }
</script>

<div class="iap-welcome">
  <h2>{hasDay ? t(greeting) : t("Womit kann ich helfen?")}</h2>
  <p>{hasDay ? now.toLocaleDateString(locale(), { weekday: "long", day: "numeric", month: "long" }) : t("Alles läuft lokal auf diesem Stick. Nichts verlässt den Rechner.")}</p>

  {#if hasDay}
    <div class="iap-day">
      {#if tasks.length}
        <section aria-labelledby="day-tasks">
          <h3 id="day-tasks">{t("Offene Aufgaben")}</h3>
          <ul>{#each tasks as task (task.id)}<li><button type="button" onclick={() => onPick(t("Hilf mir bei dieser Aufgabe: {title}", { title: task.title }) + "\n\n")}>{task.title}</button></li>{/each}</ul>
        </section>
      {/if}
      {#if facts.length}
        <section aria-labelledby="day-facts">
          <h3 id="day-facts">{t("Zuletzt gemerkt")}</h3>
          <ul>{#each facts as fact (fact.id)}<li><span>{fact.text}</span></li>{/each}</ul>
        </section>
      {/if}
      {#if lastConversation}
        <section aria-labelledby="day-continue">
          <h3 id="day-continue">{t("Weitermachen")}</h3>
          <ul><li><button type="button" onclick={() => lastConversation && onOpenConversation(lastConversation.id)}>{lastConversation.title}</button></li></ul>
        </section>
      {/if}
    </div>
    {#if tasks.length || facts.length}
      <button type="button" class="v-btn v-btn-ghost iap-day-summary" onclick={summarizeDay}>{t("Tagesübersicht erstellen lassen")}</button>
    {/if}
  {/if}

  <div class="iap-welcome-grid">
    {#each SUGGESTIONS as suggestion (suggestion.title)}
      <button type="button" onclick={() => onPick(t(suggestion.prompt))}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={suggestion.icon}/></svg>
        <span>{t(suggestion.title)}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .iap-welcome { display: flex; flex-direction: column; align-items: center; justify-content: center; min-height: 100%; padding: var(--v-space-5) 0; text-align: center; }
  h2 { margin: 0; font-size: var(--v-text-2xl); font-weight: 600; letter-spacing: -.02em; color: var(--v-text-primary); }
  p { margin: var(--v-space-2) 0 var(--v-space-5); color: var(--v-text-muted); font-size: var(--v-text-md); }
  .iap-welcome-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 15rem)); gap: var(--v-space-2); }
  .iap-welcome-grid button { display: flex; align-items: center; gap: var(--v-space-3); min-height: 3.25rem; padding: var(--v-space-3) var(--v-space-4); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); background: rgb(var(--v-tint) / .04); color: var(--v-text-secondary); font-size: var(--v-text-sm); text-align: left; transition: background-color 150ms ease, border-color 150ms ease, color 150ms ease, transform 120ms var(--v-ease-out-strong); }
  .iap-welcome-grid button svg { flex: 0 0 auto; color: var(--v-accent-blue); }
  @media (hover: hover) and (pointer: fine) { .iap-welcome-grid button:hover { border-color: var(--v-line-strong); background: rgb(var(--v-tint) / .08); color: var(--v-text-primary); } }
  .iap-welcome-grid button:active { transform: scale(.98); }
  button:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: 2px; }

  /* Tagesstart: drei schlichte Spalten, keine Karten in Karten. */
  .iap-day { display: grid; grid-template-columns: repeat(auto-fit, minmax(12rem, 1fr)); gap: var(--v-space-5); width: min(46rem, 100%); margin-bottom: var(--v-space-4); text-align: left; }
  .iap-day h3 { margin: 0 0 var(--v-space-2); color: var(--v-text-muted); font-size: var(--v-text-xs); font-weight: 600; }
  .iap-day ul { margin: 0; padding: 0; list-style: none; border-top: 1px solid var(--v-line); }
  .iap-day li { border-bottom: 1px solid var(--v-line); }
  .iap-day li > span, .iap-day li > button { display: block; width: 100%; padding: var(--v-space-2) 0; overflow: hidden; border: 0; background: transparent; color: var(--v-text-secondary); font-size: var(--v-text-sm); text-align: left; text-overflow: ellipsis; white-space: nowrap; }
  .iap-day li > button { cursor: pointer; }
  @media (hover: hover) and (pointer: fine) { .iap-day li > button:hover { color: var(--v-text-primary); } }
  .iap-day-summary { margin-bottom: var(--v-space-5); }
  @media (max-width: 560px) { .iap-welcome-grid { grid-template-columns: 1fr; width: 100%; } }
</style>
