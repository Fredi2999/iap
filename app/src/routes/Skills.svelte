<script lang="ts">
  import { t } from "../lib/i18n/index.svelte";
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { isTauri } from "@tauri-apps/api/core";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { importSkillFromPath, listInstalledSkills, pickFolderDialog, runSkill, setInstructionSkillActive, skillDryRun, uninstallSkill } from "../lib/ipc";
  import type { InstalledSkillView, DryRunOutcome } from "../lib/types";
  import { notify } from "../lib/notifications";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";

  let skills = $state<InstalledSkillView[]>([]);
  let error = $state<unknown>(null);
  let selected = $state<string | null>(null);
  let selectedTool = $state<string>("");
  let argsJson = $state<string>("{}");
  let outcome = $state<DryRunOutcome | null>(null);
  let runResult = $state<string | null>(null);
  let running = $state(false);
  let dragActive = $state(false);
  let importing = $state(false);
  // Meldung zum letzten Import, direkt an der Ablagefläche (nicht der allgemeine Fehlerhinweis).
  let importMessage = $state<{ kind: "ok" | "error"; text: string } | null>(null);
  let pathText = $state("");
  // Hinweis nach erfolgreichem Import: was bewusst nicht übernommen wurde.
  let importNote = $state<{ name: string; skipped: string[] } | null>(null);
  let confirmRemove = $state(false);
  let loaded = $state(false);

  onMount(() => {
    void refresh();
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    void getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") dragActive = true;
      if (event.payload.type === "leave") dragActive = false;
      if (event.payload.type === "drop") {
        dragActive = false;
        const paths = event.payload.paths;
        if (paths.length > 0) void importDropped(paths);
      }
    }).then((stop) => { if (disposed) stop(); else unlisten = stop; }).catch((reason) => { error = reason; });
    return () => { disposed = true; unlisten?.(); };
  });

  function cleanMessage(reason: unknown): string {
    return String(reason ?? "").replace(/^(Ungültige Eingabe|Internal|Core-Fehler|Vault-Fehler|Launcher-Fehler): /, "");
  }

  // Mehrere abgelegte Ordner werden nacheinander geprüft; jede Meldung nennt den Ordner.
  async function importDropped(paths: string[]) {
    if (importing) return;
    importing = true;
    importMessage = null;
    importNote = null;
    const problems: string[] = [];
    let installed: string[] = [];
    let last: string | null = null;
    for (const path of paths) {
      try {
        const skill = await importSkillFromPath(path);
        installed = [...installed, skill.name];
        last = skill.id;
        if (skill.skipped.length > 0) importNote = { name: skill.name, skipped: skill.skipped };
      } catch (reason) {
        const name = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
        problems.push(paths.length > 1 ? `${name}: ${cleanMessage(reason)}` : cleanMessage(reason));
      }
    }
    await refresh();
    if (last) selectSkill(last);
    if (installed.length > 0) notify("Skill installiert", `${installed.join(", ")} wurde geprüft und installiert.`, "success");
    importMessage = problems.length > 0 ? { kind: "error", text: problems.join("\n") } : null;
    importing = false;
  }

  async function browse() {
    try {
      const chosen = await pickFolderDialog(t("Skill-Ordner wählen"));
      if (chosen) await importDropped([chosen]);
    } catch (reason) {
      importMessage = { kind: "error", text: cleanMessage(reason) };
    }
  }

  async function importTyped() {
    const path = pathText.trim().replace(/^"(.*)"$/, "$1");
    if (!path) return;
    await importDropped([path]);
    if (!importMessage) pathText = "";
  }

  async function refresh() {
    try { skills = await listInstalledSkills(); error = null; }
    catch (reason) { error = reason; }
    finally { loaded = true; }
  }

  async function runDry() {
    if (!selected) return;
    try {
      outcome = await skillDryRun(selected, selectedTool, argsJson);
      error = null;
    } catch (reason) { error = reason; outcome = null; }
  }

  // Echter Aufruf in der Sandbox; das Ergebnis ist das JSON, das der Skill liefert.
  async function runReal() {
    if (!selected) return;
    running = true;
    runResult = null;
    try {
      const raw = await runSkill(selected, selectedTool, argsJson);
      try { runResult = JSON.stringify(JSON.parse(raw), null, 2); } catch { runResult = raw; }
      error = null;
    } catch (reason) { error = reason; }
    finally { running = false; }
  }

  async function toggleActive(skill: InstalledSkillView) {
    try {
      await setInstructionSkillActive(skill.id, !skill.active);
      await refresh();
    } catch (reason) { error = reason; }
  }

  async function removeSelected() {
    if (!skill) return;
    try {
      await uninstallSkill(skill.id, skill.kind);
      selected = null;
      confirmRemove = false;
      await refresh();
    } catch (reason) { error = reason; confirmRemove = false; }
  }

  function selectSkill(id: string) {
    confirmRemove = false;
    selected = id;
    const s = skills.find((x) => x.id === id);
    selectedTool = s?.tools[0]?.name ?? "";
    argsJson = "{}";
    outcome = null;
    runResult = null;
  }

  let skill = $derived(skills.find((s) => s.id === selected) ?? null);
