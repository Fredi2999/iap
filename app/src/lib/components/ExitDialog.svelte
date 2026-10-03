<script lang="ts">
  // Nachfrage beim Schließen: Spuren auf diesem PC entfernen oder behalten.
  // Keine Animation: der Dialog erscheint bei jedem Beenden und soll nicht bremsen.
  import { onMount } from "svelte";
  import { t, tk, locale } from "../i18n/index.svelte";
  import { hostTraces, quitApp, setAskOnExit } from "../ipc";
  import { friendlyError } from "../errors";
  import type { HostTraceReport } from "../types";

  interface Props { onCancel: () => void; }
  let { onCancel }: Props = $props();

  const TRACE_LABELS: Record<string, string> = {
    model_cache: tk("Modellkopie für einen schnelleren Start"),
    vault_hot: tk("Arbeitskopie des verschlüsselten Tresors"),
    legacy_webview: tk("Browserdaten älterer IAP-Versionen"),
  };

  let report = $state<HostTraceReport | null>(null);
  let myPc = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let primary = $state<HTMLButtonElement | null>(null);

  function formatSize(bytes: number): string {
    const units = ["B", "KB", "MB", "GB"];
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit++; }
    return `${value.toLocaleString(locale(), { maximumFractionDigits: unit >= 2 ? 1 : 0 })} ${units[unit]}`;
  }

  async function finish(purge: boolean) {
    busy = true;
    error = null;
    try {
      if (myPc) await setAskOnExit(false);
      await quitApp(purge);
    } catch (reason) {
      busy = false;
      error = friendlyError(reason).message;
    }
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape" && !busy) { event.preventDefault(); onCancel(); }
  }

  onMount(() => {
    hostTraces().then((value) => { report = value; }).catch((reason) => { error = friendlyError(reason).message; });
    primary?.focus();
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  });
</script>

<div class="v-exit-layer">
  <div class="v-exit-shade" aria-hidden="true"></div>
  <div class="v-exit" role="alertdialog" aria-modal="true" aria-labelledby="v-exit-title" aria-describedby="v-exit-text">
    <h2 id="v-exit-title">{t("IAP beenden")}</h2>
    <p id="v-exit-text">{t("IAP hat auf diesem PC Daten abgelegt, damit der nächste Start schneller geht. Sollen sie entfernt werden?")}</p>

    {#if report && report.traces.length > 0}
      <ul class="v-exit-list">
        {#each report.traces as trace (trace.id)}
          <li title={trace.path}><span>{t(TRACE_LABELS[trace.id] ?? trace.id)}</span><span class="v-num">{formatSize(trace.size_bytes)}</span></li>
        {/each}
      </ul>
      {#if report.traces.some((trace) => trace.id === "model_cache")}
        <p class="v-help">{t("Ohne Modellkopie wird das Modell beim nächsten Start auf diesem PC erneut vom Stick kopiert. Das kann einige Minuten dauern.")}</p>
      {/if}
    {:else if report}
      <p class="v-help">{t("Auf diesem PC liegen keine Daten von IAP.")}</p>
    {/if}

    {#if error}<p class="v-exit-error" role="alert">{t(error)}</p>{/if}

    <label class="v-exit-check"><input type="checkbox" bind:checked={myPc} disabled={busy} /> {t("Das ist mein PC, nicht mehr fragen")}</label>

    <div class="v-exit-actions">
      <button type="button" class="v-btn v-btn-ghost" onclick={onCancel} disabled={busy}>{t("Abbrechen")}</button>
      <button type="button" class="v-btn v-btn-ghost" onclick={() => finish(false)} disabled={busy}>{t("Nur beenden")}</button>
      <button type="button" class="v-btn v-btn-primary" bind:this={primary} onclick={() => finish(true)} disabled={busy || !report || report.traces.length === 0}>{t(busy ? "IAP wird beendet …" : "Spuren entfernen und beenden")}</button>
    </div>
  </div>
</div>

<style>
  .v-exit-layer { position: fixed; inset: 0; z-index: 90; display: flex; align-items: center; justify-content: center; padding: 1rem; }
  .v-exit-shade { position: absolute; inset: 0; background: rgb(var(--v-shade) / .55); }
  .v-exit { position: relative; width: min(30rem, 100%); display: flex; flex-direction: column; gap: var(--v-space-4); padding: var(--v-space-6); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: 0 25px 70px rgb(var(--v-shade) / .42); }
  h2 { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-lg); font-weight: 600; }
  p { margin: 0; color: var(--v-text-secondary); font-size: var(--v-text-md); line-height: 1.5; }
  .v-exit-list { margin: 0; padding: 0; list-style: none; border: 1px solid var(--v-line); border-radius: var(--v-radius-field); }
  .v-exit-list li { display: flex; justify-content: space-between; gap: var(--v-space-4); padding: var(--v-space-3) var(--v-space-4); color: var(--v-text-secondary); font-size: var(--v-text-sm); }
  .v-exit-list li + li { border-top: 1px solid var(--v-line); }
  .v-exit-list .v-num { color: var(--v-text-primary); white-space: nowrap; }
  .v-exit-error { color: var(--v-danger); font-size: var(--v-text-sm); }
  .v-exit-check { display: flex; align-items: center; gap: var(--v-space-2); color: var(--v-text-secondary); font-size: var(--v-text-sm); }
  .v-exit-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--v-space-2); }
  @media (max-width: 480px) { .v-exit-actions { flex-direction: column-reverse; } .v-exit-actions .v-btn { width: 100%; } }
  /* Dialoge erscheinen zentriert: kurz aus 96 % mit Blende, ohne Federn (Emil: Modals bleiben mittig). */
  .v-exit { transition: opacity 200ms var(--v-ease-out-strong), transform 200ms var(--v-ease-out-strong); }
  .v-exit-shade { transition: opacity 200ms ease; }
  @starting-style { .v-exit { opacity: 0; transform: scale(.96); } .v-exit-shade { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .v-exit { transition: opacity 120ms ease; } @starting-style { .v-exit { transform: none; } } }
</style>
