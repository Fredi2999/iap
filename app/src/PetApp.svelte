<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { t, tk } from "./lib/i18n/index.svelte";
  import { STATE_TEXT } from "./lib/avatar";
  import { friendlyError } from "./lib/errors";
  import {
    cancelStream,
    getUiLanguage,
    loadUiState,
    openConversation,
    petExpand,
    requestQuit,
    sendMessage,
    settingsSnapshot,
    showMainWindow,
    subscribeToStream,
    onWindowMode,
    onPetOpenPanel,
    onUiLanguageChanged,
    systemStats,
  } from "./lib/ipc";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import AvatarMenu, { type MenuItem } from "./lib/components/AvatarMenu.svelte";
  import Bloub from "./lib/components/Bloub.svelte";
  import Markdown from "./lib/components/Markdown.svelte";
  import MicButton from "./lib/components/MicButton.svelte";
  import PetStats from "./lib/components/PetStats.svelte";
  import PetCustomizer from "./lib/components/PetCustomizer.svelte";
  import ScreenLookDialog from "./lib/components/ScreenLookDialog.svelte";
  import { setLanguage } from "./lib/i18n/index.svelte";
  import { applyAppearance } from "./lib/stores/app.svelte";
  import { avatar, currentAvatarState, initAvatarStore, markStopped } from "./lib/stores/avatar.svelte";
  import { conversation, initConversationStore, setActive } from "./lib/stores/conversation.svelte";
  import { endConversationMode, initVoiceStore, setMuted, startListening, voice } from "./lib/stores/voice.svelte";
  import { currentTask, initJobsStore, jobs } from "./lib/stores/jobs.svelte";
  import { initPetStyle, petRows, petStyle } from "./lib/stores/petStyle.svelte";
  import { initThemeColors } from "./lib/stores/themeColors.svelte";
  import type { ScreenAnswer, SystemStats } from "./lib/types";

  // Das Pet: derselbe Avatar und dieselbe Unterhaltung in einem kleinen
  // transparenten Fenster. Es startet nichts von allein: Aufnahme, Bildschirm
  // und Aufträge brauchen eine bewusste Aktion im Menü oder im Feld. Nicht
  // sichtbar (Hauptfenster offen), ruht die Animation.
  let mainOpen = $state(false);
  const mood = $derived(currentAvatarState());
  let menu = $state<{ x: number; y: number } | null>(null);
  let panelOpen = $state(false);
  let lookOpen = $state(false);
  let answer = $state("");
  // Kurze Sprechblase über dem Pet: zeigt die Antwort, ohne das große Panel zu öffnen.
  let bubbleOpen = $state(false);
  let bubbleScroll = $state<HTMLDivElement | null>(null);
  let draft = $state("");
  let error = $state<unknown>(null);
  let liveId: string | null = null;
  // Solange eine Antwort läuft, darf das Nachladen aus dem Verlauf sie nicht überschreiben:
  // `setActive` löst es beim Start aus und würde sonst den alten Text davor setzen.
  let live = false;
  let unlisten: UnlistenFn[] = [];
  let avatarButton = $state<HTMLButtonElement | null>(null);

  let customizeOpen = $state(false);
  let stats = $state<SystemStats | null>(null);
  const rows = $derived(petRows(petStyle));
  const task = $derived(currentTask(jobs.list));

  const expanded = $derived(panelOpen || lookOpen || customizeOpen || menu !== null);
  const bubbleShown = $derived(bubbleOpen && !expanded);
  const bubbleText = $derived(error ? friendlyError(error).message : answer);
  // Fenstergröße folgt Figurgröße und Zahl der Anzeigezeilen; das Backend begrenzt die Werte.
  $effect(() => {
    void petExpand(expanded, petStyle.size, rows, bubbleShown).catch((reason) => console.error(reason));
  });

  // Die Blase bleibt, bis der Nutzer sie schließt (X, Esc) oder eine neue Antwort beginnt: Eine
  // Antwort, die von selbst verschwindet, ist mitten im Lesen weg. Beim Schreiben läuft der
  // Text mit, damit das Neueste sichtbar bleibt.
  $effect(() => {
    void bubbleText;
    if (avatar.streaming && bubbleScroll) bubbleScroll.scrollTop = bubbleScroll.scrollHeight;
  });

  // Systemwerte nur messen, solange das Pet sichtbar ist und etwas davon angezeigt wird.
  const wantsStats = $derived(petStyle.show_cpu || petStyle.show_ram || petStyle.show_storage);
  $effect(() => {
    if (!wantsStats || mainOpen) { stats = null; return; }
    let cancelled = false;
    const measure = async () => {
      try { const next = await systemStats(); if (!cancelled) stats = next; }
      catch (reason) { console.error(reason); }
    };
    void measure();
    const timer = setInterval(measure, petStyle.refresh_secs * 1000);
    return () => { cancelled = true; clearInterval(timer); };
  });

  const voiceBlocked = $derived(voice.status ? !voice.status.available : false);

  onMount(async () => {
    document.documentElement.classList.add("v-pet-root");
    try { setLanguage(await getUiLanguage()); } catch (reason) { console.error(reason); }
    try {
      const snapshot = await settingsSnapshot();
      let effects: "full" | "calm" = "full";
      const saved = await loadUiState("ui.effects");
      if (saved === "calm" || saved === "full") effects = saved;
      applyAppearance(snapshot.theme, effects);
    } catch (reason) { console.error(reason); }
    void initConversationStore();
    void initAvatarStore();
    void initVoiceStore();
    void initPetStyle();
    void initThemeColors();
    void initJobsStore();
    unlisten.push(await onUiLanguageChanged((code) => setLanguage(code)));
    unlisten.push(await onWindowMode((mode) => { mainOpen = mode === "main"; }));
    unlisten.push(await onPetOpenPanel(() => openPanel()));
    unlisten.push(await subscribeToStream((event) => {
      if (event.kind === "started") { liveId = event.assistant_message_id; live = true; answer = ""; error = null; bubbleOpen = true; void setActive(event.conversation_id); }
      else if (event.kind === "delta" && event.assistant_message_id === liveId) answer += event.text;
      else if (event.kind === "finished" && event.assistant_message_id === liveId) { answer = event.outcome.text; live = false; }
      else if (event.kind === "failed") { error = event.message; bubbleOpen = true; live = false; }
    }));
    await loadLastAnswer();
  });
  onDestroy(() => { for (const stop of unlisten) stop(); });

  $effect(() => {
    void conversation.activeId;
    if (!avatar.streaming && !live) void loadLastAnswer();
  });

  async function loadLastAnswer() {
    if (live) return;
    const id = conversation.activeId;
    if (!id) { answer = ""; return; }
    try {
      const detail = await openConversation(id);
      answer = [...detail.messages].reverse().find((message) => message.role === "assistant")?.content ?? "";
    } catch (reason) {
      console.error(reason);
    }
  }

  async function send(event?: Event) {
    event?.preventDefault();
    const content = draft.trim();
    if (!content || avatar.streaming) return;
    error = null;
    try {
      await sendMessage({ conversation_id: conversation.activeId, content, thinking_level: "standard" });
      draft = "";
    } catch (reason) {
      error = reason;
    }
  }

  // Im Pet wird Gesagtes sofort beantwortet: kein Entwurf, kein Senden-Knopf.
  function dictated(text: string) {
    draft = text;
    void send();
  }

  async function talk() {
    panelOpen = true;
    voice.conversationMode = true;
    await startListening();
  }

  // Ein Klick (und damit auch ein Doppelklick) öffnet nur das Schreibfeld und schließt es nie:
  // Früher öffnete jeder Klick das Menü, der zweite Klick schloss es wieder, und das Fenster
  // wuchs und schrumpfte im Takt. Das Menü bleibt auf Rechtsklick und Tastatur.
  function openPanel() {
    menu = null;
    bubbleOpen = false;
    panelOpen = true;
  }

  $effect(() => {
    if (panelOpen) void tick().then(() => document.getElementById("v-pet-input")?.focus());
  });

  function openMenu(event: MouseEvent | KeyboardEvent, keyboard = false) {
    event.preventDefault();
    if (keyboard) menu = { x: 12, y: 12 };
    else if (event instanceof MouseEvent) menu = { x: Math.max(8, event.clientX - 180), y: Math.max(8, event.clientY - 200) };
  }

  const menuItems = $derived<MenuItem[]>([
    { id: "open", label: tk("IAP öffnen"), run: () => void showMainWindow() },
    { id: "write", label: tk("Nachricht schreiben"), run: () => { panelOpen = true; } },
    { id: "talk", label: tk("Mit mir reden"), disabledReason: voiceBlocked ? (voice.blockReason ?? tk("Sprache ist nicht verfügbar")) : null, run: () => void talk() },
    { id: "look", label: tk("Bildschirm ansehen"), run: () => (lookOpen = true) },
    { id: "customize", label: tk("Pet anpassen"), run: () => { customizeOpen = true; } },
    { id: "stop", label: tk("Sprachsitzung stoppen"), disabledReason: voice.conversationMode || voice.state !== "idle" ? null : tk("Keine Sprachsitzung aktiv"), separated: true, run: () => void endConversationMode() },
    { id: "mute", label: voice.muted ? tk("Stummschaltung aufheben") : tk("Sprachausgabe stummschalten"), run: () => void setMuted(!voice.muted) },
    { id: "quit", label: tk("IAP vollständig beenden"), danger: true, separated: true, run: () => void requestQuit() },
  ]);

  async function looked(result: ScreenAnswer) {
    lookOpen = false;
    panelOpen = true;
    await setActive(result.conversation_id);
    await loadLastAnswer();
  }
