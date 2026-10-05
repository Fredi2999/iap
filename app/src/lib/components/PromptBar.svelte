<script lang="ts">
  import MicButton from "./MicButton.svelte";
  import { t, tk } from "../i18n/index.svelte";
  import type { AvailableModel } from "../types";
  import "./PromptBar.css";

  export type PromptMode = "normal" | "plan" | "agent";
  /** Eintrag im „/“-Menü: eingebauter Befehl, Vorlage oder Skill. */
  export interface SlashItem { id: string; group: "command" | "template" | "skill"; name: string; description: string; }
  export type ThinkingLevel = "kurz" | "standard" | "sorgfältig" | "vertieft" | "maximal";
  // Anzeigenamen der Denkstufen; die Stufen selbst sind interne Werte und werden nicht übersetzt.
  const THINKING_NAMES: Record<ThinkingLevel, string> = { kurz: tk("Kurz"), standard: tk("Standard"), "sorgfältig": tk("Sorgfältig"), vertieft: tk("Vertieft"), maximal: tk("Maximal") };
  const THINKING_LEVELS: ThinkingLevel[] = ["kurz", "standard", "sorgfältig", "vertieft", "maximal"];

  interface Props {
    value: string;
    mode?: PromptMode;
    toolsEnabled?: boolean;
    thinking?: ThinkingLevel;
    busy: boolean;
    models: AvailableModel[];
    modelId: string;
    modelBusy: boolean;
    onSelectModel: (id: string) => void;
    onOpenSettings: () => void;
    onOpenFiles?: () => void;
    onSend: (event: Event) => void;
    onStop: () => void;
    slashItems?: SlashItem[];
    onSlash?: (item: SlashItem) => void;
    /** Öffnet die Auswahl für Dokumente dieser Unterhaltung. */
    onAttachDocument?: () => void;
    /** Nimmt diktierten Text entgegen; ohne Angabe gibt es keinen Mikrofonknopf. */
    onDictate?: (text: string) => void;
    /**
     * `code` blendet die reinen Chat-Schalter aus (Dateimenü, Denkstufe, Modus, Werkzeuge),
     * weil sie im Code-Bereich nichts bewirken würden. Modellwahl, Diktat und Senden bleiben.
     */
    variant?: "chat" | "code";
    /** Bereits übersetzter Platzhaltertext; ohne Angabe gilt der Chat-Text. */
    placeholder?: string;
    /** Kennung des Eingabefelds; sie muss je Fenster eindeutig sein (Code-Bereich hat zwei Leisten). */
    inputId?: string;
  }

  let {
    value = $bindable(""),
    mode = $bindable<PromptMode>("normal"),
    toolsEnabled = $bindable(false),
    thinking = $bindable<ThinkingLevel>("standard"),
    busy,
    models,
    modelId,
    modelBusy,
    onSelectModel,
    onOpenSettings,
    onOpenFiles,
    onSend,
    onStop,
    slashItems = [],
    onSlash,
    onAttachDocument,
    onDictate,
    variant = "chat",
    placeholder,
    inputId = "iap-prompt",
  }: Props = $props();

  // „/“-Menü wie in Claude Code: öffnet sich, solange nur „/wort“ in der Eingabe steht.
  // Ohne Animation, weil es vor allem mit der Tastatur und sehr oft benutzt wird.
  const SLASH_GROUPS: { id: SlashItem["group"]; label: string }[] = [
    { id: "command", label: tk("Befehle") },
    { id: "template", label: tk("Vorlagen") },
    { id: "skill", label: tk("Skills") },
  ];
  let slashIndex = $state(0);
  let slashDismissedFor = $state<string | null>(null);
  const slashQuery = $derived(/^\/\S*$/.test(value) ? value.slice(1).toLocaleLowerCase() : null);
  const slashMatches = $derived.by(() => {
    const query = slashQuery;
    if (query === null) return [];
    const hits = slashItems.filter((item) => {
      const name = item.name.slice(1).toLocaleLowerCase();
      return name.startsWith(query) || (query.length > 1 && (name.includes(query) || item.description.toLocaleLowerCase().includes(query)));
    });
    // Gruppenreihenfolge beibehalten, damit die Pfeiltasten der sichtbaren Liste folgen.
    return SLASH_GROUPS.flatMap((group) => hits.filter((item) => item.group === group.id)).slice(0, 14);
  });
  const slashOpen = $derived(slashQuery !== null && slashDismissedFor !== value && slashMatches.length > 0);
  $effect(() => { void slashQuery; slashIndex = 0; });

  function chooseSlash(item: SlashItem) {
    slashDismissedFor = null;
    if (item.id === "cmd:modell") { value = ""; modelOpen = true; filesOpen = false; thinkingOpen = false; return; }
    if (item.id === "cmd:denken") { value = ""; thinkingOpen = false; toggleThinking(); return; }
    onSlash?.(item);
    queueMicrotask(() => textareaEl?.focus());
  }

  let filesOpen = $state(false);
  let thinkingOpen = $state(false);
  let modelOpen = $state(false);
  let textareaEl = $state<HTMLTextAreaElement | null>(null);
  let effortTrack = $state<HTMLDivElement | null>(null);
  let rootEl = $state<HTMLDivElement | null>(null);
  let modelLabel = $derived(models.find((model) => model.id === modelId)?.display_name ?? modelId);
  let compactModelLabel = $derived(modelLabel
    .replace(/(?:[\s-]+)(?:Instruct|Abliterated)(?:[\s-].*)?$/i, "")
    .replace(/\s+Q\d+_[A-Za-z0-9_]+$/i, "")
    .replace(/-/g, " "));
  let effortIndex = $derived(THINKING_LEVELS.indexOf(thinking));
  // Während des Ziehens folgt der Regler dem Zeiger stufenlos; erst beim Loslassen
  // rastet er animiert auf die nächste Stufe ein. `null` = kein aktiver Zug.
  let dragFraction = $state<number | null>(null);
  let dragOverflow = $state(0);
  let dragMoved = $state(false);
  let thinkingButton = $state<HTMLButtonElement | null>(null);
  let thinkingMenuLeft = $state(0);
  let effortVisual = $derived(dragFraction ?? effortIndex / (THINKING_LEVELS.length - 1));

  // Über den Rand hinaus folgt der Daumen dem Zeiger nur noch zu einem Fünftel und höchstens um
  // wenige Pixel, damit er nicht hart anschlägt.
  const MAX_OVERFLOW_PX = 10;
  // Feste Funkenpositionen für die lila Maximal-Stufe: [x %, y %, Größe px, Verzögerung s].
  const MAX_SPARKS: [number, number, number, number][] = [
    [5, 60, 3, -1.2], [8, 75, 2, -2.7], [11, 91, 3, -0.4], [17, 69, 2, -3.1], [21, 84, 2, -1.9], [28, 95, 2, -2.3],
    [34, 65, 3, -0.8], [39, 82, 2, -3.6], [45, 92, 3, -1.5], [51, 71, 2, -2.9], [56, 88, 3, -0.2], [62, 97, 2, -3.3],
    [67, 74, 2, -1.1], [73, 90, 3, -2.1], [79, 65, 2, -3.7], [84, 84, 2, -0.6], [89, 96, 3, -2.5], [94, 76, 2, -1.8],
  ];
  function decay(value: number, max: number) {
    return Math.max(-max, Math.min(max, value * 0.2));
  }

  function closeMenus() {
    filesOpen = false;
    thinkingOpen = false;
    modelOpen = false;
  }

  function toggleThinking() {
    thinkingOpen = !thinkingOpen;
    filesOpen = false;
    modelOpen = false;
    if (thinkingOpen && thinkingButton && rootEl) {
      // Popover am Auslöser ausrichten, damit es sichtbar aus ihm heraus wächst.
      const menuWidth = Math.min(20.5 * 16, window.innerWidth - 48);
      const buttonLeft = thinkingButton.getBoundingClientRect().left - rootEl.getBoundingClientRect().left;
      thinkingMenuLeft = Math.max(0, Math.min(buttonLeft - 8, rootEl.clientWidth - menuWidth));
    }
  }

  function setEffort(index: number) {
    thinking = THINKING_LEVELS[Math.max(0, Math.min(THINKING_LEVELS.length - 1, index))];
  }

  function effortFromPointer(event: PointerEvent) {
    const rail = effortTrack?.querySelector<HTMLElement>(".prompt-effort-rail");
    if (!rail) return;
    const rect = rail.getBoundingClientRect();
    const raw = event.clientX - rect.left;
    const width = Math.max(1, rect.width);
    const fraction = Math.max(0, Math.min(1, raw / width));
    dragOverflow = raw < 0 ? decay(raw, MAX_OVERFLOW_PX) : raw > width ? decay(raw - width, MAX_OVERFLOW_PX) : 0;
    dragFraction = fraction;
    setEffort(Math.round(fraction * (THINKING_LEVELS.length - 1)));
  }

  function effortPointerDown(event: PointerEvent & { currentTarget: HTMLDivElement }) {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    event.currentTarget.focus();
    dragMoved = false;
    effortFromPointer(event);
  }

  function effortPointerMove(event: PointerEvent) {
    if (dragFraction === null) return;
    dragMoved = true;
    effortFromPointer(event);
  }

  function effortPointerEnd() {
    dragFraction = null;
    dragOverflow = 0;
    dragMoved = false;
  }

  function effortKeydown(event: KeyboardEvent) {
    const direction = event.key === "ArrowRight" || event.key === "ArrowUp" ? 1
      : event.key === "ArrowLeft" || event.key === "ArrowDown" ? -1 : 0;
    if (direction) { event.preventDefault(); setEffort(effortIndex + direction); }
    if (event.key === "Home") { event.preventDefault(); setEffort(0); }
    if (event.key === "End") { event.preventDefault(); setEffort(THINKING_LEVELS.length - 1); }
  }

  function resizeInput() {
    if (!textareaEl) return;
    textareaEl.style.height = "auto";
    textareaEl.style.height = `${Math.min(textareaEl.scrollHeight, 144)}px`;
  }

  $effect(() => {
    void value;
    queueMicrotask(resizeInput);
  });

  function handleKeydown(event: KeyboardEvent) {
    if (slashOpen && !event.isComposing) {
      if (event.key === "ArrowDown") { event.preventDefault(); slashIndex = (slashIndex + 1) % slashMatches.length; return; }
      if (event.key === "ArrowUp") { event.preventDefault(); slashIndex = (slashIndex - 1 + slashMatches.length) % slashMatches.length; return; }
      if ((event.key === "Enter" && !event.shiftKey) || event.key === "Tab") { event.preventDefault(); chooseSlash(slashMatches[slashIndex]); return; }
      if (event.key === "Escape") { event.preventDefault(); slashDismissedFor = value; return; }
    }
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      onSend(event);
    }
    if (event.key === "Escape") closeMenus();
  }
