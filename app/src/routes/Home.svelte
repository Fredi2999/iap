<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { t, tk } from "../lib/i18n/index.svelte";
  import { cancelStream, openConversation, sendMessage, subscribeToStream } from "../lib/ipc";
  import { friendlyError } from "../lib/errors";
  import { STATE_TEXT } from "../lib/avatar";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import AvatarMenu, { type MenuItem } from "../lib/components/AvatarMenu.svelte";
  import Bloub from "../lib/components/Bloub.svelte";
  import Markdown from "../lib/components/Markdown.svelte";
  import MicButton from "../lib/components/MicButton.svelte";
  import ScreenLookDialog from "../lib/components/ScreenLookDialog.svelte";
  import HomeOverview from "../lib/components/HomeOverview.svelte";
  import { avatar, currentAvatarState, markStopped } from "../lib/stores/avatar.svelte";
  import { conversation, drafts, setActive } from "../lib/stores/conversation.svelte";
  import { initPetStyle, petStyle } from "../lib/stores/petStyle.svelte";
  import { endConversationMode, setMuted, startListening, voice } from "../lib/stores/voice.svelte";
  import type { ScreenAnswer } from "../lib/types";

  // Startseite nach dem Entsperren: der Avatar zeigt den echten Zustand, darunter
  // Sprach- und Texteingabe. Hauptseite, Chat und Pet teilen dieselbe
  // Unterhaltung. Ein Klick auf den Avatar startet nichts; Mikrofon und
  // Werkzeuge haben beschriftete Knöpfe.
  interface Props {
    /** Hauptfenster nicht sichtbar (Pet-Modus): Animation ruht. */
    paused?: boolean;
    onOpenChat: () => void;
    onOpenFlow: () => void;
    onQuit: () => void;
    /** Öffnet eine Route (für die Abkürzungen der Übersicht). */
    onOpenRoute: (route: "calendar" | "connectors") => void;
  }
  let { paused = false, onOpenChat, onOpenFlow, onQuit, onOpenRoute }: Props = $props();

  const mood = $derived(currentAvatarState());
  let answer = $state("");
  let answerMeta = $state<string | null>(null);
  let liveAssistantId: string | null = null;
  let error = $state<unknown>(null);
  let menu = $state<{ x: number; y: number } | null>(null);
  let lookOpen = $state(false);
  let unlisten: UnlistenFn | null = null;
  let stage = $state<HTMLElement | null>(null);
  let composer = $state<HTMLTextAreaElement | null>(null);

  const busy = $derived(avatar.streaming);
  const statusDetail = $derived.by(() => {
    if (voice.error) return voice.error;
    if (voice.state === "idle" && voice.status && !voice.status.available && voice.blockReason) return voice.blockReason;
    if (voice.conversationMode) return t("Sprachsitzung aktiv: Antworten werden vorgelesen.");
    return "";
  });

  onMount(async () => {
    void initPetStyle();
    unlisten = await subscribeToStream((event) => {
      if (event.kind === "started") {
        liveAssistantId = event.assistant_message_id;
        answer = "";
        answerMeta = null;
        void setActive(event.conversation_id);
      } else if (event.kind === "delta" && event.assistant_message_id === liveAssistantId) {
        answer += event.text;
      } else if (event.kind === "finished" && event.assistant_message_id === liveAssistantId) {
        answer = event.outcome.text;
      } else if (event.kind === "failed") {
        error = event.message;
      }
    });
    await loadLastAnswer();
  });
  onDestroy(() => unlisten?.());

  // Wechselt die gemeinsame Unterhaltung (Chat, Pet, Bildschirm), zeigt Home deren letzte Antwort.
  $effect(() => {
    void conversation.activeId;
    if (!avatar.streaming) void loadLastAnswer();
  });

  async function loadLastAnswer() {
    const id = conversation.activeId;
    if (!id) {
      answer = "";
      answerMeta = null;
      return;
    }
    try {
      const detail = await openConversation(id);
      const last = [...detail.messages].reverse().find((message) => message.role === "assistant");
      answer = last?.content ?? "";
      answerMeta = detail.conversation.title;
    } catch (reason) {
      console.error(reason);
    }
  }

  async function send(event?: Event) {
    event?.preventDefault();
    const content = drafts.home.trim();
    if (!content || busy) return;
    error = null;
    try {
      await sendMessage({ conversation_id: conversation.activeId, content, thinking_level: "standard" });
      drafts.home = "";
    } catch (reason) {
      error = reason;
    }
  }

  async function stop() {
    try { await cancelStream(); markStopped(); } catch (reason) { error = reason; }
  }

  function onComposerKey(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) void send(event);
  }

  // In der Sprachsitzung wird Gesagtes sofort beantwortet; sonst bleibt es ein Entwurf zum Bearbeiten.
  function dictated(text: string) {
    if (voice.conversationMode) {
      drafts.home = text;
      void send();
      return;
    }
    drafts.home = drafts.home ? `${drafts.home} ${text}` : text;
    composer?.focus();
  }

  async function talk() {
    voice.conversationMode = true;
    await startListening();
  }

  async function stopSession() {
    await endConversationMode();
  }

  function openMenu(event: MouseEvent | KeyboardEvent, keyboard = false) {
    event.preventDefault();
    if (keyboard && stage) {
      const box = stage.getBoundingClientRect();
      menu = { x: box.left + box.width / 2, y: box.top + box.height / 2 };
    } else if (event instanceof MouseEvent) {
      menu = { x: event.clientX, y: event.clientY };
    }
  }

  function stageKey(event: KeyboardEvent) {
    if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) openMenu(event, true);
  }

  const voiceBlocked = $derived(voice.status ? !voice.status.available : false);
  const menuItems = $derived<MenuItem[]>([
    { id: "talk", label: tk("Mit mir reden"), disabledReason: voiceBlocked ? (voice.blockReason ?? tk("Sprache ist nicht verfügbar")) : null, run: () => void talk() },
    { id: "look", label: tk("Bildschirm ansehen"), run: () => (lookOpen = true) },
    { id: "stop", label: tk("Sprachsitzung stoppen"), disabledReason: voice.conversationMode || voice.state !== "idle" ? null : tk("Keine Sprachsitzung aktiv"), separated: true, run: () => void stopSession() },
    { id: "mute", label: voice.muted ? tk("Stummschaltung aufheben") : tk("Sprachausgabe stummschalten"), run: () => void setMuted(!voice.muted) },
    { id: "quit", label: tk("IAP vollständig beenden"), danger: true, separated: true, run: onQuit },
  ]);

  async function looked(result: ScreenAnswer) {
    lookOpen = false;
    await setActive(result.conversation_id);
    await loadLastAnswer();
  }
