<script lang="ts" module>
  // Einmal je Sitzung auf die Ladezeit hinweisen; danach nicht mehr stören.
  let warned = false;
</script>

<script lang="ts">
  // Optionale Funktion: dieselbe Frage mit einem anderen Modell beantworten
  // und beide Antworten nebeneinander zeigen. Der Vergleich wird nicht gespeichert.
  import { t } from "../i18n/index.svelte";
  import { compareAnswer, type CompareAnswer } from "../ipc";
  import { friendlyError } from "../errors";
  import Markdown from "./Markdown.svelte";
  import type { AvailableModel } from "../types";

  interface Props {
    question: string;
    answer: string;
    currentModelId: string;
    models: AvailableModel[];
    thinking: string;
    disabled: boolean;
    /** Meldet, solange ein Vergleich läuft; der Chat sperrt dann das Senden. */
    onBusy: (busy: boolean) => void;
  }
  let { question, answer, currentModelId, models, thinking, disabled, onBusy }: Props = $props();

  let others = $derived(models.filter((model) => model.id !== currentModelId));
  let choice = $state("");
  let running = $state(false);
  let result = $state<CompareAnswer | null>(null);
  let error = $state<string | null>(null);
  let currentName = $derived(models.find((model) => model.id === currentModelId)?.display_name ?? currentModelId);

  async function run() {
    const modelId = choice || others[0]?.id;
    if (!modelId) return;
    if (!warned && !confirm(t("Für den Vergleich lädt IAP kurz das andere Modell und wechselt danach zurück. Das kann eine Minute dauern. Fortfahren?"))) return;
    warned = true;
    running = true;
    error = null;
    onBusy(true);
    try {
      result = await compareAnswer(question, modelId, thinking);
    } catch (reason) {
      error = friendlyError(reason).message;
    } finally {
      running = false;
      onBusy(false);
    }
  }
</script>

{#if result}
  <div class="v-compare">
    <div class="v-compare-col">
      <strong>{currentName}</strong>
      <div class="v-compare-text"><Markdown content={answer} /></div>
    </div>
    <div class="v-compare-col">
      <strong>{result.model_name}{result.tokens_per_second != null ? ` · ${result.tokens_per_second.toFixed(1)} Token/s` : ""}</strong>
      <div class="v-compare-text"><Markdown content={result.answer} /></div>
    </div>
    <button type="button" class="v-link" onclick={() => (result = null)}>{t("Vergleich schließen")}</button>
  </div>
{:else}
  <div class="v-compare-bar">
    {#if others.length > 1}
      <select bind:value={choice} aria-label={t("Modell für den Vergleich")} disabled={running || disabled}>
        {#each others as model (model.id)}<option value={model.id}>{model.display_name}</option>{/each}
      </select>
    {/if}
    <button type="button" class="v-link" onclick={run} disabled={running || disabled}>
      {running ? t("Vergleich läuft … Modell wird geladen") : others.length === 1 ? t("Mit {name} vergleichen", { name: others[0].display_name }) : t("Mit anderem Modell vergleichen")}
    </button>
    {#if error}<span class="v-compare-error" role="alert">{t(error)}</span>{/if}
  </div>
{/if}

<style>
  .v-compare-bar { display: flex; flex-wrap: wrap; align-items: center; gap: var(--v-space-2); margin-top: var(--v-space-2); }
  .v-compare-bar select { padding: 2px 6px; font-size: var(--v-text-xs); }
  .v-link { padding: 0; border: 0; background: transparent; color: var(--v-text-muted); font-size: var(--v-text-xs); cursor: pointer; }
  .v-link:hover:not(:disabled) { color: var(--v-accent-blue); }
  .v-link:disabled { cursor: default; opacity: .7; }
  .v-compare-error { color: var(--v-danger); font-size: var(--v-text-xs); }
  .v-compare { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--v-space-3); margin-top: var(--v-space-3); }
  .v-compare-col { min-width: 0; padding: var(--v-space-3); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); background: rgb(var(--v-tint) / .03); }
  .v-compare-col > strong { display: block; margin-bottom: var(--v-space-2); color: var(--v-text-muted); font-size: var(--v-text-xs); font-weight: 600; }
  .v-compare-text { font-size: var(--v-text-sm); }
  .v-compare > .v-link { grid-column: 1 / -1; justify-self: start; }
  @media (max-width: 720px) { .v-compare { grid-template-columns: 1fr; } }
</style>
