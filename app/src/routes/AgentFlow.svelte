<script lang="ts">
  import { onMount } from "svelte";
  import { isTauri } from "@tauri-apps/api/core";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { t, tk } from "../lib/i18n/index.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import {
    agentFlowAdopt, agentFlowAdoptPreview, agentFlowCancel, agentFlowCleanup, agentFlowDiff, agentFlowLimits,
    agentFlowList, agentFlowPreflight, agentFlowProjects, agentFlowRevokeProject, agentFlowStart, installedModels,
    onAgentFlowChanged, onAgentFlowProgress, pickFolderDialog, type AgentFlowProgress,
  } from "../lib/ipc";
  import type {
    AdoptPreview, AdoptResult, AgentFlowTask, ApprovedProject, AvailableModel, Candidate, CandidateStatus, FlowLimits,
    ProjectPreflight,
  } from "../lib/types";

  // Agent Flow: eine Aufgabe, mehrere Kandidaten. Jeder Kandidat arbeitet in einem
  // eigenen Git-Worktree neben dem Projekt. Das Modell schreibt nie selbst; die
  // Übernahme ins Projekt braucht eine zweite, ausdrückliche Bestätigung nach der
  // vollständigen Diff-Vorschau.
  interface Props {
    /** Nur der sichtbare Bereich reagiert auf abgelegte Ordner. */
    active: boolean;
  }
  let { active }: Props = $props();

  let limits = $state<FlowLimits | null>(null);
  let models = $state<AvailableModel[]>([]);
  let approved = $state<ApprovedProject[]>([]);
  let tasks = $state<AgentFlowTask[]>([]);
  let progress = $state<Record<string, AgentFlowProgress>>({});

  // Schritt 1: Projekt
  let projectPath = $state("");
  let preflight = $state<ProjectPreflight | null>(null);
  let preflightBusy = $state(false);
  let preflightError = $state<string | null>(null);
  let needsApproval = $state(false);
  let editingProject = $state(true);
  let dragOver = $state(false);
  let baseKind = $state<"head" | "snapshot">("head");
  let untrackedChosen = $state<Record<string, boolean>>({});

  // Schritt 2: Aufgabe
  let prompt = $state("");
  let contextText = $state("");
  let count = $state(2);
  let candidateModels = $state<string[]>([]);
  let maxTokens = $state(2048);
  let maxSeconds = $state(300);
  let starting = $state(false);
  let startError = $state<string | null>(null);

  // Schritt 3: Ergebnisse
  let diffs = $state<Record<string, string>>({});
  let diffBusy = $state<Record<string, boolean>>({});
  let adopting = $state<{ taskId: string; candidateId: string; preview: AdoptPreview; checked: boolean; busy: boolean } | null>(null);
  let adoptResult = $state<{ candidateId: string; result: AdoptResult } | null>(null);
  let adoptError = $state<string | null>(null);
  let cleanupFor = $state<string | null>(null);
  let taskError = $state<string | null>(null);
  let now = $state(Date.now());

  const dirtyUntracked = $derived(preflight?.dirty_files.filter((f) => f.state === "untracked") ?? []);
  const dirtyTracked = $derived(preflight?.dirty_files.filter((f) => f.state !== "untracked" && f.state !== "ignored") ?? []);
  const ready = $derived(!!preflight && preflight.blockers.length === 0);
  const startHint = $derived(
    !preflight ? tk("Prüfe zuerst das Projekt.") : preflight.blockers.length > 0 ? tk("Das Projekt lässt sich so nicht nutzen.") : !prompt.trim() ? tk("Beschreibe die Aufgabe.") : candidateModels.length === 0 ? tk("Es ist kein Modell installiert.") : null,
  );
  const worstCaseMinutes = $derived(Math.ceil((count * maxSeconds) / 60));
  const anyRunning = $derived(tasks.some(running));

  function flowError(reason: unknown): string {
    return String(reason ?? "").replace(/^(Ungültige Eingabe|Internal|Core-Fehler|Vault-Fehler|Launcher-Fehler): /, "");
  }

  function upsertTask(task: AgentFlowTask) {
    const index = tasks.findIndex((entry) => entry.id === task.id);
    if (index >= 0) tasks[index] = task;
    else tasks = [task, ...tasks];
  }

  onMount(() => {
    const stops: UnlistenFn[] = [];
    let alive = true;
    const tick = setInterval(() => { now = Date.now(); }, 1000);
    (async () => {
      try {
        limits = await agentFlowLimits();
        count = limits.default_candidates;
        maxSeconds = limits.weak_hardware ? 900 : 300;
        models = await installedModels();
        approved = await agentFlowProjects();
        tasks = await agentFlowList();
        const handles = [
          await onAgentFlowChanged(upsertTask),
          await onAgentFlowProgress((p) => { progress[p.candidate_id] = p; }),
        ];
        if (isTauri()) {
          handles.push(await getCurrentWebview().onDragDropEvent((event) => {
            if (!active) return;
            if (event.payload.type === "enter" || event.payload.type === "over") dragOver = true;
            else if (event.payload.type === "leave") dragOver = false;
            else if (event.payload.type === "drop") {
              dragOver = false;
              const path = event.payload.paths[0];
              if (path) { projectPath = path; void runPreflight(false); }
            }
          }));
        }
        if (alive) stops.push(...handles);
        else handles.forEach((stop) => stop());
      } catch (reason) {
        startError = flowError(reason);
      }
    })();
    return () => { alive = false; clearInterval(tick); stops.forEach((stop) => stop()); };
  });

  // Ein Modell je Kandidat; neue Plätze übernehmen das letzte Modell.
  $effect(() => {
    const first = models.find((m) => m.is_default)?.id ?? models[0]?.id;
    if (!first) return;
    const next = candidateModels.slice(0, count);
    while (next.length < count) next.push(next[next.length - 1] ?? first);
    if (next.length !== candidateModels.length || next.some((id, i) => id !== candidateModels[i])) candidateModels = next;
  });

  async function browse() {
    try {
      const chosen = await pickFolderDialog(t("Projektordner wählen"));
      if (chosen) { projectPath = chosen; await runPreflight(false); }
    } catch (reason) {
      preflightError = flowError(reason);
    }
  }

  async function runPreflight(grant: boolean) {
    if (!projectPath.trim() || preflightBusy) return;
    preflightBusy = true;
    preflightError = null;
    needsApproval = false;
    try {
      preflight = await agentFlowPreflight(projectPath.trim(), grant);
      projectPath = preflight.repo_root;
      untrackedChosen = {};
      baseKind = "head";
      editingProject = false;
      approved = await agentFlowProjects();
    } catch (reason) {
      preflight = null;
      const text = flowError(reason);
      if (text.startsWith("Freigabe nötig")) needsApproval = true;
      else preflightError = text;
    } finally {
      preflightBusy = false;
    }
  }

  async function revoke(path: string) {
    try { await agentFlowRevokeProject(path); approved = await agentFlowProjects(); }
    catch (reason) { preflightError = flowError(reason); }
  }

  async function start() {
    if (startHint || starting || !preflight) return;
    starting = true;
    startError = null;
    try {
      const chosen = dirtyUntracked.filter((f) => untrackedChosen[f.path]).map((f) => f.path);
      const task = await agentFlowStart({
        project_path: preflight.repo_root,
        host_project_approved: approved.some((p) => p.path === preflight?.repo_root),
        prompt: prompt.trim(),
        base: baseKind === "head" ? { kind: "head_commit" } : { kind: "snapshot", include_untracked: chosen },
        candidates: candidateModels.map((model_id) => ({ model_id, max_tokens: maxTokens, max_seconds: maxSeconds })),
        context_files: contextText.split(/[\n,]/).map((s) => s.trim()).filter(Boolean),
      });
      upsertTask(task);
      prompt = "";
    } catch (reason) {
      startError = flowError(reason);
    } finally {
      starting = false;
    }
  }

  function running(task: AgentFlowTask): boolean {
    return task.candidates.some((c) => ["queued", "loading", "generating", "applying"].includes(c.status.kind));
  }

  function isActive(status: CandidateStatus): boolean {
    return ["loading", "generating", "applying"].includes(status.kind);
  }

  function statusText(status: CandidateStatus): string {
    switch (status.kind) {
      case "queued": return t("Wartet (Platz {n})", { n: status.position });
      case "loading": return t("Modell wird geladen");
      case "generating": return t("Modell arbeitet");
      case "applying": return t("Änderung wird geprüft");
      case "ready": return t("Fertig");
      case "failed": return t("Fehlgeschlagen");
      case "cancelled": return t("Abgebrochen");
    }
  }

  function chipClass(status: CandidateStatus): string {
    if (status.kind === "ready") return "v-chip accent";
    if (status.kind === "failed") return "v-chip danger";
    if (status.kind === "cancelled") return "v-chip warn";
    return "v-chip";
  }

  const modelName = (id: string) => models.find((m) => m.id === id)?.display_name ?? id;
  const mb = (bytes: number) => Math.round(bytes / (1024 * 1024));
  const short = (hash: string | null | undefined) => (hash ? hash.slice(0, 8) : "-");
  const folderName = (path: string) => path.split(/[\\/]/).filter(Boolean).pop() ?? path;
  const readyCount = (task: AgentFlowTask) => task.candidates.filter((c) => c.status.kind === "ready").length;

  /** Laufende Werte eines Kandidaten: Fortschrittsereignisse, sonst gespeicherte Angaben. */
  function live(candidate: Candidate): { tokens: number; seconds: number } {
    const p = progress[candidate.id];
    return isActive(candidate.status) && p ? { tokens: p.tokens, seconds: p.seconds } : { tokens: candidate.usage.tokens, seconds: candidate.usage.seconds };
  }

  async function showDiff(task: AgentFlowTask, candidate: Candidate) {
    if (diffs[candidate.id] !== undefined) {
      const { [candidate.id]: _removed, ...rest } = diffs;
      diffs = rest;
      return;
    }
    diffBusy[candidate.id] = true;
    taskError = null;
    try { diffs[candidate.id] = await agentFlowDiff(task.id, candidate.id); }
    catch (reason) { taskError = flowError(reason); }
    finally { diffBusy[candidate.id] = false; }
  }

  async function openAdopt(task: AgentFlowTask, candidate: Candidate) {
    adoptError = null;
    adoptResult = null;
    try {
      const preview = await agentFlowAdoptPreview(task.id, candidate.id);
      adopting = { taskId: task.id, candidateId: candidate.id, preview, checked: false, busy: false };
    } catch (reason) { adoptError = flowError(reason); }
  }

  async function confirmAdopt() {
    if (!adopting || !adopting.checked || adopting.busy || adopting.preview.conflicts.length > 0) return;
    adopting.busy = true;
    adoptError = null;
    try {
      const result = await agentFlowAdopt(adopting.taskId, adopting.candidateId, true);
      adoptResult = { candidateId: adopting.candidateId, result };
      adopting = null;
    } catch (reason) {
      adoptError = flowError(reason);
      if (adopting) adopting.busy = false;
    }
  }

  async function cancel(task: AgentFlowTask) {
    try { await agentFlowCancel(task.id); } catch (reason) { taskError = flowError(reason); }
  }

  async function cleanup(task: AgentFlowTask) {
    taskError = null;
    try {
      await agentFlowCleanup(task.id, true);
      tasks = tasks.filter((entry) => entry.id !== task.id);
      cleanupFor = null;
    } catch (reason) { taskError = flowError(reason); }
  }

  function diffLines(text: string): { cls: string; text: string }[] {
    return text.split("\n").map((line) => ({
      cls: line.startsWith("+++") || line.startsWith("---") || line.startsWith("diff ") || line.startsWith("index ")
        ? "meta" : line.startsWith("@@") ? "hunk" : line.startsWith("+") ? "add" : line.startsWith("-") ? "del" : "",
      text: line,
    }));
  }
