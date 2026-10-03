<script lang="ts">
  import { t } from "../lib/i18n/index.svelte";
  import { errorText } from "../lib/errors";
  import { onMount } from "svelte";
  import {
    evaluateRubric,
    loadQuestionnaire,
    rubricDefault,
    runRubricMonteCarlo,
    saveQuestionnaire,
  } from "../lib/ipc";
  import type {
    CriterionScore,
    EvidenceSource,
    MonteCarloInput,
    MonteCarloResult,
    QuestionnaireResponses,
    Rubric,
    WeightedResult,
  } from "../lib/types";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import { tk, locale } from "../lib/i18n/index.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";


  type QuestionKey = keyof QuestionnaireResponses;

  interface QuestionField {
    key: QuestionKey;
    label: string;
    required: boolean;
    placeholder: string;
  }

  const QUESTIONS: QuestionField[] = [
    { key: "problem", label: tk("Problem"), required: true, placeholder: tk("Welches Problem wird gelöst?") },
    { key: "audience", label: tk("Zielgruppe"), required: true, placeholder: tk("Für wen?") },
    { key: "solution", label: tk("Lösung"), required: true, placeholder: tk("Was liefert die Idee konkret?") },
    { key: "revenue_model", label: tk("Erlösmodell"), required: true, placeholder: tk("Wie wird Geld verdient?") },
    { key: "cost_drivers", label: tk("Kostentreiber"), required: true, placeholder: tk("Was kostet den Aufbau?") },
    { key: "competition", label: tk("Wettbewerb"), required: false, placeholder: tk("Wer macht Ähnliches?") },
    { key: "unfair_advantage", label: tk("Besonderer Vorteil"), required: false, placeholder: tk("Was habt ihr, was andere nicht kopieren können?") },
    { key: "regulatory_context", label: tk("Rechtliches"), required: false, placeholder: tk("Rechtliche Rahmenbedingungen") },
    { key: "required_capital", label: tk("Kapitalbedarf"), required: false, placeholder: tk("Wie viel Startkapital?") },
    { key: "time_to_first_revenue", label: tk("Zeit bis erste Einnahmen"), required: false, placeholder: tk("In Wochen oder Monaten") },
  ];

  const SOURCE_LABELS: Record<EvidenceSource, string> = {
    user: tk("Eigene Einschätzung"),
    document: tk("Dokument"),
    assumption: tk("Annahme"),
  };

  type RangeKey =
    | "price_per_unit"
    | "units_per_month"
    | "fixed_cost_per_month"
    | "variable_cost_per_unit";

  const RANGE_FIELDS: { label: string; key: RangeKey }[] = [
    { label: tk("Preis pro Einheit"), key: "price_per_unit" },
    { label: tk("Einheiten pro Monat"), key: "units_per_month" },
    { label: tk("Fixkosten pro Monat"), key: "fixed_cost_per_month" },
    { label: tk("Variable Kosten pro Einheit"), key: "variable_cost_per_unit" },
  ];

  const SOURCE_ENTRIES = Object.entries(SOURCE_LABELS) as [EvidenceSource, string][];
  const SCORE_OPTIONS: number[] = [1, 2, 3, 4, 5];

  function emptyQuestionnaire(): QuestionnaireResponses {
    return {
      problem: null,
      audience: null,
      solution: null,
      revenue_model: null,
      cost_drivers: null,
      competition: null,
      unfair_advantage: null,
      regulatory_context: null,
      required_capital: null,
      time_to_first_revenue: null,
    };
  }

  let responses = $state<QuestionnaireResponses>(emptyQuestionnaire());
  let rubric = $state<Rubric | null>(null);
  let scores = $state<Record<string, CriterionScore>>({});
  let weighted = $state<WeightedResult | null>(null);
  let questionnaireStatus = $state<string>("");
  let missingFields = $state<string[]>([]);
  let evaluationError = $state<string | null>(null);
  let loading = $state(true);
  let step = $state<"idea" | "score" | "sim">("idea");
  let loadError = $state<string | null>(null);

  let mcInput = $state<MonteCarloInput>({
    price_per_unit: { min: 10, max: 20 },
    units_per_month: { min: 100, max: 200 },
    fixed_cost_per_month: { min: 500, max: 1000 },
    variable_cost_per_unit: { min: 2, max: 5 },
    starting_capital: 10000,
  });
  let mcRuns = $state<number>(10000);
  let mcSeed = $state<number>(42);
  let mcResult = $state<MonteCarloResult | null>(null);
  let mcRunning = $state(false);
  let mcError = $state<string | null>(null);

  onMount(async () => {
    try {
      rubric = await rubricDefault();
      for (const criterion of rubric.criteria) {
        scores[criterion.id] = {
          criterion_id: criterion.id,
          score: 3,
          rationale: "",
          source: "user",
        };
      }
      responses = await loadQuestionnaire();
    } catch (reason) {
      loadError = errorText(reason);
    } finally {
      loading = false;
    }
  });

  function questionValue(key: QuestionKey): string {
    return responses[key] ?? "";
  }

  function setQuestion(key: QuestionKey, value: string) {
    const trimmed = value.length > 0 ? value : null;
    responses = { ...responses, [key]: trimmed };
  }

  async function persistQuestionnaire() {
    questionnaireStatus = t("Wird gespeichert …");
    try {
      const result = await saveQuestionnaire(responses);
      missingFields = result.missing_fields;
      const time = new Date(result.saved_unix_ms).toLocaleTimeString();
      questionnaireStatus =
        result.missing_fields.length === 0
          ? t("Vollständig gespeichert um {time}", { time })
          : t("Gespeichert. Noch offen: {fields}", { fields: result.missing_fields.map((key) => t(QUESTIONS.find((q) => q.key === key)?.label ?? key)).join(", ") });
    } catch (reason) {
      questionnaireStatus = t("Fehler: {reason}", { reason: errorText(reason) });
    }
  }

  function setScore(criterionId: string, score: number) {
    scores[criterionId] = { ...scores[criterionId], score };
  }

  function setRationale(criterionId: string, rationale: string) {
    scores[criterionId] = { ...scores[criterionId], rationale };
  }

  function setSource(criterionId: string, source: EvidenceSource) {
    scores[criterionId] = { ...scores[criterionId], source };
  }

  async function runEvaluation() {
    if (!rubric) return;
    evaluationError = null;
    const sheet = { scores: rubric.criteria.map((c) => scores[c.id]) };
    try {
      weighted = await evaluateRubric(sheet);
    } catch (reason) {
      evaluationError = errorText(reason);
      weighted = null;
    }
  }

  async function runMonteCarlo() {
    mcRunning = true;
    mcError = null;
    try {
      mcResult = await runRubricMonteCarlo({
        input: mcInput,
        runs: Math.max(1, Math.trunc(mcRuns)),
        seed: Math.trunc(mcSeed),
      });
    } catch (reason) {
      mcError = errorText(reason);
      mcResult = null;
    } finally {
      mcRunning = false;
    }
  }

  function fmt(value: number, digits = 2): string {
    return value.toLocaleString(locale(), {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    });
  }

  function fmtMonths(value: number | null): string {
    if (value === null || !Number.isFinite(value)) return "nicht absehbar";
    return t("{n} Monate", { n: value.toLocaleString(locale(), { maximumFractionDigits: 1 }) });
  }
