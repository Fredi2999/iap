<script lang="ts">
  // Der Code-Agent im Code-Bereich: Du beschreibst eine Aufgabe für den ganzen Ordner, IAP
  // liest und durchsucht ihn selbst und schlägt Änderungen vor. Es schreibt nie selbst: Jeder
  // Vorschlag erscheint als Unterschied im Editor und wird erst nach deiner Bestätigung gespeichert.
  import { onMount, tick } from "svelte";
  import { t } from "../../i18n/index.svelte";
  import { codeAgentCancel } from "../../ipc";
  import { agent, answerCommand, discardChange, ensureAgentListener, resetAgent, sendToAgent } from "../../stores/codeAgent.svelte";
  import type { SettingsSnapshot } from "../../types";
  import CodePromptBar from "./CodePromptBar.svelte";
  import CommandConfirm from "./CommandConfirm.svelte";

  interface Props {
    /** Die gerade geöffnete Datei; IAP weiß dann, worauf sich „hier“ bezieht. */
    activeFile: string | null;
    /** Lädt einen Vorschlag zur Prüfung in den Editor. */
    onReview: (path: string) => void | Promise<void>;
    /** Sperrt die Modellwahl, solange „IAP fragen“ gerade arbeitet. */
    locked?: boolean;
    settings: SettingsSnapshot;
    onSettingsChanged: (snapshot: SettingsSnapshot) => void;
    onOpenSettings: () => void;
    onError?: (reason: unknown) => void;
    /** Ohne eigene Karte und Überschrift, wenn der Agent Teil einer gemeinsamen Karte ist (Code-Seite). */
    embedded?: boolean;
  }
  let { activeFile, onReview, locked = false, settings, onSettingsChanged, onOpenSettings, onError, embedded = false }: Props = $props();

  let text = $state("");
  let log = $state<HTMLDivElement | null>(null);

  onMount(ensureAgentListener);

  // Neue Zeilen sichtbar halten.
  $effect(() => {
    void agent.lines.length;
    void tick().then(() => log?.scrollTo({ top: log.scrollHeight }));
  });

  async function send() {
    const value = text.trim();
    if (!value || agent.running) return;
    text = "";
    await sendToAgent(value, activeFile);
  }

</script>

<svelte:element this={embedded ? "div" : "section"} class={embedded ? "v-stack v-agent" : "v-card v-stack v-agent"} aria-label={t("IAP Agent")}>
  <div class={embedded ? "v-agent-bar" : "v-card-title"}>
    {#if !embedded}<span>{t("IAP Agent")}</span>{/if}
    <button type="button" class="v-btn v-btn-ghost" onclick={resetAgent} disabled={agent.running || (agent.lines.length === 0 && agent.changes.length === 0)}>{t("Neue Aufgabe")}</button>
  </div>

  {#if agent.lines.length === 0}
    <p data-hint class="v-card-text">{t("Beschreibe eine Aufgabe für den ganzen Ordner. IAP liest, sucht und schlägt Änderungen vor.")}</p>
  {:else}
    <div class="v-agent-log" bind:this={log} role="log" aria-live="polite" aria-label={t("Verlauf")}>
      {#each agent.lines as line, index (index)}
        {#if line.kind === "user"}
          <p class="v-agent-user">{line.text}</p>
        {:else if line.kind === "step"}
          <p class="v-agent-step v-num" title={line.detail ?? ""}>{line.text}</p>
        {:else if line.kind === "error"}
          <p class="v-notice warn" role="alert">{line.text}</p>
        {:else}
          <p class="v-agent-answer">{line.text}</p>
        {/if}
      {/each}
    </div>
  {/if}

  {#if agent.running}
    <span class="v-help v-num" role="status">{t("IAP arbeitet … Schritt {n}", { n: Math.max(agent.step, 1) })}</span>
  {/if}

  <CodePromptBar bind:value={text} busy={agent.running} {locked} placeholder={t("Aufgabe für den Ordner, z. B. Prüfung auf leere Werte ergänzen.")}
    {settings} {onSettingsChanged} {onOpenSettings} onSend={send} onStop={() => void codeAgentCancel()} {onError} />

  {#if agent.changes.length > 0}
    <div class="v-stack">
      <h3 class="v-agent-heading">{t("Vorschläge ({n})", { n: agent.changes.length })}</h3>
      <ul class="v-list">
        {#each agent.changes as change (change.path)}
          <li>
            <div class="v-list-main">
              <strong class="v-num v-agent-path">{change.path}</strong>
              <span>
                {#if change.is_new}<span class="v-chip">{t("Neue Datei")}</span>{/if}
                <span class="v-num">+{change.added} −{change.removed}</span>
              </span>
            </div>
            <button type="button" class="v-btn v-btn-primary" onclick={() => onReview(change.path)}>{t("Prüfen")}</button>
            <button type="button" class="v-btn v-btn-ghost" onclick={() => discardChange(change.path)}>{t("Verwerfen")}</button>
          </li>
        {/each}
      </ul>
      <p data-hint class="v-help">{t("„Prüfen“ zeigt jede Änderung im Editor.")}</p>
    </div>
  {/if}
</svelte:element>

{#if agent.pendingCommand}
  <CommandConfirm prompt={agent.pendingCommand} onAnswer={answerCommand} />
{/if}

<style>
  .v-agent-bar { display: flex; justify-content: flex-end; }
  .v-agent-log { display: grid; gap: var(--v-space-2); max-height: 18rem; overflow-y: auto; padding-right: var(--v-space-1); }
  .v-agent-log p { margin: 0; }
  .v-agent-user { align-self: end; justify-self: end; max-width: 90%; padding: var(--v-space-2) var(--v-space-3); border-radius: var(--v-radius-control); background: var(--v-surface-muted, rgb(var(--v-shade) / .08)); color: var(--v-text-primary); font-size: var(--v-text-sm); white-space: pre-wrap; overflow-wrap: anywhere; }
  .v-agent-step { overflow: hidden; color: var(--v-text-muted); font-size: var(--v-text-xs); text-overflow: ellipsis; white-space: nowrap; }
  .v-agent-answer { color: var(--v-text-primary); font-size: var(--v-text-sm); line-height: 1.55; white-space: pre-wrap; overflow-wrap: anywhere; }
  .v-agent-heading { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-sm); font-weight: 600; }
  .v-agent-path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
