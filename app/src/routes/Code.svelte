<script lang="ts">
  import { t, tk, locale } from "../lib/i18n/index.svelte";
  import { onMount, tick, untrack } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import {
    applyHunks,
    codeAssist,
    codeAssistCancel,
    codeCreate,
    codeDelete,
    codeAgentChange,
    codeList,
    codeRootsStatus,
    codeRename,
    codeSearch,
    diffCodeFile,
    gitInfo,
    gitCommit,
    gitLog,
    gitStatus,
    onCodeAssistProgress,
    readCodeFile,
    snapshotCreate,
    snapshotDiscard,
    snapshotList,
    snapshotRestore,
    writeCodeFile,
  } from "../lib/ipc";
  import type {
    CodeAssistReply,
    CodeHit,
    CodeRootsStatus,
    GitCommit as GitCommitEntry,
    GitStatusEntry,
    SettingsSnapshot,
    SnapshotView,
    UnifiedDiff,
    WorkspaceEntry,
  } from "../lib/types";
  import { buildState, goToLine, languageFor, openSearch, setWordWrap, wordWrapEnabled, type CursorInfo } from "../lib/code-editor";
  import { code, closeTab, dirtyCount, dropTabs, editorStates, isDirty, renameExpanded, renameTabs, activeTab, tree, type CodeTab } from "../lib/stores/code.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import FileTree from "../lib/components/code/FileTree.svelte";
  import CodeRootBar from "../lib/components/code/CodeRootBar.svelte";
  import CodeAgent from "../lib/components/code/CodeAgent.svelte";
  import CodePromptBar from "../lib/components/code/CodePromptBar.svelte";
  import { agent, clearAgentView, discardChange } from "../lib/stores/codeAgent.svelte";

  interface Props {
    settings: SettingsSnapshot;
    onSettingsChanged: (snapshot: SettingsSnapshot) => void;
    onOpenSettings: () => void;
  }
  let { settings, onSettingsChanged, onOpenSettings }: Props = $props();

  // Hinweise, die das Backend im Klartext liefert; hier als Übersetzungsschlüssel markiert.
  const GIT_NOTES = [tk("Das Git-Paket ist nicht eingerichtet (Einstellungen, Pakete)."), tk("Der Arbeitsordner ist kein Git-Repository.")];
  void GIT_NOTES;

  // ---------------------------------------------------------------- Zustand

  let listingError = $state<unknown>(null);
  let pane = $state<"files" | "search" | "agent">("files");
  /** Der Bereich arbeitet in einem freigegebenen Ordner auf dem PC (nicht im Stick-Arbeitsordner). */
  let isHost = $state(false);

  let editorHost: HTMLDivElement | null = null;
  let view: EditorView | null = null;
  let shownPath: string | null = null;
  let cursor = $state<CursorInfo>({ line: 1, column: 1, selected: 0, selectedLines: 0 });
  let wrap = $state(wordWrapEnabled());

  function toggleWrap() {
    if (!view) return;
    wrap = !wrap;
    setWordWrap(view, wrap);
  }
  function editorSearch() { if (view) openSearch(view); }
  function editorGoto() { if (view) goToLine(view); }

  let diff = $state<UnifiedDiff | null>(null);
  let diffPath = $state<string | null>(null);
  let selectedHunks = $state<Set<number>>(new Set());
  let diffError = $state<unknown>(null);
  let status = $state("");
  let pageError = $state<unknown>(null);
  /** Der Vorschlag des Agenten, der zuletzt in den Editor geladen wurde, und ein Fehler dabei. */
  let reviewed = $state<string | null>(null);
  let reviewError = $state<unknown>(null);
  let diffSection = $state<HTMLElement | null>(null);
  let sidePane = $state<HTMLElement | null>(null);

  /** Öffnet den Reiter „IAP“ und holt die Spalte ins Bild, damit die Eingabe unten nicht verdeckt bleibt. */
  async function openAgentPane() {
    pane = "agent";
    await tick();
    sidePane?.scrollIntoView({ block: "nearest" });
  }
  // Nur solange der Vorschlag noch offen ist, gilt er als „im Editor“.
  const reviewing = $derived(agent.changes.some((change) => change.path === reviewed) ? reviewed : null);

  let closing = $state<string | null>(null);
  let creating = $state<{ directory: boolean; value: string } | null>(null);
  let fileNote = $state("");

  let searchQuery = $state("");
  let hits = $state<CodeHit[]>([]);
  let searching = $state(false);
  let searched = $state(false);
  let searchError = $state<unknown>(null);

  let instruction = $state("");
  let assistBusy = $state(false);
  let assistChars = $state(0);
  let assistReply = $state<CodeAssistReply | null>(null);
  let assistTarget = $state<{ path: string; from: number; to: number; text: string; base: string } | null>(null);
  let assistError = $state<unknown>(null);
  let assistLoaded = $state(false);

  let snapshots = $state<SnapshotView[]>([]);
  let snapshotError = $state<unknown>(null);
  let gitOk = $state(false);
  let gitNote = $state<string | null>(null);
  let gitBranch = $state<string | null>(null);
  let gitEntries = $state<GitStatusEntry[]>([]);
  let gitCommits = $state<GitCommitEntry[]>([]);
  let commitMessage = $state("");
  let gitError = $state<unknown>(null);
  let gitBusy = $state(false);

  let tab = $derived(code.tabs.find((entry) => entry.path === code.active) ?? null);
  // Welche Hälfte der gemeinsamen Assistenten-Karte sichtbar ist. Ohne geöffnete Datei gibt es nur den Ordner.
  let assistMode = $state<"folder" | "file">("folder");
  const assistHalf = $derived<"folder" | "file">(tab ? assistMode : "folder");
  let language = $derived(tab ? languageFor(tab.path).label : "");
  let dirty = $derived(tab ? isDirty(tab) : false);

  let unlistenProgress: UnlistenFn | null = null;

  async function loadGit() {
    try {
      const info = await gitInfo();
      gitOk = info.available;
      gitBranch = info.branch;
      gitNote = info.note;
      gitEntries = [];
      gitCommits = [];
      if (gitOk) await refreshGit();
    } catch (reason) {
      gitError = reason;
    }
  }

  /** Nach einem Wechsel des Arbeitsordners gehört nichts mehr vom alten Ordner auf den Schirm. */
  function resetForRoot(status: CodeRootsStatus) {
    isHost = status.host_path !== null;
    clearAgentView();
    code.tabs.splice(0);
    code.active = null;
    code.dir = "";
    editorStates.clear();
    tree.children = {};
    tree.expanded = {};
    tree.marks = {};
    diff = null;
    diffPath = null;
    hits = [];
    searched = false;
    assistReply = null;
    assistTarget = null;
    fileNote = "";
    pageError = null;
    reviewed = null;
    reviewError = null;
    snapshots = [];
    snapshotError = null;
    gitOk = false;
    gitBranch = null;
    void refreshTree();
    if (!isHost) void refreshSnapshots();
    void loadGit();
  }

  onMount(() => {
    void (async () => {
      try {
        isHost = (await codeRootsStatus()).host_path !== null;
      } catch {
        // Ohne Angabe gilt der Stick-Arbeitsordner.
      }
      void refreshTree();
      if (!isHost) void refreshSnapshots();
      void loadGit();
    })();
    void onCodeAssistProgress((chars) => (assistChars = chars)).then((off) => (unlistenProgress = off));
    return () => {
      unlistenProgress?.();
      if (view && shownPath) editorStates.set(shownPath, view.state);
      view?.destroy();
      view = null;
      shownPath = null;
    };
  });

  // Die aktive Datei bestimmt, was der Editor zeigt. Der Zustand jeder Datei (Verlauf,
  // Markierung) bleibt beim Wechsel erhalten.
  $effect(() => {
    const path = code.active;
    untrack(() => showTab(path));
  });

  // Eine Diff-Ansicht gehört zu genau einer Datei.
  $effect(() => {
    const path = code.active;
    untrack(() => {
      if (diffPath !== path) {
        diff = null;
        diffPath = null;
      }
    });
  });

  const handlers = {
    onChange(doc: string) {
      const current = activeTab();
      if (current) current.content = doc;
    },
    onSave() {
      void saveActive();
    },
    onCursor(info: CursorInfo) {
      cursor = info;
    },
  };

  function ensureView(): EditorView | null {
    if (view) return view;
    if (!editorHost) return null;
    view = new EditorView({ state: EditorState.create({ doc: "" }), parent: editorHost });
    return view;
  }

  function showTab(path: string | null) {
    const editor = ensureView();
    if (!editor) return;
    if (shownPath && shownPath !== path) editorStates.set(shownPath, editor.state);
    shownPath = path;
    if (!path) return;
    const entry = code.tabs.find((candidate) => candidate.path === path);
    if (!entry) return;
    let state = editorStates.get(path);
    if (!state) state = buildState(entry.content, path, handlers);
    editor.setState(state);
    const main = state.selection.main;
    const line = state.doc.lineAt(main.head);
    cursor = {
      line: line.number,
      column: main.head - line.from + 1,
      selected: main.to - main.from,
      selectedLines: main.empty ? 0 : state.doc.lineAt(main.to).number - state.doc.lineAt(main.from).number + 1,
    };
  }

  /** Ersetzt den Inhalt der gezeigten Datei als Bearbeitung, damit „Rückgängig“ funktioniert. */
  function replaceDoc(text: string) {
    if (!view) return;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
  }

  // ------------------------------------------------------------ Dateibaum

  function parentOf(path: string): string {
    const index = path.lastIndexOf("/");
    return index < 0 ? "" : path.slice(0, index);
  }

  function baseName(path: string): string {
    return path.slice(path.lastIndexOf("/") + 1);
  }

  /** Lädt den Inhalt eines Ordners in den Baum. `false`, wenn es ihn nicht mehr gibt. */
  async function loadDir(relative: string): Promise<boolean> {
    try {
      tree.children[relative] = (await codeList(relative)).entries;
      if (relative === "") listingError = null;
      return true;
    } catch (reason) {
      if (relative === "") listingError = reason;
      else delete tree.children[relative];
      return false;
    }
  }

  /** Liest die Wurzel und alle aufgeklappten Ordner neu; verschwundene Ordner klappen zu. */
  async function refreshTree() {
    const dirs = ["", ...Object.keys(tree.expanded).filter((key) => tree.expanded[key])];
    await Promise.all(dirs.map(loadDir));
    for (const dir of dirs) {
      if (dir !== "" && !tree.children[dir]) delete tree.expanded[dir];
    }
    if (code.dir !== "" && !tree.children[code.dir]) code.dir = "";
    void refreshMarks();
  }

  async function toggleDir(entry: WorkspaceEntry) {
    code.dir = entry.relative_path;
    if (tree.expanded[entry.relative_path]) {
      tree.expanded[entry.relative_path] = false;
      return;
    }
    tree.expanded[entry.relative_path] = true;
    if (!(await loadDir(entry.relative_path))) tree.expanded[entry.relative_path] = false;
  }

  async function openFile(entry: WorkspaceEntry) {
    code.dir = parentOf(entry.relative_path);
    await openPath(entry.relative_path);
  }

  /** Git-Kürzel je Datei für den Baum; stillschweigend leer, wenn Git nicht verfügbar ist. */
  async function refreshMarks() {
    if (!gitOk) return;
    try {
      const marks: Record<string, string> = {};
      for (const entry of await gitStatus()) marks[entry.relative_path] = entry.status;
      tree.marks = marks;
    } catch {
      // Der Baum funktioniert auch ohne Markierungen; Fehler zeigt die Git-Ansicht selbst.
    }
  }

  async function openPath(path: string, line?: number) {
    pageError = null;
    try {
      const existing = code.tabs.find((entry) => entry.path === path);
      if (!existing) {
        const text = await readCodeFile(path);
        code.tabs.push({ path, original: text, content: text });
      }
      code.active = path;
      await tick();
      if (line && view) {
        const target = view.state.doc.line(Math.min(Math.max(line, 1), view.state.doc.lines));
        view.dispatch({ selection: { anchor: target.from }, effects: EditorView.scrollIntoView(target.from, { y: "center" }) });
        view.focus();
      }
    } catch (reason) {
      pageError = reason;
    }
  }

  /** Ein Vorschlag des Agenten, der gespeichert wurde, ist erledigt und verschwindet aus der Liste. */
  async function settleProposal(path: string) {
    if (agent.changes.some((change) => change.path === path)) await discardChange(path);
  }

  /** Lädt einen Vorschlag des Agenten in den Editor; die Diff-Ansicht zeigt jede Änderung. */
  async function reviewChange(path: string) {
    reviewError = null;
    try {
      const change = await codeAgentChange(path);
      let entry = code.tabs.find((candidate) => candidate.path === path);
      if (entry && isDirty(entry)) {
        reviewError = t("Speichere oder verwirf zuerst die Änderungen an dieser Datei, bevor du den Vorschlag lädst.");
        return;
      }
      if (!entry) {
        const original = change.is_new ? "" : await readCodeFile(path);
        code.tabs.push({ path, original, content: original });
        entry = code.tabs[code.tabs.length - 1];
      }
      code.active = path;
      await tick();
      replaceDoc(change.proposed);
      await refreshDiff();
      reviewed = path;
      await tick();
      diffSection?.scrollIntoView({ block: "nearest" });
    } catch (reason) {
      reviewError = reason;
    }
  }

  async function createEntry() {
    if (!creating) return;
    const name = creating.value.trim();
    if (!name) return;
    const full = code.dir ? `${code.dir}/${name}` : name;
    try {
      await codeCreate(full, creating.directory);
      const wasDirectory = creating.directory;
      creating = null;
      if (code.dir) tree.expanded[code.dir] = true;
      await refreshTree();
      if (!wasDirectory) await openPath(full);
      pageError = null;
    } catch (reason) {
      pageError = reason;
    }
  }

  async function renameEntry(from: string, value: string) {
    const name = value.trim();
    if (!name || name.includes("/") || name.includes("\\")) {
      pageError = t("Gib nur einen Namen ein, ohne Schrägstrich.");
      return;
    }
    const parent = parentOf(from);
    const to = parent ? `${parent}/${name}` : name;
    if (to === from) return;
    try {
      await codeRename(from, to);
      renameTabs(from, to);
      renameExpanded(from, to);
      if (code.dir === from || code.dir.startsWith(`${from}/`)) code.dir = to + code.dir.slice(from.length);
      pageError = null;
      await refreshTree();
    } catch (reason) {
      pageError = reason;
    }
  }

  /** Verschiebt per Ziehen in einen anderen Ordner; offene Tabs folgen der Datei. */
  async function moveEntry(from: string, toDir: string) {
    const to = toDir ? `${toDir}/${baseName(from)}` : baseName(from);
    if (isOpenDirty(from) && !confirm(t("„{name}“ hat ungespeicherte Änderungen. Trotzdem verschieben?", { name: baseName(from) }))) return;
    try {
      await codeRename(from, to);
      renameTabs(from, to);
      renameExpanded(from, to);
      if (toDir) tree.expanded[toDir] = true;
      pageError = null;
      await refreshTree();
    } catch (reason) {
      pageError = reason;
    }
  }

  async function deleteEntry(path: string) {
    try {
      await codeDelete(path);
      dropTabs(path);
      fileNote = t("„{name}“ liegt jetzt im Papierkorb (Ordner .trash im Arbeitsordner).", { name: baseName(path) });
      pageError = null;
      await refreshTree();
    } catch (reason) {
      pageError = reason;
    }
  }

  function isOpenDirty(path: string): boolean {
    return code.tabs.some((entry) => isDirty(entry) && (entry.path === path || entry.path.startsWith(`${path}/`)));
  }

  // --------------------------------------------------------------- Suche

  async function runSearch() {
    const query = searchQuery.trim();
    if (query.length < 2) return;
    searching = true;
    searchError = null;
    try {
      hits = await codeSearch(query);
      searched = true;
    } catch (reason) {
      searchError = reason;
    } finally {
      searching = false;
    }
  }

  function showSearch() {
    pane = "search";
  }

  // ---------------------------------------------------- Speichern und Diff

  function timeLabel(): string {
    return new Date().toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
  }

  async function saveTab(entry: CodeTab): Promise<boolean> {
    try {
      await writeCodeFile(entry.path, entry.content);
      entry.original = entry.content;
      void settleProposal(entry.path);
      void refreshMarks();
      if (diffPath === entry.path) {
        diff = null;
        diffPath = null;
      }
      return true;
    } catch (reason) {
      pageError = reason;
      return false;
    }
  }

  async function saveActive() {
    const entry = activeTab();
    if (!entry || !isDirty(entry)) return;
    status = t("Speichere …");
    status = (await saveTab(entry)) ? t("Gespeichert um {time}", { time: timeLabel() }) : "";
  }

  async function saveAll() {
    let saved = 0;
    for (const entry of code.tabs.filter(isDirty)) {
      if (await saveTab(entry)) saved += 1;
    }
    status = saved > 0 ? t("{n} Dateien gespeichert um {time}", { n: saved, time: timeLabel() }) : "";
  }

  async function refreshDiff() {
    const entry = activeTab();
    if (!entry) return;
    diffError = null;
    try {
      diff = await diffCodeFile(entry.path, entry.content);
      diffPath = entry.path;
      selectedHunks = new Set(diff.hunks.map((_, index) => index));
    } catch (reason) {
      diffError = reason;
    }
  }

  function toggleHunk(index: number) {
    const next = new Set(selectedHunks);
    if (next.has(index)) next.delete(index);
    else next.add(index);
    selectedHunks = next;
  }

  async function applySelectedHunks() {
    const entry = activeTab();
    if (!entry || !diff) return;
    status = t("Übernehme ausgewählte Blöcke …");
    try {
      const indices = [...selectedHunks].sort((a, b) => a - b);
      const applied = await applyHunks(entry.path, entry.content, indices);
      // Was nicht gewählt wurde, bleibt draußen: Der Editor zeigt jetzt die gespeicherte Datei.
      entry.original = applied;
      void settleProposal(entry.path);
      void refreshMarks();
      replaceDoc(applied);
      entry.content = applied;
      diff = null;
      diffPath = null;
      selectedHunks = new Set();
      status = t("Blöcke übernommen um {time}", { time: timeLabel() });
    } catch (reason) {
      pageError = reason;
      status = "";
    }
  }

  // ------------------------------------------------------ Tabs schließen

  function requestClose(path: string) {
    const entry = code.tabs.find((candidate) => candidate.path === path);
    if (entry && isDirty(entry)) {
      closing = path;
      return;
    }
    closeTab(path);
  }

  async function saveAndClose(path: string) {
    const entry = code.tabs.find((candidate) => candidate.path === path);
    if (entry && !(await saveTab(entry))) return;
    closing = null;
    closeTab(path);
  }

  function discardAndClose(path: string) {
    closing = null;
    closeTab(path);
  }

  // ------------------------------------------------------------ Assistent

  const QUICK: { label: string; text: string }[] = [
    { label: "Erklären", text: "Erkläre kurz und verständlich, was dieser Code tut. Ändere den Code nicht." },
    { label: "Fehler suchen", text: "Suche Fehler und riskante Stellen und behebe sie. Ändere nur, was nötig ist." },
    { label: "Kommentieren", text: "Ergänze knappe, hilfreiche Kommentare zu den wichtigen Stellen. Ändere sonst nichts." },
    { label: "Vereinfachen", text: "Vereinfache den Code, ohne sein Verhalten zu ändern." },
  ];

  function scopeLabel(): string {
    return cursor.selected > 0 ? t("Auswahl: {n} Zeilen", { n: cursor.selectedLines }) : t("Ganze Datei");
  }

  async function askAssistant() {
    const entry = activeTab();
    if (!entry || !view || assistBusy) return;
    const text = instruction.trim();
    if (!text) return;
    const main = view.state.selection.main;
    const doc = view.state.doc;
    const selection = main.empty
      ? null
      : { before: doc.sliceString(0, main.from), text: doc.sliceString(main.from, main.to), after: doc.sliceString(main.to) };
    assistTarget = { path: entry.path, from: main.from, to: main.to, text: selection?.text ?? "", base: selection?.text ?? entry.content };
    assistBusy = true;
    assistChars = 0;
    assistReply = null;
    assistLoaded = false;
    assistError = null;
    try {
      assistReply = await codeAssist({ relative_path: entry.path, instruction: text, content: entry.content, selection });
    } catch (reason) {
      assistError = reason;
    } finally {
      assistBusy = false;
    }
  }

  function cancelAssistant() {
    void codeAssistCancel();
  }

  /** Der Vorschlag ändert die Datei nicht, wenn er dem aktuellen Text gleicht (nur eine Erklärung). */
  let replyChangesNothing = $derived.by(() => {
    if (!assistReply || !assistTarget) return false;
    return assistReply.code.trim() === assistTarget.base.trim();
  });

  async function loadReply() {
    const entry = activeTab();
    if (!assistReply || !assistTarget || !entry || !view) return;
    if (assistTarget.path !== entry.path) {
      assistError = t("Du hast die Datei gewechselt. Stelle die Frage noch einmal.");
      return;
    }
    if (assistReply.scope === "selection") {
      const current = view.state.doc.sliceString(assistTarget.from, assistTarget.to);
      if (current !== assistTarget.text) {
        assistError = t("Der markierte Text hat sich inzwischen geändert. Stelle die Frage noch einmal.");
        return;
      }
      let insert = assistReply.code;
      if (assistTarget.text.endsWith("\n") && !insert.endsWith("\n")) insert += "\n";
      view.dispatch({ changes: { from: assistTarget.from, to: assistTarget.to, insert } });
    } else {
      replaceDoc(assistReply.code);
    }
    assistLoaded = true;
    assistError = null;
    await refreshDiff();
  }

  function onPageKey(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === "f") {
      event.preventDefault();
      showSearch();
    }
  }

  // ------------------------------------------------ Sicherungen und Git

  async function refreshSnapshots() {
    try {
      snapshots = await snapshotList();
      snapshotError = null;
    } catch (reason) {
      snapshotError = reason;
    }
  }

  async function createSnapshot() {
    try {
      await snapshotCreate();
      await refreshSnapshots();
    } catch (reason) {
      snapshotError = reason;
    }
  }

  async function restoreSnapshot(id: string) {
    if (!confirm(t("Diesen Sicherungspunkt zurückholen? Ungespeicherte Änderungen im Arbeitsordner gehen verloren."))) return;
    try {
      await snapshotRestore(id);
      snapshotError = null;
      await refreshTree();
      for (const entry of [...code.tabs]) {
        try {
          const text = await readCodeFile(entry.path);
          entry.original = text;
          editorStates.delete(entry.path);
          if (entry.path === code.active) replaceDoc(text);
          entry.content = text;
        } catch {
          // Die Datei gibt es im Sicherungspunkt nicht: Tab schließen.
          closeTab(entry.path);
        }
      }
    } catch (reason) {
      snapshotError = reason;
    }
  }

  async function discardSnapshot(id: string) {
    if (!confirm(t("Diesen Sicherungspunkt endgültig löschen?"))) return;
    try {
      await snapshotDiscard(id);
      await refreshSnapshots();
    } catch (reason) {
      snapshotError = reason;
    }
  }

  async function refreshGit() {
    gitBusy = true;
    try {
      [gitEntries, gitCommits] = await Promise.all([gitStatus(), gitLog(20)]);
      const marks: Record<string, string> = {};
      for (const entry of gitEntries) marks[entry.relative_path] = entry.status;
      tree.marks = marks;
      gitError = null;
    } catch (reason) {
      gitError = reason;
    } finally {
      gitBusy = false;
    }
  }

  async function doCommit() {
    if (commitMessage.trim().length === 0) {
      gitError = t("Bitte kurz beschreiben, was du geändert hast.");
      return;
    }
    gitBusy = true;
    try {
      await gitCommit(commitMessage, []);
      commitMessage = "";
      gitError = null;
      await refreshGit();
    } catch (reason) {
      gitError = reason;
    } finally {
      gitBusy = false;
    }
  }

  function fmtBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  }

  /** Setzt den Cursor beim Einblenden in das Feld (das HTML-Attribut autofocus greift hier nicht). */
  function focusOnMount(node: HTMLInputElement) {
    node.focus();
  }