</script>

<div class="v-page">
  <PageHeader title={t("Skills")} description="Anleitungen (SKILL.md) und kleine Programme als Erweiterungen.">
    {#snippet help()}
      {t("Es gibt zwei Arten: Eine Anleitung ist ein Ordner mit einer SKILL.md; IAP gibt ihren Text dem Modell mit, solange sie aktiv ist, und führt nichts daraus aus. Ein Programm ist ein Ordner mit manifest.toml und .wasm-Datei und läuft abgeschottet. Netzwerkzugriff wird immer abgelehnt.")}
    {/snippet}
  </PageHeader>

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  <div class:drag-active={dragActive} class="v-drop-zone" aria-live="polite">
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><path d="M12 16V3m0 0-4 4m4-4 4 4M4 15v4a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4"/></svg>
    <div><strong>{t(importing ? "Skill wird geprüft …" : dragActive ? "Jetzt loslassen" : "Skill-Ordner hierher ziehen")}</strong><span>{t("Ordner mit SKILL.md, oder manifest.toml mit zugehöriger .wasm-Datei")}</span></div>
    <button type="button" class="v-btn v-btn-ghost v-drop-browse" onclick={browse} disabled={importing}>{t("Ordner wählen …")}</button>
  </div>
  {#if importMessage}
    <div class="v-notice warn v-import-message" role="alert">
      <strong>{t("Der Skill wurde nicht installiert.")}</strong>
      <span>{importMessage.text}</span>
      <button type="button" class="v-btn v-btn-ghost" onclick={() => (importMessage = null)}>{t("Schließen")}</button>
    </div>
  {/if}
  {#if importNote}
    <div class="v-notice" role="status">
      <strong>{t("Nicht übernommen bei „{name}“:", { name: importNote.name })}</strong>
      <ul>{#each importNote.skipped as item}<li class="v-num">{item}</li>{/each}</ul>
      <span class="v-help">{t("IAP führt keine Skripte aus Anleitungs-Skills aus.")}</span>
    </div>
  {/if}
  <details class="v-details">
    <summary>{t("Geht das Ziehen nicht? Pfad einfügen")}</summary>
    <form class="v-row" onsubmit={(e) => { e.preventDefault(); void importTyped(); }}>
      <input type="text" class="v-path-input" bind:value={pathText} placeholder="D:\skills\mein-skill" spellcheck="false" aria-label={t("Pfad des Skill-Ordners")} />
      <button type="submit" class="v-btn v-btn-primary" disabled={importing || !pathText.trim()}>{t("Skill installieren")}</button>
    </form>
  </details>

  {#if loaded && skills.length === 0}
    <EmptyState title={t("Noch keine Skills installiert")} text="Zieh einen Skill-Ordner in das Feld oben." icon="M12 3l2.5 5.5L20 9.5l-4 4 1 6L12 16.5 7 19.5l1-6-4-4 5.5-1L12 3Z" />
  {:else if skills.length > 0}
    <div class="v-grid-aside v-skills">
      <ul class="v-skill-list">
        {#each skills as s (s.id)}
          <li>
            <button type="button" class="v-skill" class:active={selected === s.id} onclick={() => selectSkill(s.id)}>
              <span class="v-skill-top"><strong>{s.name}</strong>{#if s.kind === "wasm"}<span class="v-help v-num">{t("Version {v}", { v: s.version })}</span>{/if}</span>
              {#if s.kind === "instructions"}
                <span class="v-help"><span class={s.active ? "v-chip accent" : "v-chip"}>{s.active ? t("Aktiv") : t("Aus")}</span> {t("Anleitung")}</span>
              {:else}
                <span class="v-help">{t("Programm")} · {t("{n} Funktionen", { n: s.tools.length })}{#if s.permissions.some((p) => p.is_sensitive)} · <span class="v-sensitive">{t("{n} sensible Rechte", { n: s.permissions.filter((p) => p.is_sensitive).length })}</span>{/if}</span>
              {/if}
            </button>
          </li>
        {/each}
      </ul>

      {#if skill && skill.kind === "instructions"}
        <section class="v-card v-stack">
          <h2 class="v-card-title">{skill.name}</h2>
          {#if skill.description}<p class="v-card-text">{skill.description}</p>{/if}
          <label class="v-switch-row">
            <input type="checkbox" checked={skill.active} onchange={() => toggleActive(skill)} />
            <span><strong>{t("Aktiv")}</strong> · {t("Der Text geht bei jeder Antwort an das Modell.")}</span>
          </label>
          <p class="v-help">{t("Eine Anleitung hat keine Rechte und führt nichts aus. Bei kleinem Kontext wird der Text gekürzt.")}</p>
          <div class="v-field"><span class="v-label">{t("Text der Anleitung")}</span><pre class="v-skill-result">{skill.body_preview}</pre></div>
          {#if skill.files.length > 0}<div class="v-field"><span class="v-label">{t("Weitere Dateien im Ordner")}</span><ul class="v-skill-files">{#each skill.files as file}<li class="v-num">{file}</li>{/each}</ul><span data-hint class="v-help">{t("Sie werden aufbewahrt, aber nicht automatisch geladen.")}</span></div>{/if}
          {@render removeButton()}
        </section>
      {:else if skill}
        <section class="v-card v-stack">
          <h2 class="v-card-title">{skill.name}</h2>
          <div class="v-field"><span class="v-label">{t("Rechte")}</span>
            <ul class="v-list">
              {#each skill.permissions as p (p.topic)}
                <li><div class="v-list-main"><strong>{p.topic}</strong><span>{p.detail}</span></div>{#if p.is_sensitive}<span class="v-chip warn">{t("Sensibel")}</span>{/if}</li>
              {/each}
            </ul>
          </div>
          <details class="v-details" open>
            <summary>{t("Testen und ausführen")}</summary>
            <div class="v-stack">
              <label class="v-field"><span class="v-label">{t("Funktion")}</span>
                <select bind:value={selectedTool}>{#each skill.tools as tool (tool.name)}<option value={tool.name}>{tool.name}: {tool.description}</option>{/each}</select>
              </label>
              <label class="v-field"><span class="v-label">{t("Eingabe (JSON)")}</span><textarea class="v-num" rows="4" bind:value={argsJson}></textarea></label>
              <div class="v-row v-row-end"><button class="v-btn v-btn-ghost" onclick={runDry}>{t("Testlauf starten")}</button><button class="v-btn v-btn-primary" onclick={runReal} disabled={running}>{t(running ? "Läuft …" : "Ausführen")}</button></div>
              <p class="v-help">{t("Der Testlauf prüft nur. „Ausführen“ läuft abgeschottet, ohne Datei- und Netzzugriff.")}</p>
              {#if outcome}
                <div class="v-notice" class:warn={outcome.warnings.length > 0}>
                  {#if outcome.warnings.length === 0}{t("Keine Auffälligkeiten.")}
                  {:else}<ul>{#each outcome.warnings as w (w)}<li>{w}</li>{/each}</ul>{/if}
                </div>
              {/if}
              {#if runResult !== null}
                <div class="v-field"><span class="v-label">{t("Ergebnis")}</span><pre class="v-skill-result">{runResult}</pre></div>
              {/if}
            </div>
          </details>
        </section>
      {:else}
        <EmptyState title={t("Skill auswählen")} text="Wähle links einen Skill." />
      {/if}
    </div>
  {/if}
</div>

{#snippet removeButton()}
  <div class="v-row v-row-end">
    {#if confirmRemove}
      <span class="v-help">{t("Den Skill samt seinen Dateien entfernen?")}</span>
      <button type="button" class="v-btn v-btn-primary" onclick={removeSelected}>{t("Ja, entfernen")}</button>
      <button type="button" class="v-btn v-btn-ghost" onclick={() => (confirmRemove = false)}>{t("Behalten")}</button>
    {:else}
      <button type="button" class="v-btn v-btn-ghost" onclick={() => (confirmRemove = true)}>{t("Skill entfernen …")}</button>
    {/if}
  </div>
{/snippet}

<style>
  .v-switch-row { display: flex; gap: var(--v-space-3); align-items: flex-start; color: var(--v-text-secondary); font-size: var(--v-text-sm); line-height: 1.45; }
  .v-switch-row input { flex: none; width: auto; margin-top: .2rem; }
  .v-skill-files { display: flex; flex-wrap: wrap; gap: var(--v-space-1); margin: 0; padding: 0; list-style: none; }
  .v-skill-files li { padding: 1px 8px; border: 1px solid var(--v-line); border-radius: 9999px; font-size: var(--v-text-xs); color: var(--v-text-secondary); }
  .v-skill-result { max-height: 16rem; margin: 0; overflow: auto; padding: var(--v-space-3); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); background: rgb(var(--v-tint) / .04); font-family: var(--v-font-mono); font-size: var(--v-text-sm); white-space: pre-wrap; }
  .v-drop-zone { display: flex; align-items: center; gap: var(--v-space-4); min-height: 5.5rem; padding: var(--v-space-4) var(--v-space-5); border: 1px dashed color-mix(in oklab, var(--v-accent-blue) 45%, transparent); border-radius: var(--v-radius-card); background: rgb(var(--v-tint) / .03); color: var(--v-accent-blue); transition: border-color 160ms ease, background-color 160ms ease; }
  .v-drop-zone.drag-active { border-style: solid; border-color: var(--v-accent-blue); background: var(--v-accent-blue-soft); }
  .v-drop-zone div { display: grid; gap: 2px; }
  .v-drop-zone strong { color: var(--v-text-primary); font-size: var(--v-text-md); }
  .v-drop-zone span { color: var(--v-text-muted); font-size: var(--v-text-sm); }
  .v-skill-list { display: flex; flex-direction: column; gap: var(--v-space-2); margin: 0; padding: 0; list-style: none; }
  .v-skill { display: flex; flex-direction: column; gap: 4px; width: 100%; padding: var(--v-space-3) var(--v-space-4); border: 1px solid var(--v-line); border-radius: var(--v-radius-card); background: var(--v-surface-2); text-align: left; transition: border-color 150ms ease; }
  .v-skill.active { border-color: color-mix(in oklab, var(--v-accent-blue) 60%, transparent); }
  .v-skill:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: 2px; }
  .v-skill-top { display: flex; align-items: baseline; justify-content: space-between; gap: var(--v-space-2); }
  .v-skill-top strong { color: var(--v-text-primary); font-size: var(--v-text-md); }
  .v-sensitive { color: var(--v-warning); }
  .v-skills { grid-template-columns: minmax(14rem, 20rem) minmax(0, 1fr); }
  @media (max-width: 900px) { .v-skills { grid-template-columns: 1fr; } }
  .v-notice ul { margin: 0; padding-left: 1.1rem; }
  .v-import-message { display: grid; gap: var(--v-space-2); justify-items: start; white-space: pre-line; }
  .v-drop-browse { margin-left: auto; flex: none; }
  .v-path-input { flex: 1; min-width: 14rem; }
</style>
