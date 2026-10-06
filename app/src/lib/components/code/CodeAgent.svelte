<script lang="ts">
  // Der Code-Agent im Code-Bereich: Du beschreibst eine Aufgabe für den ganzen Ordner, IAP
  // liest und durchsucht ihn selbst und schlägt Änderungen vor. Es schreibt nie selbst: Jeder
  // Vorschlag erscheint als Unterschied im Editor und wird erst nach deiner Bestätigung gespeichert.
  import { onMount, tick } from "svelte";
  import { groupAgentLines, splitPath, stepLabel, stepTarget } from "../../agentSteps";
  import { t, tk } from "../../i18n/index.svelte";
  import { codeAgentCancel, listInstalledSkills } from "../../ipc";
  import { codeSkillItems, parseSkillRequest } from "../../skillRequest";
  import { agent, answerCommand, discardAllChanges, discardChange, ensureAgentListener, resetAgent, sendToAgent } from "../../stores/codeAgent.svelte";
  import type { InstalledSkillView, SettingsSnapshot } from "../../types";
  import ErrorNotice from "../ErrorNotice.svelte";
  import CodePromptBar from "./CodePromptBar.svelte";
  import CommandConfirm from "./CommandConfirm.svelte";

  interface Props {
    /** Die gerade geöffnete Datei; IAP weiß dann, worauf sich „hier“ bezieht. */
    activeFile: string | null;
    /** Lädt einen Vorschlag zur Prüfung in den Editor. */
    onReview: (path: string) => void | Promise<void>;
    /** Der Vorschlag, der gerade im Editor liegt. */
    reviewing?: string | null;
    /** Fehler beim Laden eines Vorschlags; erscheint hier, nicht irgendwo oben auf der Seite. */
    reviewError?: unknown;
    onDismissReviewError?: () => void;
    /** Sperrt die Modellwahl, solange „IAP fragen“ gerade arbeitet. */
    locked?: boolean;
    settings: SettingsSnapshot;
    onSettingsChanged: (snapshot: SettingsSnapshot) => void;
    onOpenSettings: () => void;
    onError?: (reason: unknown) => void;
  }
  let { activeFile, onReview, reviewing = null, reviewError = null, onDismissReviewError, locked = false, settings, onSettingsChanged, onOpenSettings, onError }: Props = $props();

  const EXAMPLES = [
    tk("Erkläre mir kurz, wie dieses Projekt aufgebaut ist."),
    tk("Suche nach TODO-Kommentaren und fasse sie zusammen."),
    tk("Prüfe die geöffnete Datei auf Fehler und schlage Korrekturen vor."),
  ];
  // Beschriftungen aus agentSteps.ts und der Hinweis des Backends; übersetzt wird beim Anzeigen mit t().
  const DYNAMIC_TEXTS = [
    tk("Ordner ansehen"), tk("Datei lesen"), tk("Änderung vorgeschlagen"), tk("Befehl"),
    tk("Das Modell hat keinen gültigen Werkzeugaufruf geliefert. Versuche es noch einmal oder formuliere die Aufgabe anders."),
  ];
  void DYNAMIC_TEXTS;

  let text = $state("");
  let log = $state<HTMLDivElement | null>(null);

  const blocks = $derived(groupAgentLines(agent.lines, agent.running));
  const hasHistory = $derived(agent.lines.length > 0 || agent.changes.length > 0);

  // Installierte Skills für das „/“-Menü; im Code-Bereich gelten nur Anleitungen.
  let skills = $state<InstalledSkillView[]>([]);
  const slashItems = $derived(codeSkillItems(skills));

  onMount(() => {
    ensureAgentListener();
    void listInstalledSkills().then((list) => { skills = list; }).catch((reason) => onError?.(reason));
  });

  // Neue Zeilen sichtbar halten.
  $effect(() => {
    void agent.lines.length;
    void agent.step;
    void tick().then(() => log?.scrollTo({ top: log.scrollHeight }));
  });

  async function send() {
    const value = text.trim();
    if (!value || agent.running) return;
    text = "";
    // „/skill Auftrag“: Der Skill wirkt nur auf diese Anfrage und reist als eigenes Feld mit.
    const request = parseSkillRequest(value, skills);
    if (request) await sendToAgent(request.rest, activeFile, request.id, value);
    else await sendToAgent(value, activeFile);
  }

  function stepCount(count: number): string {
    return count === 1 ? t("1 Schritt") : t("{n} Schritte", { n: count });
  }