</script>

<svelte:window onkeydown={onPageKey} />

<div class="v-page">
  <PageHeader title="Code" description="Dateien bearbeiten. IAP schlägt vor, du bestätigst jede Änderung.">
    {#snippet actions()}
      <button type="button" class="v-btn v-btn-ghost" onclick={refreshDiff} disabled={!tab}>{t("Änderungen prüfen")}</button>
      <button type="button" class="v-btn v-btn-primary" onclick={saveAll} disabled={dirtyCount() === 0}>
        {dirtyCount() > 1 ? t("Alle speichern ({n})", { n: dirtyCount() }) : t("Speichern")}
      </button>
    {/snippet}
    {#snippet help()}
      {t("Dateien im Arbeitsordner oder in einem freigegebenen Ordner auf dem PC bearbeiten. IAP macht Vorschläge; gespeichert wird erst, wenn du jede Änderung gesehen und bestätigt hast.")}
    {/snippet}
  </PageHeader>

  {#if pageError}<ErrorNotice error={pageError} onDismiss={() => (pageError = null)} />{/if}

  <CodeRootBar canSwitch={() => dirtyCount() === 0} onSwitched={resetForRoot} />

  <div class="v-code" class:agent-open={pane === "agent"}>
    <aside class="v-card v-code-files" aria-label={t("Dateien")} bind:this={sidePane}>
      <div class="v-segmented" role="tablist" aria-label={t("Ansicht")}>
        <button type="button" role="tab" aria-selected={pane === "files"} class:active={pane === "files"} onclick={() => (pane = "files")}>{t("Dateien")}</button>
        <button type="button" role="tab" aria-selected={pane === "search"} class:active={pane === "search"} onclick={showSearch}>{t("Suchen")}</button>
        <button type="button" role="tab" aria-selected={pane === "agent"} class:active={pane === "agent"} onclick={openAgentPane}>
          {t("IAP")}
          {#if agent.running}<span class="v-code-pulse" role="img" aria-label={t("IAP arbeitet")}></span>
          {:else if agent.changes.length > 0}<span class="v-code-badge" role="img" aria-label={t("Vorschläge ({n})", { n: agent.changes.length })}>{agent.changes.length}</span>{/if}
        </button>
      </div>

      {#if pane === "files"}
        {#if listingError}
          <ErrorNotice error={listingError} onRetry={refreshTree} />
        {:else if tree.children[""]}
          <div class="v-code-tools">
            <button type="button" class="v-btn v-btn-ghost" onclick={() => (creating = { directory: false, value: "" })}>{t("Neue Datei")}</button>
            <button type="button" class="v-btn v-btn-ghost" onclick={() => (creating = { directory: true, value: "" })}>{t("Neuer Ordner")}</button>
          </div>
          <p class="v-help v-code-target">{t("Neu in: {ordner}", { ordner: code.dir ? code.dir : t("Arbeitsordner") })}{#if gitBranch}{" · "}{t("Zweig {name}", { name: gitBranch })}{/if}</p>

          {#if creating}
            <form class="v-code-inline" onsubmit={(event) => { event.preventDefault(); void createEntry(); }}>
              <input use:focusOnMount bind:value={creating.value} placeholder={creating.directory ? t("Name des Ordners") : t("Name der Datei, z. B. notizen.md")} aria-label={creating.directory ? t("Name des Ordners") : t("Name der Datei")} onkeydown={(event) => event.key === "Escape" && (creating = null)} />
              <button type="submit" class="v-btn v-btn-primary" disabled={creating.value.trim() === ""}>{t("Anlegen")}</button>
              <button type="button" class="v-btn v-btn-ghost" onclick={() => (creating = null)}>{t("Abbrechen")}</button>
            </form>
          {/if}

          {#if fileNote}<p class="v-help v-code-note" role="status">{fileNote}</p>{/if}

          <FileTree
            children={tree.children}
            expanded={tree.expanded}
            marks={tree.marks}
            selectedDir={code.dir}
            activePath={code.active}
            openPaths={code.tabs.map((entry) => entry.path)}
            hasUnsaved={isOpenDirty}
            onToggle={toggleDir}
            onOpen={openFile}
            onSelectDir={(path) => (code.dir = path)}
            onRename={renameEntry}
            onDelete={deleteEntry}
            onMove={moveEntry}
            canDelete={!isHost}
          />
        {/if}
      {:else if pane === "search"}
        <form class="v-code-inline v-code-search" onsubmit={(event) => { event.preventDefault(); void runSearch(); }}>
                    <input use:focusOnMount type="search" bind:value={searchQuery} placeholder={t("In allen Dateien suchen")} aria-label={t("Suchbegriff")} />
          <button type="submit" class="v-btn v-btn-primary" disabled={searching || searchQuery.trim().length < 2}>{searching ? t("Suche …") : t("Suchen")}</button>
        </form>
        {#if searchError}<ErrorNotice error={searchError} onDismiss={() => (searchError = null)} />{/if}
        {#if searched && hits.length === 0 && !searchError}
          <p class="v-help">{t("Nichts gefunden.")}</p>
        {/if}
        {#if hits.length > 0}
          <p class="v-help" role="status">{hits.length >= 200 ? t("Die ersten 200 Treffer:") : t("{n} Treffer", { n: hits.length })}</p>
          <ul class="v-code-hits">
            {#each hits as hit, index (index)}
              <li>
                <button type="button" onclick={() => openPath(hit.relative_path, hit.line)}>
                  <span class="v-code-hit-path v-num">{hit.relative_path}:{hit.line}</span>
                  <span class="v-code-hit-text">{hit.text}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}

      <div class="v-code-agent-pane" hidden={pane !== "agent"}>
        <div class="v-segmented" role="group" aria-label={t("Wobei soll IAP helfen?")}>
          <button type="button" class:active={assistHalf === "folder"} aria-pressed={assistHalf === "folder"} onclick={() => (assistMode = "folder")}>{t("Ganzer Ordner")}</button>
          <button type="button" class:active={assistHalf === "file"} aria-pressed={assistHalf === "file"} disabled={!tab} title={tab ? undefined : t("Öffne zuerst eine Datei.")} onclick={() => (assistMode = "file")}>{t("Diese Datei")}</button>
        </div>

        <div class="v-code-agent-half" hidden={assistHalf !== "folder"}>
          <CodeAgent activeFile={tab?.path ?? null} onReview={reviewChange} {reviewing} {reviewError} onDismissReviewError={() => (reviewError = null)} locked={assistBusy} {settings} {onSettingsChanged} {onOpenSettings} onError={(reason) => (pageError = reason)} />
        </div>

        {#if tab}
          <div class="v-stack v-code-agent-half" hidden={assistHalf !== "file"}>
            <div class="v-row">
              <span class="v-chip" title={t("Ohne Markierung bekommt IAP die ganze Datei, mit Markierung nur den Abschnitt.")}>{scopeLabel()}</span>
            </div>
            <div class="v-code-quick" role="group" aria-label={t("Schnellaufgaben")}>
              {#each QUICK as quick (quick.label)}
                <button type="button" class="v-chip v-code-chip" disabled={assistBusy} onclick={() => (instruction = quick.text)}>{t(quick.label)}</button>
              {/each}
            </div>
            <CodePromptBar inputId="iap-code-file-prompt" bind:value={instruction} busy={assistBusy} locked={agent.running} placeholder={t("Was soll IAP tun? Z. B. Prüfung auf leere Eingaben ergänzen.")}
              {settings} {onSettingsChanged} {onOpenSettings} onSend={askAssistant} onStop={cancelAssistant} onError={(reason) => (pageError = reason)} />
            <div class="v-row v-row-between">
              {#if assistBusy}
                <span class="v-help v-num" role="status">{t("IAP schreibt … {n} Zeichen", { n: assistChars })}</span>
              {/if}
            </div>
            {#if assistError}<ErrorNotice error={assistError} onDismiss={() => (assistError = null)} />{/if}
            {#if assistReply}
              <div class="v-code-reply" role="status">
                {#if assistReply.explanation}<p class="v-card-text">{assistReply.explanation}</p>{/if}
                {#if replyChangesNothing}
                  <p class="v-help">{t("IAP schlägt keine Änderung am Code vor.")}</p>
                {:else if assistLoaded}
                  <p data-hint class="v-help">{t("Vorschlag im Editor. Rückgängig mit Strg+Z.")}</p>
                {:else}
                  <div class="v-row">
                    <button type="button" class="v-btn v-btn-primary" onclick={loadReply}>{t("Vorschlag im Editor ansehen")}</button>
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </aside>

    <div class="v-stack v-code-main">
      <section class="v-card v-code-editor" aria-label={t("Editor")}>
        {#if code.tabs.length > 0}
          <div class="v-code-tabs" role="tablist" aria-label={t("Offene Dateien")}>
            {#each code.tabs as entry (entry.path)}
              <span class="v-code-tab" class:active={entry.path === code.active}>
                <button type="button" role="tab" aria-selected={entry.path === code.active} title={entry.path} onclick={() => (code.active = entry.path)}>
                  <span>{baseName(entry.path)}</span>
                  {#if isDirty(entry)}<span class="v-code-dot" role="img" aria-label={t("Ungespeichert")}></span>{/if}
                </button>
                <button type="button" class="v-code-tab-close" aria-label={t("„{name}“ schließen", { name: baseName(entry.path) })} onclick={() => requestClose(entry.path)}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
                </button>
              </span>
            {/each}
          </div>
        {/if}

        {#if closing}
          <div class="v-code-banner" role="alertdialog" aria-label={t("Ungespeicherte Änderungen")}>
            <span>{t("„{name}“ hat ungespeicherte Änderungen.", { name: baseName(closing) })}</span>
            <span class="v-row">
              <button type="button" class="v-btn v-btn-primary" onclick={() => closing && saveAndClose(closing)}>{t("Speichern und schließen")}</button>
              <button type="button" class="v-btn v-btn-ghost" onclick={() => closing && discardAndClose(closing)}>{t("Verwerfen")}</button>
              <button type="button" class="v-btn v-btn-ghost" onclick={() => (closing = null)}>{t("Abbrechen")}</button>
            </span>
          </div>
        {/if}

        {#if !tab}
          <div class="v-code-placeholder">
            <EmptyState title="Noch keine Datei geöffnet" text="Wähle links eine Datei oder lege eine neue an. Mit Strg+Umschalt+F suchst du in allen Dateien.">
              {#snippet action()}
                <button type="button" class="v-btn v-btn-ghost" onclick={() => { pane = "files"; creating = { directory: false, value: "" }; }}>{t("Neue Datei")}</button>
              {/snippet}
            </EmptyState>
          </div>
        {/if}
        <div bind:this={editorHost} class="v-code-host" class:hidden={!tab}></div>

        {#if tab}
          <div class="v-code-statusbar">
            <span class="v-num">{t("Zeile {l}, Spalte {c}", { l: cursor.line, c: cursor.column })}</span>
            {#if cursor.selected > 0}<span class="v-num">{t("{n} Zeichen markiert", { n: cursor.selected })}</span>{/if}
            <span>{language}</span>
            <span class="v-code-tools">
              <button type="button" class="v-code-tool" onclick={editorSearch} title={t("Suchen und ersetzen (Strg+F)")} aria-label={t("Suchen und ersetzen (Strg+F)")}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></svg>
              </button>
              <button type="button" class="v-code-tool" onclick={editorGoto} title={t("Gehe zu Zeile (Strg+G)")} aria-label={t("Gehe zu Zeile (Strg+G)")}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 7h10M4 12h16M4 17h10"/></svg>
              </button>
              <button type="button" class="v-code-tool" class:active={wrap} onclick={toggleWrap} aria-pressed={wrap} title={t("Zeilenumbruch")} aria-label={t("Zeilenumbruch")}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 7h16M4 12h13a3 3 0 0 1 0 6h-4m0 0 2-2m-2 2 2 2M4 17h4"/></svg>
              </button>
            </span>
            <span class="v-code-status-end">
              {#if dirty}<span class="v-chip warn">{t("Ungespeichert")}</span>{/if}
              {#if status}<span role="status">{status}</span>{/if}
            </span>
          </div>
        {/if}
      </section>

      {#if diffError}<ErrorNotice error={diffError} onDismiss={() => (diffError = null)} />{/if}

      {#if diff && diffPath === code.active}
        <section class="v-card v-stack" aria-label={t("Änderungen")} bind:this={diffSection}>
          <div class="v-card-title">
            <span>{t("Änderungen")} <small>{diff.hunks.length === 1 ? t("1 Block") : t("{n} Blöcke", { n: diff.hunks.length })}</small></span>
            <span class="v-row">
              {#if diff.hunks.length > 1}
                <button type="button" class="v-btn v-btn-ghost" onclick={() => (selectedHunks = selectedHunks.size === diff!.hunks.length ? new Set() : new Set(diff!.hunks.map((_, i) => i)))}>
                  {selectedHunks.size === diff.hunks.length ? t("Keinen wählen") : t("Alle wählen")}
                </button>
              {/if}
              <button type="button" class="v-btn v-btn-primary" onclick={applySelectedHunks} disabled={selectedHunks.size === 0}>{t("Ausgewählte speichern ({n})", { n: selectedHunks.size })}</button>
            </span>
          </div>
          {#if diff.hunks.length === 0}<p class="v-card-text">{t("Keine Unterschiede zur gespeicherten Datei.")}</p>
          {:else}<p class="v-help">{t("Nicht gewählte Blöcke werden verworfen, wenn du speicherst.")}</p>{/if}
          {#each diff.hunks as hunk, index (index)}
            <div class="v-hunk">
              <label class="v-hunk-head">
                <input type="checkbox" checked={selectedHunks.has(index)} onchange={() => toggleHunk(index)} />
                <span>{t("Block {n}: Zeile {from} bis {to}", { n: index + 1, from: hunk.new_start, to: hunk.new_start + Math.max(0, hunk.new_lines - 1) })}</span>
              </label>
              <pre>{#each hunk.lines as line, li (li)}<div class={line.op}>{line.op === "insert" ? "+" : line.op === "delete" ? "-" : " "}{line.text}</div>{/each}</pre>
            </div>
          {/each}
        </section>
      {/if}

      <details class="v-card v-details">
        <summary>{t("Sicherungspunkte und Versionen")}</summary>
        <div class="v-grid-2">
          {#if !isHost}
          <div class="v-stack">
            <div class="v-card-title"><span>{t("Sicherungspunkte")}</span><button type="button" class="v-btn v-btn-ghost" onclick={createSnapshot}>{t("Jetzt sichern")}</button></div>
            {#if snapshotError}<ErrorNotice error={snapshotError} onDismiss={() => (snapshotError = null)} />{/if}
            {#if snapshots.length === 0}
              <p class="v-card-text">{t("Noch keine Sicherungspunkte.")}</p>
            {:else}
              <ul class="v-list">
                {#each snapshots as snap (snap.id)}
                  <li>
                    <div class="v-list-main"><strong>{new Date(snap.created_unix_ms).toLocaleString(locale())}</strong><span>{t("{n} Dateien", { n: snap.file_count })} · {fmtBytes(snap.bytes_referenced)}</span></div>
                    <button type="button" class="v-btn v-btn-ghost" onclick={() => restoreSnapshot(snap.id)}>{t("Zurückholen")}</button>
                    <button type="button" class="v-btn-icon" aria-label={t("Sicherungspunkt löschen")} onclick={() => discardSnapshot(snap.id)}><svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3m-9 0 1 13h10l1-13" /></svg></button>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
          {/if}
          <div class="v-stack">
            <div class="v-card-title"><span>{t("Versionen (Git)")}</span>{#if gitOk}<button type="button" class="v-btn v-btn-ghost" onclick={refreshGit} disabled={gitBusy}>{t("Aktualisieren")}</button>{/if}</div>
            {#if !gitOk}
              <p class="v-card-text">{gitNote ? t(gitNote) : t("Der Arbeitsordner ist kein Git-Projekt.")} </p>
            {:else}
              {#if gitError}<ErrorNotice error={gitError} onDismiss={() => (gitError = null)} />{/if}
              {#if gitEntries.length === 0}<p class="v-help">{t("Keine offenen Änderungen.")}</p>
              {:else}
                <ul class="v-list">{#each gitEntries as entry (entry.relative_path)}<li><span class="v-chip warn v-num">{entry.status}</span><div class="v-list-main"><strong>{entry.relative_path}</strong></div></li>{/each}</ul>
              {/if}
              <input placeholder={t("Was hast du geändert?")} bind:value={commitMessage} aria-label={t("Beschreibung der Version")} />
              <button type="button" class="v-btn v-btn-primary" onclick={doCommit} disabled={gitBusy || commitMessage.trim().length === 0}>{t("Version speichern")}</button>
              {#if gitCommits.length > 0}
                <ul class="v-list">{#each gitCommits as commit (commit.id)}<li><div class="v-list-main"><strong>{commit.summary}</strong><span>{commit.author} · {new Date(commit.unix_ts * 1000).toLocaleDateString(locale())}</span></div></li>{/each}</ul>
              {/if}
            {/if}
          </div>
        </div>
      </details>
    </div>
  </div>
</div>

<style>
  .v-code { display: grid; grid-template-columns: minmax(15rem, 19rem) minmax(0, 1fr); gap: var(--v-space-4); align-items: start; }
  .v-code.agent-open { grid-template-columns: minmax(18rem, 25rem) minmax(0, 1fr); }
  @media (max-width: 900px) { .v-code, .v-code.agent-open { grid-template-columns: minmax(0, 1fr); } }
  .v-code-files { padding: var(--v-space-3); display: grid; align-content: start; gap: var(--v-space-2); position: sticky; top: 0; max-height: calc(100dvh - 10rem); overflow-y: auto; min-width: 0; }
  @media (max-width: 900px) { .v-code-files { position: static; max-height: 22rem; } }
  /* Reiter „IAP“: Die Spalte hat feste Höhe, damit nur der Verlauf scrollt und Vorschläge und Eingabe sichtbar bleiben. */
  .v-code.agent-open .v-code-files { height: calc(100dvh - 12rem); min-height: 28rem; grid-template-rows: auto minmax(0, 1fr); }
  @media (max-width: 900px) { .v-code.agent-open .v-code-files { height: auto; max-height: none; } }
  .v-code-agent-pane { display: flex; flex-direction: column; gap: var(--v-space-3); min-height: 0; }
  .v-code-agent-pane[hidden], .v-code-agent-half[hidden] { display: none; }
  .v-code-agent-pane > .v-segmented { align-self: flex-start; }
  .v-code-agent-half { min-height: 0; flex: 1 1 auto; display: flex; flex-direction: column; }
  .v-code-agent-half > :global(.v-agent) { flex: 1 1 auto; }
  .v-code-agent-pane :global(.v-agent-log) { flex: 1 1 auto; }
  @media (max-width: 900px) { .v-code-agent-pane :global(.v-agent-log) { max-height: 20rem; } }
  .v-code-pulse { display: inline-block; width: 7px; height: 7px; margin-left: var(--v-space-2); border-radius: 9999px; background: var(--v-accent-blue); animation: v-code-pulse 1.4s ease-in-out infinite; }
  @keyframes v-code-pulse { 50% { opacity: .35; } }
  @media (prefers-reduced-motion: reduce) { .v-code-pulse { animation: none; } }
  .v-code-badge { display: inline-block; min-width: 1.1rem; margin-left: var(--v-space-2); padding: 0 .3rem; border-radius: 9999px; background: var(--v-accent-blue-soft); color: var(--v-text-primary); font-size: var(--v-text-xs); line-height: 1.1rem; text-align: center; }
  .v-code-tools { display: flex; flex-wrap: wrap; gap: var(--v-space-1); }
  .v-code-inline { display: flex; flex-wrap: wrap; gap: var(--v-space-2); align-items: center; }
  .v-code-inline input { flex: 1 1 8rem; min-width: 0; }
  .v-code-note, .v-code-target { margin: 0; }
  .v-code-hits { margin: 0; padding: 0; list-style: none; display: grid; gap: 1px; }
  .v-code-hits button { display: grid; gap: 2px; width: 100%; padding: var(--v-space-2); border: 0; border-radius: var(--v-radius-control); background: transparent; text-align: left; color: var(--v-text-secondary); font-size: var(--v-text-sm); }
  .v-code-hits button:hover { background: rgb(var(--v-tint) / .08); }
  .v-code-hit-path { color: var(--v-accent-blue); font-size: var(--v-text-xs); overflow-wrap: anywhere; }
  .v-code-hit-text { font-family: var(--v-font-mono); font-size: var(--v-text-xs); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .v-code-main { min-width: 0; }
  .v-code-editor { padding: 0; overflow: hidden; }
  .v-code-tabs { display: flex; overflow-x: auto; border-bottom: 1px solid var(--v-line); scrollbar-width: thin; }
  .v-code-tab { display: inline-flex; align-items: center; flex: 0 0 auto; max-width: 16rem; border-right: 1px solid var(--v-line); }
  .v-code-tab > button:first-child { display: inline-flex; align-items: center; gap: var(--v-space-2); min-height: 2.5rem; padding: 0 var(--v-space-2) 0 var(--v-space-3); border: 0; background: transparent; color: var(--v-text-muted); font-size: var(--v-text-sm); min-width: 0; }
  .v-code-tab > button:first-child span:first-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .v-code-tab.active { background: rgb(var(--v-tint) / .08); box-shadow: inset 0 -2px 0 var(--v-accent-blue); }
  .v-code-tab.active > button:first-child { color: var(--v-text-primary); }
  .v-code-tab-close { display: grid; place-items: center; width: 1.75rem; height: 1.75rem; margin-right: var(--v-space-1); border: 0; border-radius: 6px; background: transparent; color: var(--v-text-muted); }
  .v-code-tab-close:hover { background: rgb(var(--v-tint) / .12); color: var(--v-text-primary); }
  .v-code-dot { width: 7px; height: 7px; border-radius: 9999px; background: var(--v-warning); flex: 0 0 auto; }
  .v-code-banner { display: flex; flex-wrap: wrap; gap: var(--v-space-2); align-items: center; justify-content: space-between; padding: var(--v-space-2) var(--v-space-4); border-bottom: 1px solid var(--v-line); background: rgb(var(--v-tint) / .05); font-size: var(--v-text-sm); color: var(--v-text-secondary); }
  .v-code-host { height: clamp(18rem, 46dvh, 38rem); font-size: var(--v-text-sm); }
  .v-code-host :global(.cm-editor) { height: 100%; }
  .v-code-host.hidden { display: none; }
  .v-code-placeholder { padding: var(--v-space-5); }
  .v-code-statusbar { display: flex; flex-wrap: wrap; gap: var(--v-space-4); align-items: center; min-height: 2.25rem; padding: 0 var(--v-space-4); border-top: 1px solid var(--v-line); color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .v-code-status-end { margin-left: auto; display: inline-flex; gap: var(--v-space-2); align-items: center; }
  .v-code-tools { display: inline-flex; gap: 2px; align-items: center; }
  .v-code-tool { display: grid; place-items: center; width: 1.75rem; height: 1.75rem; border: 0; border-radius: var(--v-radius-control); background: transparent; color: var(--v-text-muted); cursor: pointer; }
  .v-code-tool:hover { color: var(--v-text-primary); background: rgb(var(--v-tint) / .08); }
  .v-code-tool.active { color: var(--v-accent-blue); background: var(--v-accent-blue-soft); }
  .v-code-tool:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: -2px; }

  .v-code-quick { display: flex; flex-wrap: wrap; gap: var(--v-space-1); }
  .v-code-chip { cursor: pointer; background: transparent; }
  .v-code-chip:hover:not(:disabled) { background: rgb(var(--v-tint) / .08); color: var(--v-text-primary); }
  .v-row-between { display: flex; flex-wrap: wrap; gap: var(--v-space-2); align-items: center; justify-content: space-between; }
  .v-code-reply { display: grid; gap: var(--v-space-2); padding: var(--v-space-3); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); background: rgb(var(--v-tint) / .04); }
  .v-code-reply p { margin: 0; }

  .v-hunk { border: 1px solid var(--v-line); border-radius: var(--v-radius-field); overflow: hidden; }
  .v-hunk-head { display: flex; align-items: center; gap: var(--v-space-2); padding: var(--v-space-2) var(--v-space-3); border-bottom: 1px solid var(--v-line); color: var(--v-text-secondary); font-size: var(--v-text-sm); cursor: pointer; }
  .v-hunk pre { margin: 0; padding: var(--v-space-2) var(--v-space-3); overflow-x: auto; font-family: var(--v-font-mono); font-size: var(--v-text-xs); }
  .v-hunk .insert { color: var(--v-success); background: color-mix(in srgb, var(--v-success) 9%, transparent); }
  .v-hunk .delete { color: var(--v-danger); background: var(--v-danger-soft); }
  .v-hunk .context { color: var(--v-text-muted); }
</style>
