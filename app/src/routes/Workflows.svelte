<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { t, tk } from "../lib/i18n/index.svelte";
  import WorkflowCanvas, { type Selection } from "../lib/components/WorkflowCanvas.svelte";
  import {
    connectorOverview, onWorkflowRunEvent, onWorkflowRunFinished, workflowActiveRun, workflowCancelRun, workflowDelete,
    listInstalledSkills, workflowList, workflowRunReport, workflowRuns, workflowSave, workflowStartRun, workflowValidate,
  } from "../lib/ipc";
  import { notify } from "../lib/notifications";
  import ProposalCard from "../lib/components/workflow/ProposalCard.svelte";
  import type {
    ConnectorOverview, GraphProblem, InstalledSkillView, RunBudget, RunEvent, RunStatus, RunSummary, RunUsage, WorkflowDefinition,
    WorkflowGraph, WorkflowResult,
  } from "../lib/types";
  import {
    connectProblem, dayTemplate, externalServices, inputPorts, localTemplate, memoryTemplate, newId, nodeTitle, outputPorts,
    researchTemplate, runtimeInputs, SERVICE_NAME, triageTemplate, weatherDayTemplate, wikiTemplate,
  } from "../lib/workflow";

  // Workflows: kleine, feste Abläufe zeichnen, prüfen und mit sichtbaren Grenzen
  // laufen lassen. Exa bekommt nur öffentliche Suchbegriffe, nur mit Freigabe für
  // genau diesen Lauf und nur bei ausgeschaltetem Air Gap. Läufe brechen ab, wenn
  // das Hauptfenster geschlossen wird, und setzen sich nie von selbst fort.
  interface Props {
    /** Springt zur Konnektoren-Seite (Air Gap und Exa-Schlüssel). */
    onOpenConnectors: () => void;
  }
  let { onOpenConnectors }: Props = $props();

  let definitions = $state<WorkflowDefinition[]>([]);
  let currentId = $state<string | null>(null);
  let name = $state("");
  let graph = $state<WorkflowGraph>({ version: 1, nodes: [], edges: [] });
  let selection = $state<Selection>(null);
  let dirty = $state(false);
  let problems = $state<GraphProblem[]>([]);
  let message = $state<string | null>(null);
  let overview = $state<ConnectorOverview | null>(null);
  let confirmDelete = $state(false);
  let canvas = $state<{ connect: (a: string, b: string, c: string, d: string) => void; removeSelection: () => void; fit: () => void } | null>(null);

  // Lauf
  let starting = $state(false);
  let dialog = $state(false);
  let budget = $state<RunBudget>({ max_searches: 4, max_iterations: 2, max_seconds: 300, max_tokens: 4000, max_cost_usd: 0.1 });
  let approved = $state(false);
  let publicConfirmed = $state(false);
  let runId = $state<string | null>(null);
  let runStatus = $state<RunStatus | null>(null);
  let usage = $state<RunUsage | null>(null);
  let events = $state<RunEvent[]>([]);
  let result = $state<WorkflowResult | null>(null);
  let activeNode = $state<string | null>(null);
  let doneNodes = $state<string[]>([]);
  let past = $state<RunSummary[]>([]);
  let skills = $state<InstalledSkillView[]>([]);
  let startInputs = $state<Record<string, string>>({});
  let handledProposals = $state<number[]>([]);
  const running = $derived(runStatus === "running");

  const selectedNode = $derived(selection && selection.kind === "node" ? graph.nodes.find((n) => n.id === selection?.id) ?? null : null);
  const selectedEdge = $derived(selection && selection.kind === "edge" ? graph.edges.find((e) => e.id === selection?.id) ?? null : null);
  const services = $derived(externalServices(graph));
  const needsWeb = $derived(services.length > 0);
  const serviceNames = $derived(services.map((service) => SERVICE_NAME[service]).join(", "));
  // Grund, warum der Ablauf nicht starten kann: Air Gap oder ein Dienst, der nicht bereit ist.
  const webBlock = $derived.by(() => {
    if (!needsWeb || !overview) return null;
    if (overview.air_gap) return tk("Air Gap ist eingeschaltet: Ein Ablauf mit externen Diensten kann nicht starten.");
    const missing = overview.services.find((entry) => services.includes(entry.id) && !entry.ready);
    return missing ? (missing.reason ?? tk("Ein benötigter Dienst ist nicht eingerichtet.")) : null;
  });
  const inputNodes = $derived(runtimeInputs(graph));
  // Alles, was an einen externen Dienst gehen könnte: feste Eingaben und die als öffentlich markierten Startfelder.
  const inputTexts = $derived([
    ...graph.nodes.flatMap((n) => (n.kind.type === "input" ? [n.kind.text] : [])),
    ...inputNodes.filter((entry) => entry.public).map((entry) => startInputs[entry.id] ?? ""),
  ]);
  const startInputsMissing = $derived(inputNodes.some((entry) => !(startInputs[entry.id] ?? "").trim()));
  const instructionSkills = $derived(skills.filter((skill) => skill.kind === "instructions"));
  const currentNode = $derived(activeNode ? graph.nodes.find((n) => n.id === activeNode) ?? null : null);

  function flash(text: string) { message = text; }
  function reason(error: unknown): string {
    return String(error ?? "").replace(/^(Ungültige Eingabe|Internal|Core-Fehler|Vault-Fehler|Launcher-Fehler): /, "");
  }

  // Rückgängig/Wiederholen: Schnappschüsse des Graphen; schnelle Folgeänderungen (Tippen) werden zusammengefasst.
  let history: string[] = [];
  let historyIndex = 0;
  let historyTimer: ReturnType<typeof setTimeout> | undefined;
  let canUndo = $state(false);
  let canRedo = $state(false);
  const snapshotJson = () => JSON.stringify($state.snapshot(graph));
  function syncHistoryFlags() { canUndo = historyIndex > 0; canRedo = historyIndex < history.length - 1; }
  function resetHistory() {
    clearTimeout(historyTimer);
    history = [snapshotJson()];
    historyIndex = 0;
    syncHistoryFlags();
  }
  function recordHistory() {
    clearTimeout(historyTimer);
    const snap = snapshotJson();
    if (history[historyIndex] === snap) return;
    history = [...history.slice(0, historyIndex + 1), snap].slice(-100);
    historyIndex = history.length - 1;
    syncHistoryFlags();
  }
  function stepHistory(direction: -1 | 1) {
    if (running) return;
    recordHistory();
    const next = historyIndex + direction;
    if (next < 0 || next >= history.length) return;
    historyIndex = next;
    graph = JSON.parse(history[next]) as WorkflowGraph;
    selection = null;
    syncHistoryFlags();
    touch(false);
  }
  function duplicateSelected() {
    const node = selectedNode;
    if (!node || running || node.kind.type === "manual_start") return;
    const copy = { ...structuredClone($state.snapshot(node)), id: newId("n"), x: node.x + 30, y: node.y + 30 };
    graph.nodes.push(copy);
    selection = { kind: "node", id: copy.id };
    scheduleValidate();
  }
  function onWorkflowKey(event: KeyboardEvent) {
    if ((event.target as HTMLElement).closest("input, textarea, select")) return;
    const mod = event.ctrlKey || event.metaKey;
    if (!mod) return;
    const key = event.key.toLowerCase();
    if (key === "z" && !event.shiftKey) { event.preventDefault(); stepHistory(-1); }
    else if (key === "y" || (key === "z" && event.shiftKey)) { event.preventDefault(); stepHistory(1); }
    else if (key === "d") { event.preventDefault(); duplicateSelected(); }
  }

  let validateTimer: ReturnType<typeof setTimeout> | undefined;
  /** Nach jeder Änderung: prüfen und (zusammengefasst) im Verlauf merken. */
  function scheduleValidate() { touch(true); }
  function touch(record: boolean) {
    dirty = true;
    if (record) {
      clearTimeout(historyTimer);
      historyTimer = setTimeout(recordHistory, 500);
    }
    clearTimeout(validateTimer);
    validateTimer = setTimeout(async () => {
      try { problems = (await workflowValidate($state.snapshot(graph))).problems; }
      catch (error) { flash(reason(error)); }
    }, 300);
  }

  async function refreshPast() {
    past = currentId ? await workflowRuns(currentId).catch(() => []) : [];
  }

  // Erst nach dem Zeichnen messen, sonst ist die Fläche noch 0 Pixel breit.
  async function showCanvas() {
    await tick();
    await new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done())));
    canvas?.fit();
  }

  function resetRun() {
    runId = null; runStatus = null; usage = null; events = []; result = null; activeNode = null; doneNodes = [];
    approved = false; publicConfirmed = false; dialog = false; handledProposals = [];
  }

  function open(definition: WorkflowDefinition) {
    if (running) return;
    currentId = definition.id;
    name = definition.name;
    graph = structuredClone($state.snapshot(definition.graph)) as WorkflowGraph;
    selection = null;
    message = null;
    confirmDelete = false;
    resetRun();
    resetHistory();
    touch(false);
    dirty = false;
    void refreshPast();
    void showCanvas();
  }

  function createNew(kind: "research" | "local" | "day" | "memory" | "triage" | "wiki" | "weather" | "empty") {
    if (running) return;
    currentId = newId("wf");
    const titles = { research: "Recherche mit Quellenprüfung", local: "Text lokal bearbeiten", day: "Tagesüberblick", memory: "Frage mit Gedächtnis", triage: "Nachricht sortieren", wiki: "Recherche kostenlos", weather: "Wetter im Tagesüberblick", empty: "Neuer Ablauf" };
    name = t(titles[kind]);
    const builders = { research: researchTemplate, local: localTemplate, day: dayTemplate, memory: memoryTemplate, triage: triageTemplate, wiki: wikiTemplate, weather: weatherDayTemplate, empty: () => ({ version: 1, nodes: [], edges: [] }) as WorkflowGraph };
    graph = builders[kind]();
    selection = null;
    message = null;
    confirmDelete = false;
    resetRun();
    past = [];
    resetHistory();
    touch(false);
    void showCanvas();
  }

  async function save(): Promise<boolean> {
    if (!currentId) return false;
    try {
      const saved = await workflowSave({ id: currentId, name: name.trim() || t("Neuer Ablauf"), graph: $state.snapshot(graph), updated_unix_ms: 0 });
      definitions = await workflowList();
      name = saved.name;
      dirty = false;
      return true;
    } catch (error) {
      flash(reason(error));
      return false;
    }
  }

  async function remove() {
    if (!currentId) return;
    const id = currentId;
    try {
      await workflowDelete(id);
      definitions = await workflowList();
    } catch (error) { flash(reason(error)); return; }
    currentId = null;
    confirmDelete = false;
    dirty = false;
    resetRun();
    graph = { version: 1, nodes: [], edges: [] };
  }

  async function backToList() {
    if (running) return;
    if (dirty && !(await save())) return;
    currentId = null;
    dirty = false;
    resetRun();
  }

  async function adoptRun(id: string) {
    const report = await workflowRunReport(id).catch(() => null);
    if (!report) return;
    const definition = definitions.find((d) => d.id === report.workflow_id);
    if (definition && currentId !== definition.id) {
      currentId = definition.id;
      name = definition.name;
      graph = structuredClone($state.snapshot(definition.graph)) as WorkflowGraph;
      resetHistory();
      touch(false);
      dirty = false;
      void showCanvas();
    }
    runId = report.run_id;
    runStatus = report.status;
    usage = report.usage;
    events = report.events;
    result = report.result ?? null;
    budget = report.budget;
    doneNodes = report.events.filter((e) => e.kind === "node_finished" && e.node_id).map((e) => e.node_id as string);
    activeNode = report.current_node ?? null;
  }

  onMount(() => {
    const unlisten: UnlistenFn[] = [];
    let alive = true;
    (async () => {
      try {
        definitions = await workflowList();
        overview = await connectorOverview();
        skills = await listInstalledSkills().catch(() => []);
        const handles = [
          await onWorkflowRunEvent((p) => {
            if (p.run_id !== runId) return;
            events = [...events, p.event];
            usage = p.usage;
            activeNode = p.current_node;
            if (p.event.kind === "node_finished" && p.event.node_id && !doneNodes.includes(p.event.node_id)) doneNodes = [...doneNodes, p.event.node_id];
            // Ein „Hinweis“-Knoten meldet sich als Mitteilung: erste Zeile Titel, Rest Text.
            if (p.event.kind === "notice") {
              const [title, ...rest] = p.event.message.split("\n");
              notify(title, rest.join(" ").slice(0, 160));
            }
          }),
          await onWorkflowRunFinished((report) => {
            if (report.run_id !== runId) return;
            runStatus = report.status;
            usage = report.usage;
            events = report.events;
            result = report.result ?? null;
            activeNode = null;
            void refreshPast();
          }),
        ];
        if (alive) unlisten.push(...handles); else handles.forEach((stop) => stop());
        // Ein Lauf, der beim Verlassen der Seite weiterlief, wird wieder sichtbar.
        const active = await workflowActiveRun();
        if (active && alive) await adoptRun(active);
      } catch (error) {
        flash(reason(error));
      }
    })();
    return () => { alive = false; clearTimeout(validateTimer); clearTimeout(historyTimer); unlisten.forEach((stop) => stop()); };
  });

  // Nicht gespeicherte Änderungen gehen beim Verlassen der Seite nicht verloren.
  onDestroy(() => {
    if (dirty && currentId && !running) {
      void workflowSave({ id: currentId, name: name.trim() || "Neuer Ablauf", graph: $state.snapshot(graph), updated_unix_ms: 0 }).catch(() => undefined);
    }
  });

  async function openDialog() {
    message = null;
    overview = await connectorOverview().catch(() => overview);
    if (!(await save())) return;
    const check = await workflowValidate($state.snapshot(graph));
    problems = check.problems;
    if (!check.ok) { flash(t("Der Ablauf ist noch nicht gültig. Die Befunde stehen unter der Zeichenfläche.")); return; }
    approved = false;
    publicConfirmed = false;
    startInputs = {};
    dialog = true;
  }

  async function start() {
    if (!currentId || starting) return;
    starting = true;
    message = null;
    try {
      const keep = { ...$state.snapshot(budget) };
      // resetRun() setzt die Freigabe zurück; sie muss vorher gelesen werden, sonst ginge sie nie mit.
      const approvedNow = approved;
      const typed = { ...$state.snapshot(startInputs) };
      resetRun();
      startInputs = typed;
      const id = await workflowStartRun({
        workflow_id: currentId,
        budget: keep,
        exa_approved: approvedNow,
        inputs: typed,
        tz_offset_minutes: -new Date().getTimezoneOffset(),
      });
      runId = id;
      runStatus = "running";
    } catch (error) {
      flash(reason(error));
    } finally { starting = false; }
  }

  async function cancel() {
    if (runId) await workflowCancelRun(runId).catch(() => false);
  }

  async function showPast(summary: RunSummary) {
    if (running) return;
    await adoptRun(summary.run_id);
  }

  // --- Inspektor: Verbindung per Auswahl anlegen (Tastatur-Weg) ---
  let linkFromPort = $state("");
  let linkTo = $state("");
  let linkToPort = $state("");
  const linkTargets = $derived(graph.nodes.filter((n) => n.id !== selectedNode?.id));
  const linkTargetNode = $derived(graph.nodes.find((n) => n.id === linkTo) ?? null);

  function addLink() {
    if (!selectedNode || !linkFromPort || !linkTo || !linkToPort) return;
    const problem = connectProblem(graph, selectedNode.id, linkFromPort, linkTo, linkToPort);
    if (problem) { flash(t(problem)); return; }
    canvas?.connect(selectedNode.id, linkFromPort, linkTo, linkToPort);
  }

  const statusText: Record<RunStatus, string> = {
    running: tk("Läuft"),
    finished: tk("Fertig: alle Kriterien belegt"),
    not_sufficiently_supported: tk("Nicht ausreichend belegt"),
    budget_exhausted: tk("Limit erreicht"),
    cancelled: tk("Abgebrochen"),
    failed: tk("Fehlgeschlagen"),
  };
  const statusChip = (s: RunStatus) => (s === "finished" ? "accent" : s === "failed" || s === "budget_exhausted" ? "danger" : s === "running" ? "" : "warn");
  const fmtTime = (ms: number) => new Date(ms).toLocaleTimeString();
  const pct = (used: number, max: number) => (max > 0 ? Math.min(100, Math.round((used / max) * 100)) : 0);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="wf" onkeydown={onWorkflowKey}>
  {#if !currentId}
    <!-- Startansicht: Vorlagen und gespeicherte Abläufe -->
    <section class="wf-intro" aria-labelledby="wf-h">
      <h2 id="wf-h">{t("Neuer Ablauf")}</h2>
      <p class="wf-lead">{t("Eine Kette aus festen Bausteinen. IAP prüft sie und führt sie mit sichtbaren Grenzen aus.")}</p>
      <div class="wf-templates">
        <button type="button" class="wf-tpl" onclick={() => createNew("research")}>
          <strong>{t("Recherche mit Quellenprüfung")}</strong>
          <span>{t("Sucht im Web, prüft deine Kriterien, sucht bei Lücken nach.")}</span>
          <span class="v-chip warn">{t("Braucht Exa")}</span>
        </button>
        <button type="button" class="wf-tpl" onclick={() => createNew("local")}>
          <strong>{t("Text lokal bearbeiten")}</strong>
          <span>{t("Text an das lokale Modell, Ergebnis angezeigt.")}</span>
          <span class="v-chip accent">{t("Läuft komplett lokal")}</span>
        </button>
        <button type="button" class="wf-tpl" onclick={() => createNew("day")}>
          <strong>{t("Tagesüberblick")}</strong>
          <span>{t("Termine und Aufgaben der nächsten Tage als Tagesplan.")}</span>
          <span class="v-chip accent">{t("Läuft komplett lokal")}</span>
        </button>
        <button type="button" class="wf-tpl" onclick={() => createNew("memory")}>
          <strong>{t("Frage mit Gedächtnis")}</strong>
          <span>{t("Frage stellen, IAP sucht Erinnerungen und antwortet.")}</span>
          <span class="v-chip accent">{t("Läuft komplett lokal")}</span>
        </button>
        <button type="button" class="wf-tpl" onclick={() => createNew("triage")}>
          <strong>{t("Nachricht sortieren")}</strong>
          <span>{t("Eilige Nachrichten bekommen eine Kurzantwort, der Rest wird Aufgabe.")}</span>
          <span class="v-chip accent">{t("Läuft komplett lokal")}</span>
        </button>
        <button type="button" class="wf-tpl" onclick={() => createNew("wiki")}>
          <strong>{t("Recherche kostenlos")}</strong>
          <span>{t("Sucht bei Wikipedia und fasst die Treffer zusammen.")}</span>
          <span class="v-chip warn">{t("Braucht Wikipedia")}</span>
        </button>
        <button type="button" class="wf-tpl" onclick={() => createNew("weather")}>
          <strong>{t("Wetter im Tagesüberblick")}</strong>
          <span>{t("Termine und Aufgaben plus Wetter für deinen Ort.")}</span>
          <span class="v-chip warn">{t("Braucht Open-Meteo")}</span>
        </button>
        <button type="button" class="wf-tpl" onclick={() => createNew("empty")}>
          <strong>{t("Leerer Ablauf")}</strong>
          <span>{t("Selbst Knoten für Knoten bauen.")}</span>
        </button>
      </div>
      {#if message}<p class="v-notice warn" role="alert">{message}</p>{/if}
    </section>
    {#if definitions.length > 0}
      <section class="wf-saved" aria-labelledby="wf-saved-h">
        <h2 id="wf-saved-h">{t("Gespeicherte Abläufe")}</h2>
        <ul class="wf-saved-list">
          {#each definitions as definition (definition.id)}
            <li><button type="button" class="wf-saved-item" onclick={() => open(definition)}>
              <strong>{definition.name}</strong>
              <span class="v-help">{t("{n} Knoten", { n: definition.graph.nodes.length })} · {new Date(definition.updated_unix_ms).toLocaleDateString()}</span>
            </button></li>
          {/each}
        </ul>
      </section>
    {/if}
  {:else}
    <header class="wf-bar">
      <button type="button" class="v-btn v-btn-ghost" onclick={backToList} disabled={running}>{t("Alle Abläufe")}</button>
      <div class="wf-name">
        <label class="v-label" for="wf-name">{t("Name")}</label>
        <input id="wf-name" type="text" bind:value={name} oninput={() => { dirty = true; }} maxlength="80" disabled={running} />
      </div>
      <div class="v-row wf-actions">
        <button type="button" class="v-btn v-btn-ghost" onclick={() => stepHistory(-1)} disabled={!canUndo || running} title={t("Rückgängig (Strg+Z)")}>{t("Rückgängig")}</button>
        <button type="button" class="v-btn v-btn-ghost" onclick={() => stepHistory(1)} disabled={!canRedo || running} title={t("Wiederholen (Strg+Y)")}>{t("Wiederholen")}</button>
        <button type="button" class="v-btn v-btn-ghost" onclick={save} disabled={!dirty || running}>{dirty ? t("Speichern") : t("Gespeichert")}</button>
        {#if confirmDelete}
          <button type="button" class="v-btn v-btn-primary" onclick={remove}>{t("Wirklich löschen")}</button>
          <button type="button" class="v-btn v-btn-ghost" onclick={() => { confirmDelete = false; }}>{t("Behalten")}</button>
        {:else}
          <button type="button" class="v-btn v-btn-ghost" onclick={() => { confirmDelete = true; }} disabled={running}>{t("Löschen")}</button>
        {/if}
        <button type="button" class="v-btn v-btn-primary" onclick={openDialog} disabled={running || graph.nodes.length === 0}>{t("Ablauf starten …")}</button>
      </div>
    </header>

    {#if message}<p class="v-notice warn" role="alert">{message}</p>{/if}
    {#if needsWeb && webBlock && !running}
      <div class="v-notice warn wf-banner" role="status">
        <span>{t(webBlock)}</span>
        <button type="button" class="v-btn v-btn-ghost" onclick={onOpenConnectors}>{t("Konnektoren öffnen")}</button>
      </div>
    {/if}

    {#if dialog}
      <section class="wf-card wf-dialog" aria-labelledby="wf-dlg">
        <h3 id="wf-dlg">{t("Lauf vorbereiten")}</h3>
        <p class="wf-lead">{t("Diese Grenzen gelten nur für diesen Lauf. Ist eine erreicht, endet er.")}</p>
        {#if inputNodes.length > 0}
          <div class="wf-start-inputs">
            {#each inputNodes as entry (entry.id)}
              <div class="v-field">
                <label class="v-label" for={`wf-in-${entry.id}`}>{entry.label}</label>
                <textarea id={`wf-in-${entry.id}`} rows="3" bind:value={startInputs[entry.id]}></textarea>
                <span class="v-help">{entry.public && needsWeb ? t("Dieser Text darf an einen externen Dienst gehen. Schreibe nichts Privates hinein.") : t("Dieser Text bleibt auf deinem Rechner.")}</span>
              </div>
            {/each}
          </div>
        {/if}
        <div class="wf-budget">
          {#if needsWeb}
            <div class="v-field"><label class="v-label" for="b-s">{t("Externe Aufrufe (höchstens)")}</label><input id="b-s" type="number" min="0" max="50" bind:value={budget.max_searches} /></div>
            <div class="v-field"><label class="v-label" for="b-i">{t("Wiederholungen (höchstens)")}</label><input id="b-i" type="number" min="0" max="5" bind:value={budget.max_iterations} /></div>
            <div class="v-field"><label class="v-label" for="b-c">{t("Kosten externer Dienste in US-Dollar (höchstens)")}</label><input id="b-c" type="number" min="0" max="5" step="0.01" bind:value={budget.max_cost_usd} /></div>
          {/if}
          <div class="v-field"><label class="v-label" for="b-t">{t("Zeit in Sekunden (höchstens)")}</label><input id="b-t" type="number" min="10" max="3600" bind:value={budget.max_seconds} /></div>
          <div class="v-field"><label class="v-label" for="b-k">{t("Token (höchstens)")}</label><input id="b-k" type="number" min="256" max="200000" step="256" bind:value={budget.max_tokens} /></div>
        </div>
        {#if needsWeb}
          <div class="v-notice">
            <strong>{t("Das geht an {dienste}:", { dienste: serviceNames })}</strong>
            <ul>{#each inputTexts as text}<li>„{text}“</li>{/each}</ul>
            <p>{t("Jede Anfrage steht im Protokoll. Vor jedem Limit bricht IAP ab. Wiederholungen formuliert das lokale Modell nur aus deinen Suchbegriffen und Kriterien.")}</p>
          </div>
          {#if webBlock}<p class="v-notice warn" role="alert">{t(webBlock)}</p>{/if}
          <label class="wf-check"><input type="checkbox" bind:checked={publicConfirmed} /><span>{t("Die Suchfragen oben sind öffentlich. Es steht nichts Privates darin.")}</span></label>
          <label class="wf-check"><input type="checkbox" bind:checked={approved} /><span>{t("Ich gebe {dienste} nur für diesen Lauf frei.", { dienste: serviceNames })}</span></label>
        {:else}
          <p class="v-help">{t("Dieser Ablauf nutzt kein Internet.")}</p>
        {/if}
        <div class="v-row v-row-end">
          <button type="button" class="v-btn v-btn-ghost" onclick={() => { dialog = false; }}>{t("Abbrechen")}</button>
          <button type="button" class="v-btn v-btn-primary" onclick={start} disabled={starting || startInputsMissing || (needsWeb && (!!webBlock || !approved || !publicConfirmed))}>{starting ? t("Starte …") : t("Lauf starten")}</button>
        </div>
      </section>
    {/if}

    {#if runStatus}
      <section class="wf-card wf-run" aria-labelledby="wf-run" aria-live="polite">
        <header class="wf-run-head">
          <h3 id="wf-run">{t("Lauf")}</h3>
          <span class={`v-chip ${statusChip(runStatus)}`}>{#if running}<span class="wf-spin" aria-hidden="true"></span>{/if}{t(statusText[runStatus])}</span>
          {#if running && currentNode}<span class="v-help">{t("Gerade:")} {t(nodeTitle(currentNode.kind))}</span>{/if}
          {#if running}<button type="button" class="v-btn v-btn-ghost wf-run-cancel" onclick={cancel}>{t("Lauf abbrechen")}</button>{/if}
        </header>
        {#if usage}
          <dl class="wf-usage">
            {#if needsWeb}
              <div><dt>{t("Externe Aufrufe")}</dt><dd class="v-num">{usage.searches} / {budget.max_searches}</dd><i style={`--p:${pct(usage.searches, budget.max_searches)}%`}></i></div>
              <div><dt>{t("Wiederholungen")}</dt><dd class="v-num">{usage.iterations} / {budget.max_iterations}</dd><i style={`--p:${pct(usage.iterations, budget.max_iterations)}%`}></i></div>
              <div><dt>{t("Kosten externer Dienste")}</dt><dd class="v-num">{usage.cost_usd.toFixed(4)} / {budget.max_cost_usd.toFixed(2)} $</dd><i style={`--p:${pct(usage.cost_usd, budget.max_cost_usd)}%`}></i></div>
            {/if}
            <div><dt>{t("Zeit")}</dt><dd class="v-num">{Math.round(usage.seconds)} / {budget.max_seconds} s</dd><i style={`--p:${pct(usage.seconds, budget.max_seconds)}%`}></i></div>
            <div><dt>{t("Token")}</dt><dd class="v-num">{usage.tokens} / {budget.max_tokens}</dd><i style={`--p:${pct(usage.tokens, budget.max_tokens)}%`}></i></div>
          </dl>
        {/if}
        {#if result}
          <div class="wf-result">
            <h4>{t("Ergebnis")}</h4>
            <p class="wf-answer">{result.answer}</p>
            {#if result.sources.length > 0}
              <h4>{t("Quellen")}</h4>
              <ul class="v-list">{#each result.sources as source}<li><span>{source.title}</span> <span class="v-help v-num">{source.url}</span></li>{/each}</ul>
            {/if}
            {#if result.open_points.length > 0}
              <h4>{t("Offene Punkte")}</h4>
              <ul>{#each result.open_points as point}<li>{point}</li>{/each}</ul>
            {/if}
            {#if (result.proposals ?? []).length > 0}
              <h4>{t("Vorschläge zum Ablegen")}</h4>
              <p class="v-help">{t("Nichts gespeichert. Prüfe jeden Vorschlag.")}</p>
              {#each result.proposals ?? [] as proposal, index (index)}
                {#if !handledProposals.includes(index)}
                  <ProposalCard {proposal} onDone={() => { handledProposals = [...handledProposals, index]; }} />
                {/if}
              {/each}
            {/if}
          </div>
        {/if}
        <details class="v-details" open={running}>
          <summary>{t("Protokoll")} ({events.length})</summary>
          <ol class="wf-log">
            {#each events as event (event.seq)}
              <li class={event.kind}>
                <span class="v-num v-help">{fmtTime(event.at_unix_ms)}</span>
                <span>{event.message}</span>
                {#if event.origin}<span class="v-chip">{event.origin === "user_public" ? t("öffentlich (deine Eingabe)") : event.origin === "web_content" ? t("Webinhalt, unvertrauenswürdig") : t("privat")}</span>{/if}
              </li>
            {/each}
          </ol>
        </details>
      </section>
    {/if}

    <WorkflowCanvas bind:this={canvas} bind:graph bind:selection {problems} {activeNode} {doneNodes} locked={running} onChange={scheduleValidate} onMessage={flash} />

    <div class="wf-grid">
      <section class="wf-card" aria-labelledby="wf-inspector">
        <h3 id="wf-inspector">{t("Inspektor")}</h3>
        {#if selectedNode}
          <p class="v-help"><strong>{t(nodeTitle(selectedNode.kind))}</strong></p>
          {#if selectedNode.kind.type === "input"}
            <div class="v-field">
              <label class="v-label" for="wf-input">{needsWeb ? t("Öffentliche Suchfrage") : t("Text")}</label>
              <textarea id="wf-input" rows="3" bind:value={selectedNode.kind.text} oninput={scheduleValidate} disabled={running}></textarea>
              <span class="v-help">{needsWeb ? t("Nur dieser Text geht an den Dienst. Schreibe nichts Privates hinein.") : t("Dieser Text bleibt auf deinem Rechner.")}</span>
            </div>
          {:else if selectedNode.kind.type === "exa_search"}
            <div class="v-field"><label class="v-label" for="wf-n">{t("Treffer je Suche (1 bis 10)")}</label>
              <input id="wf-n" type="number" min="1" max="10" bind:value={selectedNode.kind.num_results} oninput={scheduleValidate} disabled={running} /></div>
          {:else if selectedNode.kind.type === "exa_contents"}
            <div class="v-field"><label class="v-label" for="wf-c">{t("Zeichen je Seite (200 bis 20000)")}</label>
              <input id="wf-c" type="number" min="200" max="20000" step="100" bind:value={selectedNode.kind.max_characters} oninput={scheduleValidate} disabled={running} /></div>
          {:else if selectedNode.kind.type === "local_model"}
            <div class="v-field"><label class="v-label" for="wf-m">{t("Anweisung an das Modell")}</label>
              <textarea id="wf-m" rows="4" bind:value={selectedNode.kind.instruction} oninput={scheduleValidate} disabled={running}></textarea>
              <span data-hint class="v-help">{t("Das Modell arbeitet ohne Chat, Gedächtnis und Werkzeuge. Webtext ist für es nur Daten.")}</span></div>
          {:else if selectedNode.kind.type === "check"}
            <div class="v-field"><label class="v-label" for="wf-k">{t("Kriterien (eins pro Zeile)")}</label>
              <textarea id="wf-k" rows="4" value={selectedNode.kind.criteria.join("\n")} oninput={(e) => { if (selectedNode?.kind.type === "check") { selectedNode.kind.criteria = e.currentTarget.value.split("\n"); scheduleValidate(); } }} disabled={running}></textarea>
              <span data-hint class="v-help">{t("Belegt nur, wenn die Quellen jedes Kriterium tragen.")}</span></div>
          {:else if selectedNode.kind.type === "runtime_input"}
            <div class="v-field"><label class="v-label" for="wf-rl">{t("Beschriftung des Feldes")}</label>
              <input id="wf-rl" type="text" maxlength="80" bind:value={selectedNode.kind.label} oninput={scheduleValidate} disabled={running} /></div>
            <label class="wf-check"><input type="checkbox" bind:checked={selectedNode.kind.public} onchange={scheduleValidate} disabled={running} /><span>{t("Öffentlich: darf als Suchfrage an einen externen Dienst gehen.")}</span></label>
          {:else if selectedNode.kind.type === "calendar"}
            <div class="v-field"><label class="v-label" for="wf-cd">{t("Tage voraus (1 bis 31)")}</label>
              <input id="wf-cd" type="number" min="1" max="31" bind:value={selectedNode.kind.days_ahead} oninput={scheduleValidate} disabled={running} /></div>
            <label class="wf-check"><input type="checkbox" bind:checked={selectedNode.kind.include_tasks} onchange={scheduleValidate} disabled={running} /><span>{t("Offene Aufgaben mitnehmen")}</span></label>
            <span class="v-help">{t("Privat: geht nur an das lokale Modell.")}</span>
          {:else if selectedNode.kind.type === "mail_search"}
            <div class="v-field"><label class="v-label" for="wf-mf">{t("Absender (optional)")}</label>
              <input id="wf-mf" type="text" maxlength="200" placeholder="anna@example.com" bind:value={selectedNode.kind.from} oninput={scheduleValidate} disabled={running} /></div>
            <div class="v-field"><label class="v-label" for="wf-ms">{t("Betreff enthält (optional)")}</label>
              <input id="wf-ms" type="text" maxlength="200" bind:value={selectedNode.kind.subject} oninput={scheduleValidate} disabled={running} /></div>
            <label class="wf-check"><input type="checkbox" bind:checked={selectedNode.kind.unread_only} onchange={scheduleValidate} disabled={running} /><span>{t("Nur ungelesene Mails")}</span></label>
            <div class="v-field"><label class="v-label" for="wf-ml">{t("Mails (1 bis 10)")}</label>
              <input id="wf-ml" type="number" min="1" max="10" bind:value={selectedNode.kind.limit} oninput={scheduleValidate} disabled={running} />
              <span class="v-help">{t("Braucht Gmail (Lesezugriff erlaubt, Air Gap aus). Mails bleiben privat.")}</span></div>
          {:else if selectedNode.kind.type === "memory_search"}
            <div class="v-field"><label class="v-label" for="wf-mh">{t("Treffer (1 bis 10)")}</label>
              <input id="wf-mh" type="number" min="1" max="10" bind:value={selectedNode.kind.max_hits} oninput={scheduleValidate} disabled={running} />
              <span class="v-help">{t("Sucht im Gedächtnis. Treffer bleiben privat.")}</span></div>
          {:else if selectedNode.kind.type === "skill"}
            <div class="v-field"><label class="v-label" for="wf-sk">{t("Anleitungs-Skill")}</label>
              <select id="wf-sk" bind:value={selectedNode.kind.skill_id} onchange={scheduleValidate} disabled={running}>
                <option value="">{t("wählen")}</option>
                {#each instructionSkills as skill (skill.id)}<option value={skill.id}>{skill.name}</option>{/each}
              </select>
              {#if instructionSkills.length === 0}<span data-hint class="v-help">{t("Keine Anleitung (SKILL.md) installiert. Siehe Werkzeuge.")}</span>
              {:else}<span data-hint class="v-help">{t("Das Modell folgt der Anleitung. Der Skill führt nichts aus.")}</span>{/if}</div>
          {:else if selectedNode.kind.type === "branch"}
            <div class="v-field"><label class="v-label" for="wf-br">{t("Regel")}</label>
              <select id="wf-br" value={selectedNode.kind.rule.kind} onchange={(e) => { if (selectedNode?.kind.type === "branch") { selectedNode.kind.rule = e.currentTarget.value === "contains" ? { kind: "contains", text: "" } : { kind: "model_yes_no", question: "" }; scheduleValidate(); } }} disabled={running}>
                <option value="contains">{t("Text enthält …")}</option>
                <option value="model_yes_no">{t("Das Modell urteilt Ja oder Nein")}</option>
              </select></div>
            {#if selectedNode.kind.rule.kind === "contains"}
              <div class="v-field"><label class="v-label" for="wf-bt">{t("Gesuchter Text")}</label>
                <input id="wf-bt" type="text" maxlength="200" bind:value={selectedNode.kind.rule.text} oninput={scheduleValidate} disabled={running} />
                <span data-hint class="v-help">{t("Groß-/Kleinschreibung egal. „Ja“, wenn der Text vorkommt.")}</span></div>
            {:else}
              <div class="v-field"><label class="v-label" for="wf-bq">{t("Frage an das Modell")}</label>
                <textarea id="wf-bq" rows="3" maxlength="300" bind:value={selectedNode.kind.rule.question} oninput={scheduleValidate} disabled={running}></textarea>
                <span data-hint class="v-help">{t("Eine unklare Antwort zählt als „Nein“.")}</span></div>
            {/if}
          {:else if selectedNode.kind.type === "merge"}
            <div class="v-field"><label class="v-label" for="wf-mt">{t("Vorlage")}</label>
              <textarea id="wf-mt" rows="4" bind:value={selectedNode.kind.template} oninput={scheduleValidate} disabled={running}></textarea>
              <span class="v-help">{t("{{a}}, {{b}}, {{c}} setzen die Texte der Eingänge ein.")}</span></div>
          {:else if selectedNode.kind.type === "notify"}
            <div class="v-field"><label class="v-label" for="wf-nt">{t("Titel des Hinweises")}</label>
              <input id="wf-nt" type="text" maxlength="80" bind:value={selectedNode.kind.title} oninput={scheduleValidate} disabled={running} />
              <span data-hint class="v-help">{t("Erscheint an diesem Schritt mit dem Textanfang.")}</span></div>
          {:else if selectedNode.kind.type === "store"}
            <div class="v-field"><label class="v-label" for="wf-st">{t("Ablegen in")}</label>
              <select id="wf-st" value={selectedNode.kind.target.kind} onchange={(e) => { if (selectedNode?.kind.type === "store") { const v = e.currentTarget.value; selectedNode.kind.target = v === "file" ? { kind: "file", relative_path: "" } : { kind: v as "memory" | "task" }; scheduleValidate(); } }} disabled={running}>
                <option value="memory">{t("Gedächtnis")}</option>
                <option value="task">{t("Aufgabe im Kalender")}</option>
                <option value="file">{t("Datei im Arbeitsordner")}</option>
              </select></div>
            {#if selectedNode.kind.target.kind === "file"}
              <div class="v-field"><label class="v-label" for="wf-sp">{t("Dateipfad im Arbeitsordner")}</label>
                <input id="wf-sp" type="text" maxlength="200" placeholder="notizen/idee.md" bind:value={selectedNode.kind.target.relative_path} oninput={scheduleValidate} disabled={running} /></div>
            {/if}
            <span class="v-help">{t("Der Lauf schreibt nichts. Du bestätigst einen Vorschlag.")}</span>
          {:else if selectedNode.kind.type === "wikipedia_search"}
            <div class="v-field"><label class="v-label" for="wf-wl">{t("Sprache")}</label>
              <select id="wf-wl" bind:value={selectedNode.kind.lang} onchange={scheduleValidate} disabled={running}>
                <option value="de">{t("Deutsch")}</option><option value="en">{t("Englisch")}</option>
              </select></div>
            <div class="v-field"><label class="v-label" for="wf-wn">{t("Treffer (1 bis 10)")}</label>
              <input id="wf-wn" type="number" min="1" max="10" bind:value={selectedNode.kind.num_results} oninput={scheduleValidate} disabled={running} />
              <span class="v-help">{t("Kostenlos, ohne Schlüssel. Nur die Suchfrage geht an Wikipedia. Unter Konnektoren einschalten.")}</span></div>
          {:else if selectedNode.kind.type === "brave_search"}
            <div class="v-field"><label class="v-label" for="wf-bn">{t("Treffer (1 bis 10)")}</label>
              <input id="wf-bn" type="number" min="1" max="10" bind:value={selectedNode.kind.num_results} oninput={scheduleValidate} disabled={running} />
              <span class="v-help">{t("Braucht einen Brave-Schlüssel (Konnektoren). 0,005 US-Dollar je Aufruf.")}</span></div>
          {:else if selectedNode.kind.type === "weather"}
            <div class="v-field"><label class="v-label" for="wf-wd">{t("Tage (1 bis 7)")}</label>
              <input id="wf-wd" type="number" min="1" max="7" bind:value={selectedNode.kind.days} oninput={scheduleValidate} disabled={running} />
              <span class="v-help">{t("Der Ort muss öffentlich sein. Ergebnis: Wetterbericht (Open-Meteo, kostenlos).")}</span></div>
          {:else if selectedNode.kind.type === "condition"}
            <div class="v-field"><label class="v-label" for="wf-r">{t("Wiederholungen (1 bis 5)")}</label>
              <input id="wf-r" type="number" min="1" max="5" bind:value={selectedNode.kind.max_iterations} oninput={scheduleValidate} disabled={running} />
              <span data-hint class="v-help">{t("Bei Lücken sucht der Ablauf erneut, bis zu dieser Zahl oder dem Lauf-Limit.")}</span></div>
          {/if}

          {#if outputPorts(selectedNode.kind).length > 0 && !running}
            <details class="v-details">
              <summary>{t("Verbindung anlegen (ohne Maus)")}</summary>
              <div class="wf-link">
                <div class="v-field"><label class="v-label" for="wf-lf">{t("Von Ausgang")}</label>
                  <select id="wf-lf" bind:value={linkFromPort}><option value="">{t("wählen")}</option>{#each outputPorts(selectedNode.kind) as port}<option value={port.name}>{t(port.label)}</option>{/each}</select></div>
                <div class="v-field"><label class="v-label" for="wf-lt">{t("Zu Knoten")}</label>
                  <select id="wf-lt" bind:value={linkTo} onchange={() => { linkToPort = ""; }}><option value="">{t("wählen")}</option>{#each linkTargets as node}<option value={node.id}>{t(nodeTitle(node.kind))} ({node.id.slice(0, 6)})</option>{/each}</select></div>
                <div class="v-field"><label class="v-label" for="wf-lp">{t("Zu Eingang")}</label>
                  <select id="wf-lp" bind:value={linkToPort}><option value="">{t("wählen")}</option>{#if linkTargetNode}{#each inputPorts(linkTargetNode.kind) as port}<option value={port.name}>{t(port.label)}</option>{/each}{/if}</select></div>
                <button type="button" class="v-btn v-btn-ghost" onclick={addLink}>{t("Verbinden")}</button>
              </div>
            </details>
          {/if}
          <div class="v-row v-row-end">
            {#if selectedNode.kind.type !== "manual_start"}<button type="button" class="v-btn v-btn-ghost" onclick={duplicateSelected} disabled={running} title={t("Duplizieren (Strg+D)")}>{t("Duplizieren")}</button>{/if}
            <button type="button" class="v-btn v-btn-ghost" onclick={() => canvas?.removeSelection()} disabled={running}>{t("Knoten löschen")}</button>
          </div>
        {:else if selectedEdge}
          <p class="v-help">{t("Verbindung")}: <span class="v-num">{selectedEdge.from_port} → {selectedEdge.to_port}</span></p>
          <div class="v-row v-row-end"><button type="button" class="v-btn v-btn-ghost" onclick={() => canvas?.removeSelection()} disabled={running}>{t("Verbindung löschen")}</button></div>
        {:else}
          <p data-hint class="v-help">{t("Wähle einen Knoten oder eine Verbindung. Entf löscht, Pfeiltasten verschieben.")}</p>
        {/if}
      </section>

      <section class="wf-card" aria-labelledby="wf-check">
        <h3 id="wf-check">{t("Prüfung des Ablaufs")}</h3>
        {#if problems.length > 0}
          <ul class="wf-problems">{#each problems as problem}<li>{problem.message}</li>{/each}</ul>
        {:else}
          <p class="v-help">{t("Keine Befunde.")}</p>
        {/if}
      </section>
    </div>

    {#if past.length > 0}
      <details class="v-details">
        <summary>{t("Frühere Läufe")} ({past.length})</summary>
        <ul class="v-list">
          {#each past as summary (summary.run_id)}
            <li><button type="button" class="wf-past" onclick={() => showPast(summary)} disabled={running}>{new Date(summary.started_unix_ms).toLocaleString()} · {t(statusText[summary.status])}</button></li>
          {/each}
        </ul>
      </details>
    {/if}
  {/if}
</div>

<style>
  .wf { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--v-space-4); min-width: 0; }
  .wf h2, .wf h3, .wf h4 { margin: 0; color: var(--v-text-primary); }
  .wf h2 { font-size: var(--v-text-lg); font-weight: 600; }
  .wf h3 { font-size: var(--v-text-md); font-weight: 600; }
  .wf-start-inputs { display: grid; gap: var(--v-space-3); }
  .wf-lead { margin: 0; color: var(--v-text-muted); font-size: var(--v-text-sm); line-height: 1.5; }
  .wf-intro { display: grid; gap: var(--v-space-3); }
  .wf-templates { display: grid; grid-template-columns: repeat(auto-fit, minmax(15rem, 1fr)); gap: var(--v-space-3); }
  .wf-tpl { display: grid; gap: var(--v-space-2); align-content: start; justify-items: start; padding: var(--v-space-4); border: 1px solid var(--v-line); border-radius: var(--v-radius-card); background: var(--v-surface-2); color: var(--v-text-secondary); font: inherit; text-align: left; cursor: pointer; transition: border-color 160ms ease, transform 160ms ease; }
  .wf-tpl strong { color: var(--v-text-primary); font-size: var(--v-text-md); }
  .wf-tpl span:not(.v-chip) { font-size: var(--v-text-sm); line-height: 1.45; }
  .wf-tpl:hover { border-color: var(--v-accent-blue); }
  .wf-tpl:active { transform: scale(.985); }
  .wf-tpl:focus-visible { outline: none; box-shadow: var(--v-shadow-focus); }
  .wf-saved { display: grid; gap: var(--v-space-3); }
  .wf-saved-list { display: grid; grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr)); gap: var(--v-space-2); margin: 0; padding: 0; list-style: none; }
  .wf-saved-item { display: grid; gap: 2px; width: 100%; padding: var(--v-space-3) var(--v-space-4); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); background: transparent; color: var(--v-text-primary); font: inherit; text-align: left; cursor: pointer; }
  .wf-saved-item:hover { border-color: var(--v-line-strong); background: rgb(var(--v-tint) / .05); }
  .wf-saved-item:focus-visible { outline: none; box-shadow: var(--v-shadow-focus); }
  .wf-saved-item .v-help { margin: 0; }
  .wf-bar { display: flex; flex-wrap: wrap; align-items: end; gap: var(--v-space-3); }
  .wf-name { flex: 1 1 12rem; min-width: 0; display: grid; gap: 4px; }
  .wf-actions { margin-left: auto; }
  .wf-banner { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--v-space-3); }
  .wf-card { display: grid; gap: var(--v-space-3); align-content: start; min-width: 0; padding: var(--v-space-4) var(--v-space-5); border: 1px solid var(--v-line); border-radius: var(--v-radius-card); background: var(--v-surface-2); }
  .wf-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(18rem, 1fr)); gap: var(--v-space-3); }
  .wf-budget { display: grid; grid-template-columns: repeat(auto-fit, minmax(11rem, 1fr)); gap: var(--v-space-3); }
  .wf-link { display: grid; gap: var(--v-space-2); }
  .wf-problems { margin: 0; padding-left: 1.1rem; color: var(--v-danger); font-size: var(--v-text-sm); line-height: 1.5; }
  .wf-check { display: flex; gap: var(--v-space-2); align-items: flex-start; color: var(--v-text-secondary); font-size: var(--v-text-sm); line-height: 1.45; }
  .wf-check input { flex: none; width: auto; margin-top: .2rem; }
  .wf-run-head { display: flex; flex-wrap: wrap; align-items: center; gap: var(--v-space-3); }
  .wf-run-cancel { margin-left: auto; }
  .wf-spin { display: inline-block; width: .7rem; height: .7rem; margin-right: 6px; border: 2px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: wf-rot 900ms linear infinite; }
  @keyframes wf-rot { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .wf-spin { animation: none; } }
  .wf-usage { display: grid; grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr)); gap: var(--v-space-3); margin: 0; }
  .wf-usage > div { display: grid; gap: 2px; }
  .wf-usage dt { color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .wf-usage dd { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-sm); }
  .wf-usage i { display: block; height: 3px; border-radius: 3px; background: linear-gradient(to right, var(--v-accent-blue) var(--p), var(--v-line) var(--p)); }
  .wf-result h4 { margin: var(--v-space-3) 0 var(--v-space-1); font-size: var(--v-text-md); }
  .wf-answer { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; color: var(--v-text-primary); line-height: 1.55; }
  .wf-log { display: grid; gap: 2px; margin: 0; padding: 0; list-style: none; max-height: 18rem; overflow: auto; font-size: var(--v-text-xs); }
  .wf-log li { display: flex; flex-wrap: wrap; gap: var(--v-space-2); align-items: baseline; }
  .wf-log li.error, .wf-log li.denied { color: var(--v-danger); }
  .wf-log li.data_flow { color: var(--v-text-secondary); }
  .wf-past { all: unset; box-sizing: border-box; width: 100%; padding: var(--v-space-2) var(--v-space-3); border-radius: var(--v-radius-control); color: var(--v-text-secondary); font-size: var(--v-text-sm); cursor: pointer; }
  .wf-past:hover:not(:disabled) { background: rgb(var(--v-tint) / .08); color: var(--v-text-primary); }
  .wf-past:focus-visible { box-shadow: var(--v-shadow-focus); }
</style>