</script>

<div class="v-agent v-stack" aria-label={t("IAP Agent")}>
  {#if agent.running || hasHistory}
    <div class="v-agent-head">
      <span class="v-agent-state v-help" role="status" aria-live="polite">
        {#if agent.running}<span class="v-agent-dot" aria-hidden="true"></span>{t("IAP arbeitet … Schritt {n}", { n: Math.max(agent.step, 1) })}{/if}
      </span>
      <button type="button" class="v-btn v-btn-ghost" onclick={resetAgent} disabled={agent.running}>{t("Neue Aufgabe")}</button>
    </div>
  {/if}

  <div class="v-agent-log" bind:this={log} role="log" aria-live="polite" aria-label={t("Verlauf")}>
    {#if blocks.length === 0}
      <div class="v-agent-intro">
        <p class="v-card-text">{t("Beschreibe eine Aufgabe für den ganzen Ordner. IAP liest, sucht und schlägt Änderungen vor.")}</p>
        <div class="v-agent-examples" role="group" aria-label={t("Beispiele")}>
          {#each EXAMPLES as example (example)}
            <button type="button" class="v-chip v-agent-example" disabled={agent.running} onclick={() => (text = t(example))}>{t(example)}</button>
          {/each}
        </div>
      </div>
    {:else}
      {#each blocks as block, index (index)}
        {#if block.kind === "user"}
          <p class="v-agent-user">{block.text}</p>
        {:else if block.kind === "steps"}
          <details class="v-agent-steps" open={block.open}>
            <summary>
              <span>{stepCount(block.steps.length)}</span>
              {#if block.open}<span class="v-agent-dot" aria-hidden="true"></span>{/if}
            </summary>
            <ol>
              {#each block.steps as step, stepIndex (stepIndex)}
                <li>
                  <span class="v-agent-step-label">{t(stepLabel(step.tool ?? ""))}</span>
                  <span class="v-agent-step-target v-num" title={step.args ?? step.text}>{stepTarget(step.tool ?? "", step.args ?? "")}</span>
                  {#if step.detail}<span class="v-agent-step-detail v-num">{step.detail}</span>{/if}
                </li>
              {/each}
            </ol>
          </details>
        {:else if block.kind === "error"}
          <p class="v-notice warn" role="alert">{t(block.text)}</p>
        {:else}
          <p class="v-agent-answer">{block.text}</p>
        {/if}
      {/each}
    {/if}
  </div>

  {#if agent.changes.length > 0}
    <div class="v-stack v-agent-changes">
      <div class="v-agent-changes-head">
        <h3 class="v-agent-heading">{t("Vorschläge ({n})", { n: agent.changes.length })}</h3>
        {#if agent.changes.length > 1}
          <button type="button" class="v-btn v-btn-ghost" onclick={() => void discardAllChanges()} disabled={agent.running}>{t("Alle verwerfen")}</button>
        {/if}
      </div>
      <ul class="v-list">
        {#each agent.changes as change (change.path)}
          {@const parts = splitPath(change.path)}
          <li class:v-agent-active={reviewing === change.path}>
            <div class="v-list-main">
              <span class="v-agent-path v-num" title={change.path}><span class="v-agent-dir">{parts.dir}</span><strong>{parts.name}</strong></span>
              <span>
                {#if reviewing === change.path}<span class="v-chip">{t("Im Editor")}</span>{/if}
                {#if change.is_new}<span class="v-chip">{t("Neue Datei")}</span>{/if}
                <span class="v-num">+{change.added} −{change.removed}</span>
              </span>
            </div>
            <button type="button" class="v-btn v-btn-primary" title={t("„Prüfen“ lädt den Vorschlag in den Editor. Dort siehst du jede Änderung und bestätigst sie.")} onclick={() => onReview(change.path)}>{t("Prüfen")}</button>
            <button type="button" class="v-btn v-btn-ghost" onclick={() => discardChange(change.path)}>{t("Verwerfen")}</button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  {#if reviewError}<ErrorNotice error={reviewError} onDismiss={onDismissReviewError} />{/if}

  <CodePromptBar inputId="iap-code-agent-prompt" {slashItems} onSlash={(item) => (text = item.name + " ")} bind:value={text} busy={agent.running} {locked} placeholder={t("Aufgabe für den Ordner, z. B. Prüfung auf leere Werte ergänzen.")}
    {settings} {onSettingsChanged} {onOpenSettings} onSend={send} onStop={() => void codeAgentCancel()} {onError} />
</div>

{#if agent.pendingCommand}
  <CommandConfirm prompt={agent.pendingCommand} onAnswer={answerCommand} />
{/if}

<style>
  .v-agent { min-height: 0; gap: var(--v-space-2); }
  .v-agent :global(.v-btn) { min-height: 2rem; padding: 0.25rem 0.75rem; font-size: var(--v-text-sm); }
  .v-agent-head { display: flex; align-items: center; justify-content: space-between; gap: var(--v-space-2); min-height: 2rem; }
  .v-agent-state { display: inline-flex; align-items: center; gap: var(--v-space-2); }
  .v-agent-dot { width: 7px; height: 7px; flex: 0 0 auto; border-radius: 9999px; background: var(--v-accent-blue); animation: v-agent-pulse 1.4s ease-in-out infinite; }
  @keyframes v-agent-pulse { 50% { opacity: .35; } }
  @media (prefers-reduced-motion: reduce) { .v-agent-dot { animation: none; } }

  .v-agent-log { display: grid; align-content: start; gap: var(--v-space-2); min-height: 9rem; overflow-y: auto; padding-right: var(--v-space-1); }
  .v-agent-log p { margin: 0; }
  .v-agent-intro { display: grid; gap: var(--v-space-3); }
  .v-agent-examples { display: grid; gap: var(--v-space-1); justify-items: start; }
  .v-agent-example { height: auto; max-width: 100%; padding: var(--v-space-1) var(--v-space-3); text-align: left; white-space: normal; }

  .v-agent-user { align-self: end; justify-self: end; max-width: 92%; padding: var(--v-space-2) var(--v-space-3); border-radius: var(--v-radius-control); background: var(--v-surface-muted, rgb(var(--v-shade) / .08)); color: var(--v-text-primary); font-size: var(--v-text-sm); white-space: pre-wrap; overflow-wrap: anywhere; }
  .v-agent-answer { color: var(--v-text-primary); font-size: var(--v-text-sm); line-height: 1.55; white-space: pre-wrap; overflow-wrap: anywhere; }

  .v-agent-steps { border: 1px solid var(--v-line); border-radius: var(--v-radius-control); background: rgb(var(--v-tint) / .03); font-size: var(--v-text-xs); color: var(--v-text-muted); }
  .v-agent-steps summary { display: flex; align-items: center; gap: var(--v-space-2); padding: var(--v-space-1) var(--v-space-3); min-height: 2rem; cursor: pointer; }
  .v-agent-steps ol { display: grid; gap: var(--v-space-2); margin: 0; padding: 0 var(--v-space-3) var(--v-space-2); list-style: none; }
  .v-agent-steps li { display: grid; gap: 1px; }
  .v-agent-step-label { color: var(--v-text-secondary); font-weight: 550; }
  .v-agent-step-target { overflow-wrap: anywhere; color: var(--v-text-primary); }
  .v-agent-step-detail { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; opacity: .8; }

  .v-agent-changes-head { display: flex; align-items: center; justify-content: space-between; gap: var(--v-space-2); }
  .v-agent-heading { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-sm); font-weight: 600; }
  .v-agent-path { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .v-agent-dir { color: var(--v-text-muted); font-weight: 400; }
  .v-agent-active { box-shadow: inset 2px 0 0 var(--v-accent-blue); }
</style>