</script>

<main class="v-pet" style={`--v-pet-size:${petStyle.size}px`} aria-label={t("IAP Pet")}>
  {#if customizeOpen}
    <section class="v-pet-panel v-pet-custom" aria-label={t("Pet anpassen")}>
      <header>
        <strong>{t("Pet anpassen")}</strong>
        <button type="button" class="v-btn-icon" aria-label={t("Panel schließen")} onclick={() => { customizeOpen = false; }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M5 5l14 14M19 5 5 19"/></svg>
        </button>
      </header>
      <div class="v-pet-custom-body"><PetCustomizer compact /></div>
    </section>
  {/if}
  {#if panelOpen}
    <section class="v-pet-panel" aria-label={t("Unterhaltung")}>
      <header>
        <strong>{t(STATE_TEXT[mood])}</strong>
        <button type="button" class="v-btn-icon" aria-label={t("Panel schließen")} onclick={() => { panelOpen = false; }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M5 5l14 14M19 5 5 19"/></svg>
        </button>
      </header>
      {#if voice.error}<p class="v-pet-error" role="alert">{voice.error}</p>{/if}
      {#if answer}<div class="v-pet-answer" aria-live="polite"><Markdown content={answer} isStreaming={avatar.streaming} /></div>{/if}
      {#if error}<p class="v-pet-error" role="alert">{friendlyError(error).message}</p>{/if}
      <form onsubmit={send}>
        <label class="sr-only" for="v-pet-input">{t("Nachricht")}</label>
        <textarea id="v-pet-input" bind:value={draft} rows="2" placeholder={t("Nachricht schreiben oder diktieren …")}
          onkeydown={(event) => { if (event.key === "Enter" && !event.shiftKey && !event.isComposing) void send(event); }}></textarea>
        <div class="v-pet-row">
          <MicButton compact autoStop onText={dictated} />
          {#if avatar.streaming}
            <button type="button" class="v-btn v-btn-ghost" onclick={() => { void cancelStream(); markStopped(); }}>{t("Stopp")}</button>
          {:else}
            <button type="submit" class="v-btn v-btn-primary" disabled={!draft.trim()}>{t("Senden")}</button>
          {/if}
        </div>
      </form>
    </section>
  {/if}

{#if bubbleShown}
    <div class="v-pet-bubble" class:err={!!error} style={`--v-pet-size:${petStyle.size}px`} role="status" aria-live="polite">
      <button type="button" class="v-pet-bubble-close v-btn-icon" aria-label={t("Antwort schließen")} onclick={() => { bubbleOpen = false; }}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M5 5l14 14M19 5 5 19"/></svg>
      </button>
      <div class="v-pet-bubble-text" bind:this={bubbleScroll}>
        {#if bubbleText}
          {#if error}<p>{bubbleText}</p>{:else}<Markdown content={bubbleText} isStreaming={avatar.streaming} />{/if}
        {:else}<p class="wait">{t(STATE_TEXT[mood])}</p>{/if}
      </div>
      {#if bubbleText && !avatar.streaming}
        <button type="button" class="v-pet-bubble-more" onclick={openPanel}>{t("Weiterschreiben")}</button>
      {/if}
    </div>
  {/if}

  <div class="v-pet-figure">
    <div class="v-pet-grip" data-tauri-drag-region title={t("Zum Verschieben ziehen")}></div>
    <button type="button" class="v-pet-avatar" bind:this={avatarButton} aria-haspopup="menu" aria-label={t("Avatar: {status}. Klicken zum Schreiben, Rechtsklick für das Menü.", { status: t(STATE_TEXT[mood]) })}
      onclick={(event) => { if (event.detail === 0) openMenu(event, true); else openPanel(); }} oncontextmenu={(event) => openMenu(event)}
      onkeydown={(event) => { if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) openMenu(event, true); }}>
      <Bloub avatarState={mood} size={petStyle.size} shape={petStyle.shape} color={petStyle.color} paused={mainOpen} />
    </button>
    <p class="v-pet-status" data-tauri-drag-region aria-live="polite">{t(STATE_TEXT[mood])}</p>
    {#if rows > 0}<PetStats style={petStyle} {stats} {task} />{/if}
  </div>
</main>

<svelte:window onkeydown={(event) => { if (event.key === "Escape" && bubbleShown) bubbleOpen = false; }} />
{#if menu}<AvatarMenu x={menu.x} y={menu.y} items={menuItems} onClose={() => { menu = null; avatarButton?.focus(); }} />{/if}
{#if lookOpen}<ScreenLookDialog conversationId={conversation.activeId} onClose={() => (lookOpen = false)} onDone={looked} />{/if}

<style>
  :global(html.v-pet-root), :global(html.v-pet-root body) { background: transparent !important; overflow: hidden; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  .v-pet { position: fixed; inset: 0; display: flex; flex-direction: column; align-items: flex-end; justify-content: flex-end; gap: .5rem; padding: .35rem; }
  .v-pet-figure { display: grid; justify-items: center; gap: .15rem; width: calc(var(--v-pet-size, 124px) + 1.9rem); padding: .35rem .4rem .5rem; border-radius: 1.4rem; background: color-mix(in oklab, var(--v-surface-solid) 78%, transparent); backdrop-filter: blur(10px); border: 1px solid var(--v-line); box-shadow: 0 10px 28px rgb(var(--v-shade) / .3); }
  .v-pet-grip { width: 2.4rem; height: .3rem; border-radius: 99px; background: rgb(var(--v-tint) / .25); cursor: grab; }
  .v-pet-avatar { padding: 0; border: 0; border-radius: 1.2rem; background: transparent; cursor: pointer; }
  .v-pet-avatar:focus-visible { outline: 2px solid var(--v-focus-ring); outline-offset: 3px; }
  .v-pet-status { margin: 0; color: var(--v-text-secondary); font-size: var(--v-text-xs); font-weight: 560; }
  .v-pet-panel { display: grid; gap: .5rem; width: 100%; max-height: calc(100dvh - 12rem); padding: .7rem; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); box-shadow: var(--v-shadow-lg); overflow: hidden; }
  .v-pet-custom { max-height: calc(100dvh - 8rem); }
  .v-pet-custom-body { min-height: 0; overflow-y: auto; padding-right: 2px; }
  .v-pet-panel header { display: flex; align-items: center; justify-content: space-between; font-size: var(--v-text-sm); color: var(--v-text-primary); }
  .v-pet-answer { min-height: 0; max-height: 12rem; overflow-y: auto; font-size: var(--v-text-sm); }
  .v-pet-bubble { position: relative; width: 100%; max-height: 8.5rem; display: grid; grid-template-rows: minmax(0, 1fr) auto; gap: .25rem; padding: .55rem 1.8rem .45rem .75rem; margin-bottom: .35rem; border: 1px solid var(--v-line-strong); border-radius: 1rem; background: var(--v-surface-solid); color: var(--v-text-primary); box-shadow: 0 8px 22px rgb(var(--v-shade) / .28); font-size: var(--v-text-sm); line-height: 1.4; transform-origin: calc(100% - var(--v-pet-size) / 2 - .9rem) 100%; animation: v-bubble-in .16s ease-out; }
  /* Spitze zeigt auf die Figur: gedrehtes Quadrat, das den Rand der Blase nach unten fortsetzt. */
  .v-pet-bubble::after { content: ""; position: absolute; bottom: -.36rem; right: calc(var(--v-pet-size) / 2 + .55rem); width: .65rem; height: .65rem; background: var(--v-surface-solid); border-right: 1px solid var(--v-line-strong); border-bottom: 1px solid var(--v-line-strong); transform: rotate(45deg); }
  .v-pet-bubble.err { border-color: color-mix(in oklab, var(--v-danger) 55%, var(--v-line-strong)); }
  .v-pet-bubble.err::after { border-color: color-mix(in oklab, var(--v-danger) 55%, var(--v-line-strong)); }
  .v-pet-bubble-text { min-height: 0; overflow-y: auto; overscroll-behavior: contain; }
  .v-pet-bubble-text p { margin: 0; }
  .v-pet-bubble.err .v-pet-bubble-text { color: var(--v-danger); }
  .v-pet-bubble .wait { color: var(--v-text-secondary); }
  .v-pet-bubble-close { position: absolute; top: .3rem; right: .3rem; width: 1.4rem; height: 1.4rem; }
  .v-pet-bubble-more { justify-self: end; padding: 0; border: 0; background: transparent; color: var(--v-accent-blue); font-size: var(--v-text-xs); font-weight: 560; cursor: pointer; }
  .v-pet-bubble-more:hover { text-decoration: underline; }
  @keyframes v-bubble-in { from { opacity: 0; transform: translateY(4px) scale(.96); } to { opacity: 1; transform: none; } }
  @media (prefers-reduced-motion: reduce) { .v-pet-bubble { animation: none; } }
  .v-pet-error { margin: 0; color: var(--v-danger); font-size: var(--v-text-xs); }
  .v-pet-panel form { display: grid; gap: .4rem; }
  .v-pet-panel textarea { min-height: 2.8rem; resize: none; }
  .v-pet-row { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: .4rem; }
</style>