</script>

<div class="af">
  <!-- Schritt 1: Projekt -->
  <section class="af-step" class:done={ready && !editingProject} aria-labelledby="af-s1">
    <header class="af-step-head">
      <span class="af-badge" aria-hidden="true">{ready && !editingProject ? "✓" : "1"}</span>
      <h2 id="af-s1">{t("Projekt")}</h2>
      {#if ready && !editingProject}<button type="button" class="v-btn v-btn-ghost af-small" onclick={() => { editingProject = true; }}>{t("Ändern")}</button>{/if}
    </header>

    {#if ready && !editingProject && preflight}
      <p class="af-summary">
        <strong>{folderName(preflight.repo_root)}</strong>
        <span class="v-chip">{preflight.location === "stick" ? t("USB-Stick") : t("Dieser Computer")}</span>
        <span class="v-num">{preflight.branch ?? t("abgelöst")} · {short(preflight.head)}</span>
        <span class="v-help">{preflight.filesystem} · {t("{n} MB frei", { n: mb(preflight.free_bytes) })}</span>
      </p>
    {:else}
      <p class="af-lead">{t("Wähle ein Git-Projekt. Die Kandidaten arbeiten in eigenen Kopien (Worktrees); dein Ordner bleibt unberührt, bis du übernimmst.")}</p>
      <div class="af-drop" class:over={dragOver}>
        <div class="v-row af-path-row">
          <input id="af-path" class="af-path" type="text" bind:value={projectPath} placeholder="D:\projekte\meine-app" spellcheck="false" aria-label={t("Projektordner")}
            onkeydown={(e) => { if (e.key === "Enter") runPreflight(false); }} />
          <button type="button" class="v-btn v-btn-ghost" onclick={browse} disabled={preflightBusy}>{t("Ordner wählen …")}</button>
          <button type="button" class="v-btn v-btn-primary" onclick={() => runPreflight(false)} disabled={preflightBusy || !projectPath.trim()}>{preflightBusy ? t("Prüfe …") : t("Prüfen")}</button>
        </div>
        <p class="v-help">{dragOver ? t("Jetzt loslassen") : t("Oder einen Ordner hierher ziehen.")}</p>
      </div>
      {#if needsApproval}
        <div class="v-notice af-approval" role="alert">
          <strong>{t("Dieses Projekt liegt auf dem Computer, nicht auf dem Stick.")}</strong>
          <span>{t("Gib den Ordner für Agent Flow frei. Die Freigabe liegt im Tresor und ist widerrufbar.")}</span>
          <div class="v-row">
            <button type="button" class="v-btn v-btn-primary" onclick={() => runPreflight(true)}>{t("Freigeben und prüfen")}</button>
            <button type="button" class="v-btn v-btn-ghost" onclick={() => { needsApproval = false; }}>{t("Abbrechen")}</button>
          </div>
        </div>
      {/if}
      {#if preflightError}<p class="v-notice warn" role="alert">{preflightError}</p>{/if}
      {#if approved.length > 0}
        <details class="v-details">
          <summary>{t("Freigegebene Projekte")} ({approved.length})</summary>
          <ul class="v-list">
            {#each approved as project (project.path)}
              <li>
                <button type="button" class="af-link" onclick={() => { projectPath = project.path; void runPreflight(false); }}>{project.path}</button>
                <button type="button" class="v-btn v-btn-ghost af-small" onclick={() => revoke(project.path)}>{t("Freigabe widerrufen")}</button>
              </li>
            {/each}
          </ul>
        </details>
      {/if}
    {/if}

    {#if preflight && (!editingProject || preflight.blockers.length > 0)}
      {#each preflight.blockers as blocker}<p class="v-notice warn" role="alert"><strong>{t("Nicht möglich:")}</strong> {blocker}</p>{/each}
      {#each preflight.warnings as warning}<p class="v-notice">{warning}</p>{/each}
    {/if}

    {#if ready && !editingProject && preflight}
      <div class="af-base" role="group" aria-label={t("Startbasis der Kandidaten")}>
        <span class="v-label">{t("Startbasis")}</span>
        <div class="v-segmented">
          <button type="button" class:active={baseKind === "head"} aria-pressed={baseKind === "head"} onclick={() => { baseKind = "head"; }}>{t("Letzter Commit (HEAD)")}</button>
          <button type="button" class:active={baseKind === "snapshot"} aria-pressed={baseKind === "snapshot"} disabled={preflight.dirty_files.length === 0}
            onclick={() => { baseKind = "snapshot"; }}>{t("Meine aktuellen Änderungen")}{#if preflight.dirty_files.length > 0} ({preflight.dirty_files.length}){/if}</button>
        </div>
        {#if baseKind === "snapshot"}
          <p data-hint class="v-help">{t("Eigener Index: dein Arbeitsordner bleibt unverändert, nichts wird versteckt. Ignorierte Dateien kommen nie mit.")}</p>
          {#if dirtyTracked.length > 0}<p class="v-help">{t("Diese Änderungen an versionierten Dateien sind dabei:")} <span class="v-num">{dirtyTracked.map((f) => f.path).join(", ")}</span></p>{/if}
          {#if dirtyUntracked.length > 0}
            <p class="v-label">{t("Neue Dateien (nur angehakte kommen mit):")}</p>
            <ul class="af-untracked">
              {#each dirtyUntracked as file (file.path)}
                <li><label class="af-check"><input type="checkbox" bind:checked={untrackedChosen[file.path]} /><span class="v-num">{file.path}</span>{#if file.sensitive}<span class="v-chip warn">{t("sensibel")}</span>{/if}</label></li>
              {/each}
            </ul>
          {/if}
        {/if}
        <p class="v-help">{t("Worktrees liegen hier")}: <span class="v-num">{preflight.worktrees_root}</span></p>
      </div>
    {/if}
  </section>

  <!-- Schritt 2: Aufgabe -->
  <section class="af-step" class:locked={!ready} aria-labelledby="af-s2">
    <header class="af-step-head"><span class="af-badge" aria-hidden="true">2</span><h2 id="af-s2">{t("Aufgabe")}</h2></header>
    <div class="v-field">
      <label class="v-label" for="af-prompt">{t("Was soll geändert werden?")}</label>
      <textarea id="af-prompt" rows="3" bind:value={prompt} disabled={!ready} placeholder={t("Beschreibe, was geändert werden soll.")}></textarea>
    </div>

    <div class="af-cands">
      <div class="af-count">
        <span class="v-label" id="af-count-label">{t("Kandidaten")}</span>
        <div class="af-stepper" role="group" aria-labelledby="af-count-label">
          <button type="button" class="v-btn v-btn-ghost" aria-label={t("Weniger")} onclick={() => { count = Math.max(1, count - 1); }} disabled={!ready || count <= 1}>−</button>
          <output class="v-num" aria-live="polite">{count}</output>
          <button type="button" class="v-btn v-btn-ghost" aria-label={t("Mehr")} onclick={() => { count = Math.min(limits?.max_candidates ?? 5, count + 1); }} disabled={!ready || count >= (limits?.max_candidates ?? 5)}>+</button>
        </div>
      </div>
      <ol class="af-model-list">
        {#each candidateModels as _id, index}
          <li>
            <label class="af-model-row" for={`af-model-${index}`}>
              <span>{t("Kandidat {n}", { n: index + 1 })}</span>
              <select id={`af-model-${index}`} bind:value={candidateModels[index]} disabled={!ready}>
                {#each models as model (model.id)}<option value={model.id}>{model.display_name}</option>{/each}
              </select>
            </label>
          </li>
        {/each}
      </ol>
    </div>
    <p class="v-help">{t("Die Kandidaten laufen nacheinander, höchstens etwa {n} Minuten.", { n: worstCaseMinutes })}{#if limits?.weak_hardware}{" "}{t("Auf diesem Rechner starten zwei Kandidaten.")}{/if}</p>

    <details class="v-details">
      <summary>{t("Erweitert")}</summary>
      <div class="af-adv">
        <div class="v-field">
          <label class="v-label" for="af-context">{t("Kontextdateien (optional)")}</label>
          <textarea id="af-context" rows="2" bind:value={contextText} disabled={!ready} spellcheck="false"></textarea>
          <span data-hint class="v-help">{t("Relative Pfade, getrennt durch Komma oder Zeilenumbruch. Ohne Angabe wählt IAP passende Dateien. Sensible Dateien wie .env gehen nie mit.")}</span>
        </div>
        <div class="v-field"><label class="v-label" for="af-tokens">{t("Token je Kandidat (höchstens)")}</label><input id="af-tokens" type="number" min="256" step="256" bind:value={maxTokens} disabled={!ready} /></div>
        <div class="v-field"><label class="v-label" for="af-seconds">{t("Sekunden je Kandidat (höchstens)")}</label><input id="af-seconds" type="number" min="30" step="30" bind:value={maxSeconds} disabled={!ready} /></div>
      </div>
    </details>

    <p class="v-help af-tests">{t("Projekt-Tests laufen hier nicht (keine Sandbox) und erscheinen als „nicht ausgeführt“.")}</p>
    {#if startError}<p class="v-notice warn" role="alert">{startError}</p>{/if}
    <div class="af-start">
      {#if startHint}<span class="v-help">{t(startHint)}</span>{/if}
      <button type="button" class="v-btn v-btn-primary" onclick={start} disabled={!!startHint || starting}>{starting ? t("Starte …") : t("Kandidaten starten")}</button>
    </div>
  </section>

  <!-- Schritt 3: Ergebnisse -->
  <section class="af-results" aria-labelledby="af-s3">
    <header class="af-step-head"><span class="af-badge" aria-hidden="true">3</span><h2 id="af-s3">{t("Ergebnisse")}</h2></header>
    {#if taskError}<p class="v-notice warn" role="alert">{taskError}</p>{/if}
    {#if tasks.length === 0}
      <EmptyState title="Noch keine Aufgaben" text="Gestartete Aufgaben erscheinen hier." />
    {/if}
    {#each tasks as task (task.id)}
      <article class="af-task">
        <header class="af-task-head">
          <div class="af-task-title">
            <h3>{task.prompt.split("\n")[0]}</h3>
            <p class="v-help">{folderName(task.project_path)} · {t("Basis")} <span class="v-num">{short(task.base.base_commit)}</span> ({task.base.choice.kind === "head_commit" ? t("letzter Commit") : t("Momentaufnahme")}) · {t("{a} von {b} fertig", { a: readyCount(task), b: task.candidates.length })}</p>
          </div>
          <div class="v-row">
            {#if running(task)}
              <button type="button" class="v-btn v-btn-ghost" onclick={() => cancel(task)}>{t("Abbrechen")}</button>
            {:else if cleanupFor === task.id}
              <span class="v-help">{t("Worktrees und Branches dieser Aufgabe werden entfernt. Die Ergebnisse sind danach weg.")}</span>
              <button type="button" class="v-btn v-btn-primary" onclick={() => cleanup(task)}>{t("Ja, aufräumen")}</button>
              <button type="button" class="v-btn v-btn-ghost" onclick={() => { cleanupFor = null; }}>{t("Behalten")}</button>
            {:else}
              <button type="button" class="v-btn v-btn-ghost" onclick={() => { cleanupFor = task.id; }}>{t("Aufräumen …")}</button>
            {/if}
          </div>
        </header>

        <div class="af-grid">
          {#each task.candidates as candidate (candidate.id)}
            {@const figures = live(candidate)}
            <div class="af-cand" class:ready={candidate.status.kind === "ready"} class:busy={isActive(candidate.status)}>
              <div class="af-cand-head">
                <strong>{t("Kandidat {n}", { n: candidate.index })}</strong>
                <span class={chipClass(candidate.status)} role="status">
                  {#if isActive(candidate.status)}<span class="af-spin" aria-hidden="true"></span>{/if}{statusText(candidate.status)}
                </span>
              </div>
              <p class="v-help">{modelName(candidate.model_id)}</p>
              {#if isActive(candidate.status)}<div class="af-bar" role="progressbar" aria-label={t("Läuft")}><span></span></div>{/if}
              {#if candidate.status.kind === "failed"}<p class="v-notice warn">{candidate.status.reason}</p>{/if}
              {#if candidate.status.kind !== "queued"}
                <dl class="af-usage">
                  <div><dt>{t("Token")}</dt><dd class="v-num">{isActive(candidate.status) ? "≈ " : ""}{figures.tokens}</dd></div>
                  <div><dt>{t("Dauer")}</dt><dd class="v-num">{figures.seconds.toFixed(1)} s</dd></div>
                  <div><dt>{t("Speicher (Spitze)")}</dt><dd class="v-num">{candidate.usage.peak_rss_bytes ? `${mb(candidate.usage.peak_rss_bytes)} MB` : "–"}</dd></div>
                </dl>
              {/if}
              {#if candidate.changed_files.length > 0}
                <ul class="af-files">{#each candidate.changed_files as file}<li class="v-num">{file}</li>{/each}</ul>
              {/if}
              {#if candidate.status.kind === "ready"}
                <p class="v-help">{t("Tests:")} {candidate.tests.kind === "not_run" ? t("nicht ausgeführt") : candidate.tests.kind === "passed" ? t("bestanden") : t("fehlgeschlagen")}</p>
                <div class="v-row">
                  <button type="button" class="v-btn v-btn-ghost" onclick={() => showDiff(task, candidate)} disabled={diffBusy[candidate.id]}>{diffs[candidate.id] !== undefined ? t("Diff ausblenden") : t("Diff ansehen")}</button>
                  <button type="button" class="v-btn v-btn-primary" onclick={() => openAdopt(task, candidate)}>{t("Übernehmen …")}</button>
                </div>
              {/if}
              <details class="v-details af-where"><summary>{t("Ort des Worktrees")}</summary><span class="v-num">{candidate.worktree_path}</span></details>

              {#if diffs[candidate.id] !== undefined}
                <pre class="af-diff" aria-label={t("Vollständiger Diff")}>{#each diffLines(diffs[candidate.id]) as line}<span class={line.cls}>{line.text}
</span>{/each}</pre>
              {/if}

              {#if adopting && adopting.candidateId === candidate.id}
                <div class="af-adopt" role="group" aria-label={t("Übernahme bestätigen")}>
                  <h4>{t("Übernahme prüfen")}</h4>
                  {#if adopting.preview.conflicts.length > 0}
                    <div class="v-notice warn" role="alert">
                      <strong>{t("Übernahme gestoppt. Es wird nichts geschrieben.")}</strong>
                      <ul>{#each adopting.preview.conflicts as conflict}<li><span class="v-num">{conflict.path}</span>: {conflict.reason}</li>{/each}</ul>
                      <p class="v-help">{t("Der Kandidat bleibt erhalten. Sichere deine Änderungen und starte die Aufgabe neu.")}</p>
                    </div>
                  {/if}
                  <p class="v-help">{t("Diese Dateien werden im Projekt geschrieben:")} <span class="v-num">{adopting.preview.files.join(", ")}</span></p>
                  <pre class="af-diff" aria-label={t("Vollständiger Diff")}>{#each diffLines(adopting.preview.diff) as line}<span class={line.cls}>{line.text}
</span>{/each}</pre>
                  {#if adopting.preview.conflicts.length === 0}
                    <label class="af-check"><input type="checkbox" bind:checked={adopting.checked} /><span>{t("Ich habe den Diff geprüft und will diese Änderungen schreiben. Nichts wird committet oder hochgeladen.")}</span></label>
                  {/if}
                  {#if adoptError}<p class="v-notice warn" role="alert">{adoptError}</p>{/if}
                  <div class="v-row v-row-end">
                    <button type="button" class="v-btn v-btn-ghost" onclick={() => { adopting = null; adoptError = null; }}>{t("Abbrechen")}</button>
                    <button type="button" class="v-btn v-btn-primary" onclick={confirmAdopt} disabled={!adopting.checked || adopting.busy || adopting.preview.conflicts.length > 0}>{adopting.busy ? t("Schreibe …") : t("Jetzt ins Projekt übernehmen")}</button>
                  </div>
                </div>
              {/if}
              {#if adoptError && !adopting && adoptResult === null}<p class="v-notice warn" role="alert">{adoptError}</p>{/if}
              {#if adoptResult && adoptResult.candidateId === candidate.id}
                <p class="v-notice" role="status">{t("Übernommen:")} <span class="v-num">{adoptResult.result.written_files.join(", ")}</span>. {adoptResult.result.note}</p>
              {/if}
            </div>
          {/each}
        </div>
      </article>
    {/each}
  </section>
</div>

<style>
  .af { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--v-space-4); }
  .af-step, .af-results { display: grid; gap: var(--v-space-3); align-content: start; padding: var(--v-space-4) var(--v-space-5); border: 1px solid var(--v-line); border-radius: var(--v-radius-card); background: var(--v-surface-2); min-width: 0; }
  .af-results { background: transparent; border-style: dashed; }
  .af-step.locked { opacity: .6; }
  .af-step-head { display: flex; align-items: center; gap: var(--v-space-3); }
  .af-step-head h2 { margin: 0; margin-right: auto; color: var(--v-text-primary); font-size: var(--v-text-lg); font-weight: 600; }
  .af-badge { display: inline-grid; place-items: center; width: 1.75rem; height: 1.75rem; border-radius: 50%; border: 1px solid var(--v-line-strong); color: var(--v-text-secondary); font-size: var(--v-text-sm); font-weight: 600; }
  .af-step.done .af-badge { border-color: var(--v-accent-blue); background: var(--v-accent-blue-soft); color: var(--v-text-primary); }
  .af-small { min-height: 2rem; padding-inline: .75rem; font-size: var(--v-text-sm); }
  .af-lead { margin: 0; color: var(--v-text-muted); font-size: var(--v-text-sm); line-height: 1.5; }
  .af-drop { display: grid; gap: var(--v-space-2); padding: var(--v-space-3); border: 1px dashed var(--v-line-strong); border-radius: var(--v-radius-field); transition: border-color 160ms ease, background-color 160ms ease; }
  .af-drop.over { border-style: solid; border-color: var(--v-accent-blue); background: var(--v-accent-blue-soft); }
  .af-drop .v-help { margin: 0; }
  .af-path-row { flex-wrap: wrap; }
  .af-path { flex: 1 1 14rem; min-width: 0; }
  .af-summary { display: flex; flex-wrap: wrap; align-items: center; gap: var(--v-space-2) var(--v-space-3); margin: 0; color: var(--v-text-primary); font-size: var(--v-text-sm); }
  .af-approval { display: grid; gap: var(--v-space-2); justify-items: start; }
  .af-link { all: unset; flex: 1; min-width: 0; overflow-wrap: anywhere; cursor: pointer; color: var(--v-text-primary); font-family: var(--v-font-mono); font-size: var(--v-text-xs); }
  .af-link:focus-visible { box-shadow: var(--v-shadow-focus); border-radius: var(--v-radius-control); }
  .af-base { display: grid; gap: var(--v-space-2); }
  .af-base .v-help { margin: 0; overflow-wrap: anywhere; }
  .af-untracked { display: grid; gap: var(--v-space-1); margin: 0; padding: 0; list-style: none; }
  .af-check { display: flex; gap: var(--v-space-2); align-items: flex-start; color: var(--v-text-secondary); font-size: var(--v-text-sm); line-height: 1.45; }
  .af-check input { flex: none; width: auto; margin-top: .2rem; }
  .af-cands { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--v-space-4); align-items: start; }
  .af-count { display: grid; gap: var(--v-space-1); }
  .af-stepper { display: inline-flex; align-items: center; gap: var(--v-space-2); }
  .af-stepper .v-btn { min-width: 2.25rem; padding-inline: 0; }
  .af-stepper output { min-width: 1.5rem; text-align: center; color: var(--v-text-primary); font-size: var(--v-text-lg); }
  .af-model-list { display: grid; gap: var(--v-space-2); margin: 0; padding: 0; list-style: none; }
  .af-model-row { display: grid; grid-template-columns: 6.5rem minmax(0, 1fr); align-items: center; gap: var(--v-space-3); color: var(--v-text-secondary); font-size: var(--v-text-sm); }
  .af-adv { display: grid; gap: var(--v-space-3); grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .af-adv > :first-child { grid-column: 1 / -1; }
  .af-tests { margin: 0; }
  .af-start { display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: var(--v-space-3); }
  .af-task { display: grid; gap: var(--v-space-3); padding: var(--v-space-4); border: 1px solid var(--v-line); border-radius: var(--v-radius-card); background: var(--v-surface-2); }
  .af-task-head { display: flex; flex-wrap: wrap; justify-content: space-between; gap: var(--v-space-3); }
  .af-task-title { min-width: 0; flex: 1 1 16rem; }
  .af-task-title h3 { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-md); font-weight: 600; overflow-wrap: anywhere; }
  .af-task-title .v-help { margin: 2px 0 0; overflow-wrap: anywhere; }
  .af-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(17rem, 1fr)); gap: var(--v-space-3); }
  .af-cand { display: grid; gap: var(--v-space-2); align-content: start; min-width: 0; padding: var(--v-space-3); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); }
  .af-cand.ready { border-color: color-mix(in oklab, var(--v-accent-blue) 45%, transparent); }
  .af-cand.busy { border-color: var(--v-accent-blue); }
  .af-cand-head { display: flex; align-items: center; justify-content: space-between; gap: var(--v-space-2); color: var(--v-text-primary); }
  .af-cand .v-help { margin: 0; overflow-wrap: anywhere; }
  .af-usage { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: var(--v-space-2); margin: 0; }
  .af-usage dt { color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .af-usage dd { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-sm); }
  .af-files { display: flex; flex-wrap: wrap; gap: var(--v-space-1); margin: 0; padding: 0; list-style: none; }
  .af-files li { padding: 1px 8px; border: 1px solid var(--v-line); border-radius: 9999px; color: var(--v-text-secondary); font-size: var(--v-text-xs); overflow-wrap: anywhere; }
  .af-where { font-size: var(--v-text-xs); }
  .af-where span { display: block; margin-top: var(--v-space-1); overflow-wrap: anywhere; color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .af-spin { display: inline-block; width: .7rem; height: .7rem; margin-right: 6px; border: 2px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: af-rot 900ms linear infinite; }
  .af-bar { height: 3px; overflow: hidden; border-radius: 3px; background: var(--v-line); }
  .af-bar span { display: block; width: 40%; height: 100%; border-radius: 3px; background: var(--v-accent-blue); animation: af-slide 1.4s var(--v-ease-in-out, ease-in-out) infinite; }
  @keyframes af-rot { to { transform: rotate(360deg); } }
  @keyframes af-slide { from { transform: translateX(-100%); } to { transform: translateX(260%); } }
  @media (prefers-reduced-motion: reduce) { .af-spin, .af-bar span { animation: none; } .af-bar span { width: 100%; opacity: .5; } }
  .af-diff { max-height: 26rem; overflow: auto; margin: 0; padding: var(--v-space-3); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); background: rgb(var(--v-tint) / .04); font-family: var(--v-font-mono); font-size: var(--v-text-xs); line-height: 1.5; white-space: pre; }
  .af-diff .add { color: var(--v-success, #3fa46a); }
  .af-diff .del { color: var(--v-danger); }
  .af-diff .hunk { color: var(--v-accent-blue); }
  .af-diff .meta { color: var(--v-text-muted); }
  .af-adopt { display: grid; gap: var(--v-space-3); padding-top: var(--v-space-3); border-top: 1px solid var(--v-line); }
  .af-adopt h4 { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-md); }
  @media (max-width: 640px) { .af-cands { grid-template-columns: 1fr; } .af-adv { grid-template-columns: 1fr; } .af-usage { grid-template-columns: 1fr; } .af-model-row { grid-template-columns: 1fr; gap: var(--v-space-1); } }
</style>