</script>

<section class="v-home" aria-labelledby="v-home-title">
  <h1 id="v-home-title" class="sr-only">{t("Startseite")}</h1>

  <div class="v-home-stage">
    <!-- Der Avatar selbst startet nichts; er öffnet nur das Menü mit den beschrifteten Aktionen. -->
    <button type="button" class="v-home-avatar" bind:this={stage} aria-haspopup="menu" aria-label={t("Avatar: {status}. Menü öffnen.", { status: t(STATE_TEXT[mood]) })}
      onclick={(event) => openMenu(event, event.detail === 0)} oncontextmenu={(event) => openMenu(event)} onkeydown={stageKey}>
      <Bloub avatarState={mood} size={Math.max(248, petStyle.size)} shape={petStyle.shape} color={petStyle.color} {paused} />
    </button>
    <p class="v-home-status" aria-live="polite"><strong>{t(STATE_TEXT[mood])}</strong>{#if statusDetail}<span>{statusDetail}</span>{/if}</p>
    <div class="v-home-actions">
      <button type="button" class="v-btn v-btn-primary" aria-pressed={voice.conversationMode} disabled={voiceBlocked || voice.state !== "idle"} title={voiceBlocked ? (voice.blockReason ?? undefined) : undefined}
        onclick={() => (voice.conversationMode ? void stopSession() : void talk())}>{t(voice.conversationMode ? "Sprachsitzung beenden" : tk("Mit mir reden"))}</button>
      <MicButton onText={dictated} />
      <button type="button" class="v-btn v-btn-ghost" onclick={() => (lookOpen = true)}>{t(tk("Bildschirm ansehen"))}</button>
      {#if voice.muted}<button type="button" class="v-btn v-btn-ghost" onclick={() => setMuted(false)}>{t(tk("Stummschaltung aufheben"))}</button>{/if}
    </div>
    <HomeOverview onOpen={onOpenRoute} />
  </div>

  <div class="v-home-panel">
    {#if answer}
      <article class="v-home-answer" aria-live="polite" aria-label={t("Letzte Antwort")}>
        {#if answerMeta}<small>{answerMeta}</small>{/if}
        <Markdown content={answer} isStreaming={busy} />
      </article>
    {:else}
      <p class="v-home-empty">{t("Sprich mit IAP oder schreibe unten eine Frage. Die Unterhaltung erscheint auch im Chat.")}</p>
    {/if}
    {#if error}<p class="v-home-error" role="alert">{friendlyError(error).message}</p>{/if}
    <form class="v-home-compose" onsubmit={send}>
      <label class="sr-only" for="v-home-input">{t("Nachricht")}</label>
      <textarea id="v-home-input" bind:this={composer} bind:value={drafts.home} rows="2" placeholder={t("Nachricht schreiben oder diktieren …")} onkeydown={onComposerKey}></textarea>
      {#if busy}
        <button type="button" class="v-btn v-btn-ghost" onclick={stop}>{t("Stopp")}</button>
      {:else}
        <button type="submit" class="v-btn v-btn-primary" disabled={!drafts.home.trim()}>{t("Senden")}</button>
      {/if}
    </form>
    <div class="v-home-links">
      <button type="button" class="v-btn v-btn-ghost" onclick={onOpenChat}>{t("Chat öffnen")}</button>
      <button type="button" class="v-btn v-btn-ghost" onclick={onOpenFlow}>{t("Flow Version")}</button>
    </div>
  </div>
</section>

{#if menu}<AvatarMenu x={menu.x} y={menu.y} items={menuItems} onClose={() => { menu = null; stage?.focus(); }} />{/if}
{#if lookOpen}<ScreenLookDialog conversationId={conversation.activeId} onClose={() => (lookOpen = false)} onDone={looked} />{/if}

<style>
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  .v-home { display: grid; grid-template-columns: minmax(16rem, 22rem) minmax(0, 1fr); gap: var(--v-space-6); align-items: center; width: min(64rem, 100%); min-height: 100%; margin: 0 auto; padding: var(--v-space-6) var(--v-space-4); }
  .v-home-stage { display: grid; justify-items: center; gap: var(--v-space-4); min-width: 0; }
  .v-home-avatar { padding: 0; border: 0; border-radius: 2rem; background: transparent; outline-offset: 6px; cursor: context-menu; }
  .v-home-avatar:focus-visible { outline: 2px solid var(--v-focus-ring); }
  .v-home-status { display: grid; justify-items: center; gap: .25rem; margin: 0; text-align: center; font-size: var(--v-text-md); }
  .v-home-status strong { color: var(--v-text-primary); font-weight: 620; }
  .v-home-status span { max-width: 26ch; color: var(--v-text-muted); font-size: var(--v-text-sm); line-height: 1.45; }
  .v-home-actions { display: flex; flex-wrap: wrap; justify-content: center; gap: var(--v-space-2); }
  .v-home-panel { display: grid; gap: var(--v-space-4); min-width: 0; }
  .v-home-answer { max-height: 46dvh; overflow-y: auto; padding: var(--v-space-4) var(--v-space-5); border: 1px solid var(--v-line); border-radius: var(--v-radius-card); background: var(--v-surface-2); }
  .v-home-answer small { display: block; margin-bottom: .4rem; color: var(--v-text-muted); font-size: var(--v-text-xs); }
  .v-home-empty { margin: 0; padding: var(--v-space-4) var(--v-space-5); border: 1px dashed var(--v-line-strong); border-radius: var(--v-radius-card); color: var(--v-text-muted); font-size: var(--v-text-sm); line-height: 1.55; }
  .v-home-error { margin: 0; color: var(--v-danger); font-size: var(--v-text-sm); }
  .v-home-compose { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--v-space-2); align-items: end; }
  .v-home-compose textarea { min-height: 3.2rem; resize: vertical; }
  .v-home-links { display: flex; flex-wrap: wrap; gap: var(--v-space-2); }
  @media (max-width: 900px) {
    .v-home { grid-template-columns: minmax(0, 1fr); align-items: start; gap: var(--v-space-4); padding-top: var(--v-space-4); }
  }
</style>