</script>

<svelte:window
  onkeydown={(event) => { if (event.key === "Escape") closeMenus(); }}
  onpointerdown={(event) => { if (rootEl && !rootEl.contains(event.target as Node)) closeMenus(); }}
/>

<div bind:this={rootEl} class="prompt-bar" data-variant={variant} data-max={thinking === "maximal" ? "" : undefined}>
  {#if slashOpen}
    <div id="iap-slash-menu" class="prompt-popup prompt-slash-menu" role="listbox" aria-label={t("Befehle, Vorlagen und Skills")}>
      {#each slashMatches as item, index (item.id)}
        {#if index === 0 || slashMatches[index - 1].group !== item.group}<strong>{t(SLASH_GROUPS.find((group) => group.id === item.group)?.label ?? "")}</strong>{/if}
        <button type="button" id={"slash-" + index} role="option" aria-selected={index === slashIndex} class:selected={index === slashIndex}
          onpointerenter={() => (slashIndex = index)} onclick={() => chooseSlash(item)}>
          <span class="prompt-slash-name">{item.name}</span><small>{item.description}</small>
        </button>
      {/each}
    </div>
  {/if}

  {#if filesOpen}
    <div class="prompt-popup prompt-files-menu">
      {#if onAttachDocument}<button type="button" onclick={() => { closeMenus(); onAttachDocument?.(); }}>{t("Dokument anhängen")}</button>{/if}
      <button type="button" onclick={() => { closeMenus(); onOpenFiles?.(); }}>{t("Dateien öffnen")}</button>
    </div>
  {/if}

  {#if modelOpen}
    <div id="iap-model-menu" class="prompt-popup prompt-model-menu" role="group" aria-label={t("Lokales Modell wählen")}>
      <strong>{t("Installierte Modelle")}</strong>
      {#each models as model}
        <button type="button" aria-pressed={model.id === modelId} onclick={() => { closeMenus(); onSelectModel(model.id); }}>
          <span>{model.display_name}</span><small>{t(model.id === modelId ? "Aktiv" : model.is_default ? "Standard" : "Lokal")}</small>
        </button>
      {:else}
        <p>{t("Keine Modellliste verfügbar.")}</p>
      {/each}
      <button type="button" class="prompt-model-settings" onclick={() => { closeMenus(); onOpenSettings(); }}>{t("Modelle konfigurieren")}</button>
    </div>
  {/if}

  {#if thinkingOpen}
    <div class="prompt-popup prompt-thinking-menu" role="group" aria-label={t("Denkstufe wählen")} style={"left: " + thinkingMenuLeft + "px"}>
      <div class="prompt-thinking-head">
        <span>{t("Denken")}</span><strong>{t(THINKING_NAMES[thinking])}</strong>
        <span class="prompt-thinking-help" title={t("Steuert das Denkbudget, sofern das gewählte Modell einen Denkmodus unterstützt.")} aria-label={t("Information zum Denkbudget")}>
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M9.8 9a2.3 2.3 0 1 1 3.7 1.8c-1 .8-1.5 1.2-1.5 2.3M12 17h.01"/></svg>
        </span>
      </div>
      <div class="prompt-thinking-ends"><span>{t("Schneller")}</span><span>{t("Gründlicher")}</span></div>
      <div bind:this={effortTrack} class="prompt-effort-track" role="slider" tabindex="0" aria-label={t("Denkstufe")}
        aria-valuemin="0" aria-valuemax="4" aria-valuenow={effortIndex} aria-valuetext={t(THINKING_NAMES[thinking])}
        data-dragging={dragMoved ? "" : undefined} data-pressed={dragFraction !== null ? "" : undefined}
        style={"--effort-p: " + effortVisual + "; --effort-overflow: " + dragOverflow.toFixed(2) + "px"}
        onpointerdown={effortPointerDown}
        onpointermove={effortPointerMove}
        onpointerup={effortPointerEnd}
        onpointercancel={effortPointerEnd}
        onlostpointercapture={effortPointerEnd}
        onkeydown={effortKeydown}>
        <span class="prompt-effort-clip" aria-hidden="true">
          <span class="prompt-effort-rail"><span class="prompt-effort-pos"><span class="prompt-effort-fill"></span></span></span>
        </span>
        <span class="prompt-effort-rail" aria-hidden="true">
          {#each THINKING_LEVELS as level, index (level)}
            <i class:passed={index <= effortIndex} style={"left: " + index * 25 + "%"}></i>
          {/each}
          <span class="prompt-effort-pos"><span class="prompt-effort-thumb"></span></span>
        </span>
      </div>
    </div>
  {/if}

  <form onsubmit={onSend} aria-label={t("Nachricht verfassen")}>
    {#if thinking === "maximal"}
      <div class="prompt-max-sparks" aria-hidden="true">
        {#each MAX_SPARKS as spark}
          <i style={"--spark-x: " + spark[0] + "%; --spark-y: " + spark[1] + "%; --spark-size: " + spark[2] + "px; --spark-delay: " + spark[3] + "s"}></i>
        {/each}
      </div>
    {/if}
    <label class="sr-only" for={inputId}>{t("Nachricht an IAP")}</label>
    <textarea id={inputId} rows={variant === "code" ? 2 : 1} bind:this={textareaEl} bind:value
      placeholder={placeholder ?? t(mode === "plan" ? "Vorhaben beschreiben oder mit OK bestätigen …" : "Nachricht an IAP …")}
      role="combobox" aria-autocomplete="list" aria-expanded={slashOpen} aria-controls="iap-slash-menu" aria-activedescendant={slashOpen ? "slash-" + slashIndex : undefined}
      oninput={resizeInput} onfocus={closeMenus} onkeydown={handleKeydown}></textarea>

    <div class="prompt-controls">
      {#if variant === "chat"}
        <button type="button" class="prompt-tool-button prompt-add" aria-label={t("Dateien öffnen")} title={t("Dateien öffnen")} aria-expanded={filesOpen}
          onclick={() => { filesOpen = !filesOpen; thinkingOpen = false; modelOpen = false; }}>
          <svg width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M12 4v16M4 12h16"/></svg>
        </button>
      {/if}

      <button type="button" class="prompt-picker prompt-model-picker" title={modelLabel} aria-label={t("Lokales Modell wählen")} aria-expanded={modelOpen} aria-controls="iap-model-menu"
        disabled={busy || modelBusy} onclick={() => { modelOpen = !modelOpen; filesOpen = false; thinkingOpen = false; }}>
        <span>{modelBusy ? t("Wechsle …") : compactModelLabel}</span>
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>
      </button>

      {#if variant === "chat"}
        <button type="button" bind:this={thinkingButton} class="prompt-picker prompt-thinking-picker" title={t("Denkstufe wählen")} aria-label={t("Denkstufe: {level}", { level: t(THINKING_NAMES[thinking]) })} aria-expanded={thinkingOpen}
          onclick={toggleThinking}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="m12 2 1.6 5.4L19 9l-5.4 1.6L12 16l-1.6-5.4L5 9l5.4-1.6L12 2ZM19 15l.8 2.2L22 18l-2.2.8L19 21l-.8-2.2L16 18l2.2-.8L19 15Z"/></svg>
          <span>{t(THINKING_NAMES[thinking])}</span>
        </button>

        <div class="prompt-mode-switch" role="group" aria-label={t("Chat-Modus")}>
          <button type="button" class:active={mode === "normal"} aria-pressed={mode === "normal"} onclick={() => (mode = "normal")}>{t("Normal")}</button>
          <button type="button" class:active={mode === "plan"} aria-pressed={mode === "plan"} onclick={() => (mode = "plan")}>{t("Planmodus")}</button>
          <button type="button" class:active={mode === "agent"} aria-pressed={mode === "agent"} onclick={() => (mode = "agent")}>{t("Agent")}</button>
        </div>
      {/if}

      <span class="prompt-spacer"></span>
      {#if variant === "chat"}
        <label class="prompt-tools" title={t(mode === "plan" ? "Im Planmodus werden Werkzeuge erst nach deinem OK eingesetzt." : "IAP darf Werkzeuge nutzen. Jede Aktion wird vorher auf Sicherheit geprüft.")}>
          <input type="checkbox" bind:checked={toolsEnabled} /><span>{t("Werkzeuge")}</span>
        </label>
      {/if}

      {#if onDictate}<MicButton compact onText={onDictate} />{/if}

      {#if busy}
        <button type="button" class="prompt-send armed" onclick={onStop} aria-label={t("Antwort stoppen")} title={t("Antwort stoppen")}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>
        </button>
      {:else}
        <button type="submit" class="prompt-send" class:armed={value.trim().length > 0} disabled={value.trim().length === 0}
          aria-label={t("Nachricht senden")} title={t("Nachricht senden (Enter; Umschalt+Enter für Zeilenumbruch)")}>
          <svg width="17" height="17" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 3 4 11h5v10h6V11h5l-8-8Z"/></svg>
        </button>
      {/if}
    </div>
  </form>
</div>
