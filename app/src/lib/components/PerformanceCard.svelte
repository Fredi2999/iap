<script lang="ts">
  // Leistungs-Check: misst, wie schnell das Modell auf diesem PC antwortet,
  // und empfiehlt daraus Denkstufe und gegebenenfalls ein kleineres Modell.
  import { onMount } from "svelte";
  import { t, tk, locale } from "../i18n/index.svelte";
  import { performanceReport, runPerformanceCheck } from "../ipc";
  import { formatBytes } from "../documents";
  import type { AvailableModel, PerfReport } from "../types";

  interface Props { models: AvailableModel[]; onError: (reason: unknown) => void; }
  let { models, onError }: Props = $props();

  const THINKING_NAMES: Record<string, string> = { kurz: tk("Kurz"), standard: tk("Standard"), "sorgfältig": tk("Sorgfältig") };

  let report = $state<PerfReport | null>(null);
  let running = $state(false);

  onMount(async () => {
    try { report = await performanceReport(); } catch (reason) { onError(reason); }
  });

  async function measure() {
    running = true;
    try { report = await runPerformanceCheck(); } catch (reason) { onError(reason); }
    finally { running = false; }
  }

  function number(value: number | null, digits: number, unit: string): string {
    return value == null ? t("noch nicht gemessen") : `${value.toLocaleString(locale(), { maximumFractionDigits: digits })} ${unit}`;
  }
  let recommendedModel = $derived(models.find((model) => model.id === report?.recommended_model_id) ?? null);
</script>

<section class="v-card v-stack" aria-labelledby="set-perf">
  <div class="v-card-title">
    <h2 id="set-perf" class="v-card-title-text">{t("Leistung auf diesem PC")}</h2>
    <button type="button" class="v-btn v-btn-ghost" onclick={measure} disabled={running}>{t(running ? "Misst …" : report ? "Erneut messen" : "Jetzt messen")}</button>
  </div>
  {#if running}
    <p data-hint class="v-card-text" aria-live="polite">{t("Eine kurze Probeaufgabe, meist 10 bis 30 Sekunden.")}</p>
  {:else if !report}
    <p class="v-card-text">{t("Noch nicht gemessen. Der Check zeigt, wie schnell Antworten hier kommen.")}</p>
  {:else}
    <ul class="v-list">
      <li><div class="v-list-main"><strong>{t("Antwortgeschwindigkeit")}</strong><span>{report.model_name}</span></div><span class="v-num">{number(report.tokens_per_second, 1, t("Token/s"))}</span></li>
      <li><div class="v-list-main"><strong>{t("Start des Modells")}</strong></div><span class="v-num">{number(report.model_start_ms == null ? null : report.model_start_ms / 1000, 1, "s")}</span></li>
      <li><div class="v-list-main"><strong>{t("Arbeitsspeicher des Modells")}</strong><span>{t("von {total} im PC", { total: formatBytes(report.total_ram_bytes) })}</span></div><span class="v-num">{report.ram_peak_bytes == null ? t("noch nicht gemessen") : formatBytes(report.ram_peak_bytes)}</span></li>
    </ul>
    {#if report.recommended_thinking}
      <p class="v-notice">
        {t("Empfohlene Denkstufe: {level}.", { level: t(THINKING_NAMES[report.recommended_thinking] ?? report.recommended_thinking) })}
        {#if recommendedModel}{" "}{t("Schneller wäre hier das Modell „{name}“.", { name: recommendedModel.display_name })}{/if}
      </p>
    {/if}
    <p class="v-help">{t("Gemessen am {date}.", { date: new Date(report.measured_at_unix_ms).toLocaleString(locale()) })}</p>
  {/if}
</section>