</script>

<div class="v-page">
  <PageHeader title={t("Bewertung")} description="Prüfe eine Geschäfts- oder Projektidee in drei Schritten. Alle Zahlen berechnet IAP exakt, nicht das Sprachmodell.">
    {#snippet help()}{t("Schritt 1 hält deine Idee fest. In Schritt 2 vergibst du pro Kriterium 1 bis 5 Punkte mit Begründung. Schritt 3 spielt Preise, Mengen und Kosten zehntausendfach zufällig durch und zeigt, wie wahrscheinlich sich die Idee trägt.")}{/snippet}
  </PageHeader>

  {#if loading}
    <p class="v-card-text">{t("Wird geladen …")}</p>
  {:else if loadError}
    <ErrorNotice error={loadError} onRetry={() => location.reload()} />
  {:else if rubric}
    <div class="v-segmented v-steps" role="tablist" aria-label={t("Schritte")}>
      {#each [["idea", tk("1  Idee beschreiben")], ["score", tk("2  Bewerten")], ["sim", tk("3  Zahlen durchspielen")]] as [id, label] (id)}
        <button type="button" role="tab" aria-selected={step === id} class:active={step === id} onclick={() => (step = id as typeof step)}>{t(label)}</button>
      {/each}
    </div>

    {#if step === "idea"}
      <section class="v-card v-stack">
        <div class="v-grid-2">
          {#each QUESTIONS as question (question.key)}
            <label class="v-field">
              <span class="v-label">{t(question.label)}{#if question.required} <span class="v-chip">{t("Pflicht")}</span>{/if}</span>
              <textarea rows="2" placeholder={t(question.placeholder)} value={questionValue(question.key)}
                oninput={(e) => setQuestion(question.key, (e.currentTarget as HTMLTextAreaElement).value)}></textarea>
            </label>
          {/each}
        </div>
        <div class="v-row" style="justify-content: space-between">
          <span class="v-help">{questionnaireStatus}</span>
          <div class="v-row">
            <button class="v-btn v-btn-ghost" onclick={persistQuestionnaire}>{t("Speichern")}</button>
            <button class="v-btn v-btn-primary" onclick={async () => { await persistQuestionnaire(); step = "score"; }}>{t("Weiter zur Bewertung")}</button>
          </div>
        </div>
      </section>
    {:else if step === "score"}
      <div class="v-grid-aside">
        <section class="v-card v-stack">
          {#each rubric.criteria as criterion (criterion.id)}
            <div class="v-criterion">
              <div class="v-card-title"><span>{criterion.label}</span><small>{t("Gewicht {n} %", { n: (criterion.weight * 100).toFixed(0) })}</small></div>
              <p class="v-card-text">{criterion.description}</p>
              <div class="v-row">
                <div class="v-segmented" role="group" aria-label={t("Punkte für {name}", { name: criterion.label })}>
                  {#each SCORE_OPTIONS as value (value)}
                    <button type="button" class:active={(scores[criterion.id]?.score ?? 3) === value} aria-pressed={(scores[criterion.id]?.score ?? 3) === value} onclick={() => setScore(criterion.id, value)}>{value}</button>
                  {/each}
                </div>
                <select aria-label={t("Grundlage")} value={scores[criterion.id]?.source ?? "user"} onchange={(e) => setSource(criterion.id, (e.currentTarget as HTMLSelectElement).value as EvidenceSource)}>
                  {#each SOURCE_ENTRIES as entry (entry[0])}<option value={entry[0]}>{t(entry[1])}</option>{/each}
                </select>
              </div>
              <input placeholder={t("Warum diese Punktzahl?")} value={scores[criterion.id]?.rationale ?? ""} aria-label={"Begründung für " + criterion.label}
                oninput={(e) => setRationale(criterion.id, (e.currentTarget as HTMLInputElement).value)} />
            </div>
          {/each}
        </section>
        <aside class="v-card v-stack v-sticky">
          <h2 class="v-card-title">{t("Ergebnis")}</h2>
          {#if evaluationError}<ErrorNotice error={evaluationError} onDismiss={() => (evaluationError = null)} />{/if}
          {#if weighted}
            <div class="v-score"><span class="v-num">{fmt(weighted.weighted_score, 1)}</span><small>{t("von 5 Punkten")}</small></div>
            <details class="v-details">
              <summary>{t("Wie setzt sich das zusammen?")}</summary>
              <ul class="v-list">{#each weighted.contributions as c (c.criterion_id)}<li><div class="v-list-main"><strong>{c.label}</strong><span>{t("{score} Punkte · Gewicht {weight} %", { score: c.score, weight: (c.weight * 100).toFixed(0) })}</span></div><span class="v-num v-help">+{fmt(c.contribution, 2)}</span></li>{/each}</ul>
            </details>
          {:else}
            <p data-hint class="v-card-text">{t("Vergib links Punkte und lass dann auswerten.")}</p>
          {/if}
          <button class="v-btn v-btn-primary" onclick={runEvaluation}>{t("Auswerten")}</button>
        </aside>
      </div>
    {:else}
      <section class="v-card v-stack">
        <p data-hint class="v-card-text">{t("Gib für jeden Wert eine Spanne an. IAP spielt viele zufällige Kombinationen durch.")}</p>
        <div class="v-grid-3">
          {#each RANGE_FIELDS as range (range.key)}
            <div class="v-field"><span class="v-label">{t(range.label)}</span>
              <div class="v-range-pair">
                <input type="number" step="any" aria-label={t("{name} mindestens", { name: t(range.label) })} value={mcInput[range.key].min} oninput={(e) => (mcInput[range.key] = { ...mcInput[range.key], min: Number((e.currentTarget as HTMLInputElement).value) })} />
                <span class="v-help">{t("bis")}</span>
                <input type="number" step="any" aria-label={t("{name} höchstens", { name: t(range.label) })} value={mcInput[range.key].max} oninput={(e) => (mcInput[range.key] = { ...mcInput[range.key], max: Number((e.currentTarget as HTMLInputElement).value) })} />
              </div>
            </div>
          {/each}
          <label class="v-field"><span class="v-label">{t("Startkapital (€)")}</span>
            <input type="number" step="any" value={mcInput.starting_capital} oninput={(e) => (mcInput.starting_capital = Number((e.currentTarget as HTMLInputElement).value))} />
          </label>
        </div>
        <details class="v-details">
          <summary>{t("Erweitert")}</summary>
          <div class="v-grid-3">
            <label class="v-field"><span class="v-label">{t("Durchläufe")}</span><input type="number" min="1" value={mcRuns} oninput={(e) => (mcRuns = Number((e.currentTarget as HTMLInputElement).value))} /></label>
            <label class="v-field"><span class="v-label">{t("Startwert (für wiederholbare Ergebnisse)")}</span><input type="number" value={mcSeed} oninput={(e) => (mcSeed = Number((e.currentTarget as HTMLInputElement).value))} /></label>
          </div>
        </details>
        <div class="v-row v-row-end"><button class="v-btn v-btn-primary" onclick={runMonteCarlo} disabled={mcRunning}>{t(mcRunning ? "Rechne …" : "Durchspielen")}</button></div>
        {#if mcError}<ErrorNotice error={mcError} onDismiss={() => (mcError = null)} />{/if}
        {#if mcResult}
          <div class="v-grid-3 v-results">
            <div class="v-stat"><span>{t("Gewinn pro Monat, schlechter Fall")}</span><strong class="v-num">{fmt(mcResult.monthly_profit_p10, 0)} €</strong></div>
            <div class="v-stat accent"><span>{t("Gewinn pro Monat, typisch")}</span><strong class="v-num">{fmt(mcResult.monthly_profit_p50, 0)} €</strong></div>
            <div class="v-stat"><span>{t("Gewinn pro Monat, guter Fall")}</span><strong class="v-num">{fmt(mcResult.monthly_profit_p90, 0)} €</strong></div>
            <div class="v-stat"><span>{t("Chance auf Gewinn im Monat")}</span><strong class="v-num">{(mcResult.probability_positive_month * 100).toFixed(0)} %</strong></div>
            <div class="v-stat"><span>{t("Bis die Investition zurück ist")}</span><strong class="v-num">{fmtMonths(mcResult.expected_break_even_months)}</strong></div>
            <div class="v-stat"><span>{t("So lange reicht das Startkapital")}</span><strong class="v-num">{fmtMonths(mcResult.expected_runway_months)}</strong></div>
          </div>
          <p class="v-help">{t("Berechnet aus {n} Durchläufen.", { n: mcResult.runs.toLocaleString(locale()) })}</p>
        {/if}
      </section>
    {/if}
  {/if}
</div>

<style>
  .v-steps { align-self: flex-start; }
  .v-steps button { padding: .5rem 1rem; font-size: var(--v-text-sm); }
  .v-criterion { display: flex; flex-direction: column; gap: var(--v-space-2); padding-bottom: var(--v-space-4); border-bottom: 1px solid var(--v-line); }
  .v-criterion:last-child { border-bottom: 0; padding-bottom: 0; }
  .v-criterion .v-card-title { margin: 0; }
  .v-criterion select { padding: .35rem .6rem; font-size: var(--v-text-sm); }
  .v-sticky { position: sticky; top: 0; }
  .v-score { display: flex; align-items: baseline; gap: var(--v-space-2); }
  .v-score span { color: var(--v-text-primary); font-size: 2.75rem; font-weight: 300; line-height: 1; }
  .v-score small { color: var(--v-text-muted); font-size: var(--v-text-sm); }
  .v-range-pair { display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; gap: var(--v-space-2); }
  .v-stat { display: flex; flex-direction: column; gap: 4px; padding: var(--v-space-3) var(--v-space-4); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); }
  .v-stat span { color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .v-stat strong { color: var(--v-text-primary); font-size: var(--v-text-xl); font-weight: 500; }
  .v-stat.accent { border-color: color-mix(in oklab, var(--v-accent-blue) 45%, transparent); background: var(--v-accent-blue-soft); }
</style>
