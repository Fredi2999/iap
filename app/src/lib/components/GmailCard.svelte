<script lang="ts">
  // Gmail: Postfach lesen und automatisch an EINE Adresse antworten, im Takt und gut kontrollierbar.
  // Die Sperren (Absender, Google-Prüfung, Auto-Mails, Limits, eine Antwort je Mail) erzwingt das
  // Backend; diese Karte zeigt sie nur an und stellt die Werte ein. Das App-Passwort geht nur
  // hinein und kommt nie wieder heraus.
  import { onMount, onDestroy } from "svelte";
  import { t, tk, locale } from "../i18n/index.svelte";
  import {
    mailClearLog, mailClearPassword, mailEmergencyStop, mailLog, mailRunNow, mailSetPassword,
    mailSetPolling, mailStatus, mailTestConnection, onMailChanged,
  } from "../ipc";
  import { friendlyError } from "../errors";
  import { notify } from "../notifications";
  import type { ConnectorConfig, MailLogEntry, MailStatus } from "../types";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  interface Props {
    config: ConnectorConfig;
    /** Speichert Einstellungen über den Konnektoren-Dialog; liefert, ob es geklappt hat. */
    onSave: (patch: Partial<ConnectorConfig>) => Promise<boolean>;
  }
  let { config, onSave }: Props = $props();

  const REASONS: Record<string, string> = {
    no_message_id: tk("Mail ohne Kennung"),
    already_handled: tk("Schon beantwortet"),
    wrong_sender: tk("Anderer Absender"),
    auto_mail: tk("Automatische Mail"),
    not_authenticated: tk("Absender nicht von Google bestätigt"),
    no_text: tk("Kein Text"),
    rate_limited: tk("Limit erreicht, wartet"),
    empty_reply: tk("Modell lieferte keine Antwort"),
    stopped_air_gap: tk("Gestoppt: Air Gap eingeschaltet"),
    stopped_session: tk("Gestoppt: Tresor gesperrt"),
    emergency_stop: tk("Not-Aus ausgelöst"),
  };
  const KINDS: Record<string, string> = {
    drafted: tk("Entwurf abgelegt"), sent: tk("Gesendet"), skipped: tk("Übersprungen"), error: tk("Fehler"), info: tk("Hinweis"),
  };

  // Texte, die das Backend als deutschen Klartext liefert. Hier registriert, damit sie übersetzt werden.
  const BACKEND_TEXT = [
    tk("Air Gap ist eingeschaltet. Schalte ihn in der Seitenleiste aus."),
    tk("Gmail ist ausgeschaltet."),
    tk("Es ist kein App-Passwort hinterlegt."),
    tk("Die Einstellungen sind unvollständig."),
    tk("Mail ist auf diesem System noch nicht eingerichtet."),
  ];
  const backendText = (text: string) => (BACKEND_TEXT.includes(text) ? t(text) : text);

  let status = $state<MailStatus | null>(null);
  let log = $state<MailLogEntry[]>([]);
  let password = $state("");
  let showPassword = $state(false);
  let busy = $state<"" | "save" | "test" | "run" | "password" | "toggle">("");
  let testMessage = $state("");
  let failure = $state<string | null>(null);
  let confirmSend = $state(false);
  let understood = $state(false);
  let unlisten: UnlistenFn | null = null;
  let timer: ReturnType<typeof setInterval> | undefined;

  // Entwurf der Felder: erst „Speichern“ übernimmt sie, damit eine halbe Eingabe nichts auslöst.
  let draft = $state({
    address: "", target: "", interval: 5, perHour: 6, perDay: 30, instruction: "", allowRead: false,
  });
  let loadedFrom = "";
  $effect(() => {
    const signature = JSON.stringify([config.gmail_address, config.gmail_target_email, config.gmail_check_interval_minutes, config.gmail_max_per_hour, config.gmail_max_per_day, config.gmail_instruction, config.gmail_allow_read]);
    if (signature !== loadedFrom) {
      loadedFrom = signature;
      draft = {
        address: config.gmail_address, target: config.gmail_target_email, interval: config.gmail_check_interval_minutes,
        perHour: config.gmail_max_per_hour, perDay: config.gmail_max_per_day, instruction: config.gmail_instruction, allowRead: config.gmail_allow_read,
      };
    }
  });
  const dirty = $derived(
    draft.address !== config.gmail_address || draft.target !== config.gmail_target_email
    || draft.interval !== config.gmail_check_interval_minutes || draft.perHour !== config.gmail_max_per_hour
    || draft.perDay !== config.gmail_max_per_day || draft.instruction !== config.gmail_instruction
    || draft.allowRead !== config.gmail_allow_read,
  );

  async function refresh() {
    try {
      status = await mailStatus();
      log = await mailLog();
    } catch (reason) {
      // Vor dem Entsperren gibt es noch nichts anzuzeigen.
      console.error(reason);
    }
  }

  onMount(async () => {
    await refresh();
    unlisten = await onMailChanged(() => void refresh());
    // Die Uhr der nächsten Prüfung läuft im Backend; hier nur die Anzeige gelegentlich auffrischen.
    timer = setInterval(() => void refresh(), 15000);
  });
  onDestroy(() => { unlisten?.(); if (timer) clearInterval(timer); });

  async function guarded<T>(kind: typeof busy, work: () => Promise<T>): Promise<T | undefined> {
    busy = kind; failure = null;
    try { return await work(); }
    catch (reason) { failure = friendlyError(reason).message; return undefined; }
    finally { busy = ""; }
  }

  async function save() {
    await guarded("save", async () => {
      const ok = await onSave({
        gmail_address: draft.address.trim(), gmail_target_email: draft.target.trim(),
        gmail_check_interval_minutes: Number(draft.interval), gmail_max_per_hour: Number(draft.perHour),
        gmail_max_per_day: Number(draft.perDay), gmail_instruction: draft.instruction, gmail_allow_read: draft.allowRead,
      });
      if (ok) await refresh();
    });
  }

  // Wie bei den anderen Konnektoren: einschalten geht nur bei Air Gap aus, ausschalten immer.
  async function toggleEnabled() {
    if (config.offline_mode && !config.gmail_enabled) return;
    await guarded("toggle", async () => {
      if (await onSave({ gmail_enabled: !config.gmail_enabled })) await refresh();
    });
  }

  async function savePassword() {
    if (!password.trim()) return;
    await guarded("password", async () => {
      await mailSetPassword(password);
      password = "";
      notify("Gmail", "App-Passwort gespeichert.", "success");
      await refresh();
    });
  }

  async function removePassword() {
    await guarded("password", async () => { await mailClearPassword(); await refresh(); });
  }

  async function test() {
    const count = await guarded("test", () => mailTestConnection());
    if (count !== undefined) testMessage = t("Verbindung steht. {n} ungelesene Mails vom eingetragenen Absender.", { n: Number(count) });
    else testMessage = "";
  }

  async function runNow() {
    await guarded("run", async () => { await mailRunNow(); await refresh(); });
  }

  async function togglePolling() {
    if (!status) return;
    await guarded("toggle", async () => { await mailSetPolling(!status!.polling); await refresh(); });
  }

  async function emergency() {
    await guarded("toggle", async () => {
      await mailEmergencyStop();
      confirmSend = false;
      notify("Gmail", "Auto-Antwort gestoppt. Der Sendemodus ist wieder auf Entwurf.", "success");
      await refresh();
    });
  }

  async function chooseDraft() {
    confirmSend = false;
    understood = false;
    if (config.gmail_reply_mode === "draft") return;
    await guarded("save", async () => { await onSave({ gmail_reply_mode: "draft", gmail_send_acknowledged: false }); });
  }

  async function enableSend() {
    if (!understood) return;
    const ok = await guarded("save", () => onSave({ gmail_reply_mode: "send", gmail_send_acknowledged: true }));
    if (ok) { confirmSend = false; understood = false; }
  }

  async function clearLog() {
    await guarded("run", async () => { await mailClearLog(); await refresh(); });
  }

  const formatTime = (ms: number) => new Date(ms).toLocaleString(locale(), { dateStyle: "short", timeStyle: "short" });
  const detailText = (entry: MailLogEntry) => REASONS[entry.detail] ? t(REASONS[entry.detail]) : entry.detail;
  const needsConnection = $derived(config.offline_mode ? "Der Test braucht Air Gap aus." : !status?.has_password ? "Trage zuerst das App-Passwort ein." : !config.gmail_address ? "Trage zuerst dein Gmail-Konto ein." : null);
</script>

<section class="v-card v-stack gmail">
    <div class="flex flex-wrap items-start justify-between gap-4 border-b g-border pb-4">
      <div>
        <h3 class="text-base font-semibold g-primary">{t("Gmail")}</h3>
        <p class="text-xs g-muted">{t("Postfach lesen und automatisch an eine Adresse antworten")}</p>
      </div>
      <div class="flex items-center gap-3">
        <span class="text-xs g-muted">{t(config.gmail_enabled && !config.offline_mode ? "Aktiv" : config.offline_mode ? "Gesperrt (Air Gap)" : "Deaktiviert")}</span>
        <button type="button" class={`v-apple-switch ${config.gmail_enabled && !config.offline_mode ? "active" : ""}`} onclick={toggleEnabled}
          aria-pressed={config.gmail_enabled && !config.offline_mode} aria-label={t("{name} aktivieren", { name: t("Gmail") })}
          disabled={busy !== "" || (config.offline_mode && !config.gmail_enabled)}>
          <span class="v-apple-thumb"></span>
        </button>
      </div>
    </div>

    <p class="text-xs g-secondary leading-relaxed">{t("IAP prüft dein Postfach im Takt. Antworten entstehen lokal, standardmäßig als Entwurf. Daten gehen nur zu Google (imap.gmail.com, smtp.gmail.com) und nur bei Air Gap aus.")}</p>
    {#if status && !status.supported}
      <p class="v-notice warn">{t("Mail ist hier noch nicht eingerichtet. Die Google-Verbindung gibt es nur unter Windows.")}</p>
    {/if}

    <details class="gmail-surface" open={!status?.has_password}>
      <summary class="text-xs font-medium g-secondary cursor-pointer">{t("Einrichtung in vier Schritten")}</summary>
      <ol class="text-xs g-muted leading-relaxed list-decimal pl-5 mt-3 space-y-1.5">
        <li>{t("Aktiviere in deinem Google-Konto die Bestätigung in zwei Schritten (Bereich Sicherheit).")}</li>
        <li>{t("Erstelle unter „App-Passwörter“ eines mit dem Namen IAP (16 Buchstaben).")}</li>
        <li>{t("Trage unten Konto und App-Passwort ein. Es liegt verschlüsselt im Tresor.")}</li>
        <li>{t("Schalte Air Gap aus, klicke „Verbindung testen“ und trage die Antwort-Adresse ein.")}</li>
      </ol>
    </details>

    <div class="grid gap-4 md:grid-cols-2">
      <label class="space-y-1.5">
        <span class="text-xs g-secondary font-medium">{t("Dein Gmail-Konto")}</span>
        <input type="email" class="v-input-glass w-full text-xs" placeholder="name@gmail.com" bind:value={draft.address} autocomplete="off" />
      </label>
      <div class="space-y-1.5">
        <label for="gmail-pw" class="text-xs g-secondary font-medium">{t("App-Passwort")}</label>
        <div class="flex gap-2">
          <input id="gmail-pw" type={showPassword ? "text" : "password"} class="v-input-glass flex-1 min-w-0 font-mono text-xs" autocomplete="off"
            placeholder={status?.has_password ? t("gespeichert (nicht sichtbar)") : "abcd efgh ijkl mnop"} bind:value={password}
            onkeydown={(event) => { if (event.key === "Enter") void savePassword(); }} />
          <button type="button" class="v-btn v-btn-ghost text-xs" onclick={() => (showPassword = !showPassword)}>{t(showPassword ? "Ausblenden" : "Anzeigen")}</button>
        </div>
        <div class="flex flex-wrap gap-2">
          <button type="button" class="v-btn v-btn-primary text-xs" onclick={savePassword} disabled={!password.trim() || busy !== ""}>{t("Passwort speichern")}</button>
          {#if status?.has_password}<button type="button" class="v-btn v-btn-ghost text-xs" onclick={removePassword} disabled={busy !== ""}>{t("Passwort entfernen")}</button>{/if}
        </div>
      </div>
      <label class="space-y-1.5">
        <span class="text-xs g-secondary font-medium">{t("IAP antwortet nur dieser Adresse")}</span>
        <input type="email" class="v-input-glass w-full text-xs" placeholder="du@beispiel.at" bind:value={draft.target} autocomplete="off" />
        <span class="v-help">{t("Andere Absender werden ignoriert. Nicht von Google bestätigte gelten als gefälscht.")}</span>
      </label>
      <label class="space-y-1.5">
        <span class="text-xs g-secondary font-medium">{t("Prüfen alle {n} Minuten", { n: draft.interval })}</span>
        <input type="range" min="1" max="60" step="1" bind:value={draft.interval} class="w-full" />
      </label>
      <label class="space-y-1.5">
        <span class="text-xs g-secondary font-medium">{t("Höchstens pro Stunde")}</span>
        <input type="number" min="1" max="60" class="v-input-glass w-full text-xs" bind:value={draft.perHour} />
      </label>
      <label class="space-y-1.5">
        <span class="text-xs g-secondary font-medium">{t("Höchstens pro Tag")}</span>
        <input type="number" min="1" max="200" class="v-input-glass w-full text-xs" bind:value={draft.perDay} />
      </label>
    </div>

    <label class="block space-y-1.5">
      <span class="text-xs g-secondary font-medium">{t("Anweisung für die Antworten (optional)")}</span>
      <textarea rows="2" maxlength="1000" class="v-input-glass w-full text-xs" bind:value={draft.instruction} placeholder={t("Z. B. Antworte freundlich und kurz auf Deutsch.")}></textarea>
    </label>

    <label class="flex items-start gap-2 text-xs g-muted">
      <input type="checkbox" bind:checked={draft.allowRead} class="mt-0.5" />
      <span>{t("Chat und Workflows dürfen mein Postfach lesen (nur lesend). Mails bleiben privat und gehen nie an Websuche oder andere Dienste.")}</span>
    </label>

    <div class="flex flex-wrap gap-3">
      <button type="button" class="v-btn v-btn-primary text-xs" onclick={save} disabled={!dirty || busy !== ""}>{t(busy === "save" ? "Speichere …" : "Einstellungen speichern")}</button>
      <button type="button" class="v-btn v-btn-ghost text-xs" onclick={test} disabled={needsConnection !== null || busy !== "" || dirty}>{t(busy === "test" ? "Teste …" : "Verbindung testen")}</button>
    </div>
    {#if needsConnection}<p class="v-help">{t(needsConnection)}</p>{/if}
    {#if testMessage}<p class="v-notice" role="status">{testMessage}</p>{/if}
    {#if failure}<p class="v-notice warn" role="alert">{t(failure)}</p>{/if}

    <fieldset class="space-y-3 border-t g-border pt-4">
      <legend class="text-xs g-secondary font-medium">{t("Was IAP mit der Antwort tut")}</legend>
      <label class="flex items-start gap-2 text-xs g-secondary">
        <input type="radio" name="gmail-mode" checked={config.gmail_reply_mode === "draft" && !confirmSend} onchange={chooseDraft} class="mt-0.5" />
        <span><strong>{t("Als Entwurf ablegen (empfohlen)")}</strong><br /><span class="g-muted">{t("Liegt im Entwürfe-Ordner von Gmail. Du sendest selbst.")}</span></span>
      </label>
      <label class="flex items-start gap-2 text-xs g-secondary">
        <input type="radio" name="gmail-mode" checked={config.gmail_reply_mode === "send" || confirmSend} onchange={() => (confirmSend = true)} class="mt-0.5" />
        <span><strong>{t("Automatisch senden")}</strong><br /><span class="g-muted">{t("Geht sofort an die eingetragene Adresse, ohne dass du sie siehst.")}</span></span>
      </label>
      {#if confirmSend && config.gmail_reply_mode !== "send"}
        <div class="v-notice warn space-y-2" role="alertdialog" aria-label={t("Automatisches Senden bestätigen")}>
          <p>{t("Achtung: Das Modell kann sich irren oder durch den Mailtext getäuscht werden. Gesendete Antworten lassen sich nicht zurückholen. Die Sperren (ein Empfänger, Limits, keine Auto-Mails) bleiben.")}</p>
          <label class="flex items-start gap-2"><input type="checkbox" bind:checked={understood} class="mt-0.5" /> <span>{t("Verstanden: IAP soll automatisch senden.")}</span></label>
          <div class="flex gap-2">
            <button type="button" class="v-btn v-btn-primary text-xs" onclick={enableSend} disabled={!understood || busy !== ""}>{t("Automatisch senden aktivieren")}</button>
            <button type="button" class="v-btn v-btn-ghost text-xs" onclick={() => { confirmSend = false; understood = false; }}>{t("Abbrechen")}</button>
          </div>
        </div>
      {/if}
      {#if config.gmail_reply_mode === "send"}<p class="v-notice warn">{t("Automatisches Senden ist AN.")}</p>{/if}
    </fieldset>

    <div class="space-y-3 border-t g-border pt-4">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div>
          <div class="flex items-center gap-2">
            <span class="text-xs font-semibold g-primary">{t("Auto-Antwort")}</span>
            <span class="v-chip" class:accent={status?.polling}>{t(status?.polling ? (status.running ? "Prüft gerade" : "Läuft") : "Aus")}</span>
          </div>
          <p data-hint class="v-help mt-1">{t("Startet nach jedem Entsperren aus. Läuft auch im Pet-Modus.")}</p>
        </div>
        <div class="flex items-center gap-3">
          <button type="button" class={`v-apple-switch ${status?.polling ? "active" : ""}`} onclick={togglePolling}
            disabled={busy !== "" || (!status?.polling && status?.blocked_reason != null)} aria-pressed={status?.polling ?? false} aria-label={t("Auto-Antwort")}>
            <span class="v-apple-thumb"></span>
          </button>
          <button type="button" class="v-btn text-xs v-btn-danger" onclick={emergency} disabled={busy !== ""}>{t("Not-Aus")}</button>
        </div>
      </div>
      {#if status?.blocked_reason && !status.polling}<p class="v-help">{backendText(status.blocked_reason)}</p>{/if}
      {#if status}
        <p class="v-help">
          {t("Heute {n} Antworten, in der letzten Stunde {m}.", { n: status.sent_today, m: status.sent_last_hour })}
          {#if status.last_run_unix_ms} · {t("Zuletzt geprüft: {time}", { time: formatTime(status.last_run_unix_ms) })}{/if}
          {#if status.polling && status.next_run_unix_ms} · {t("Nächste Prüfung: {time}", { time: formatTime(status.next_run_unix_ms) })}{/if}
        </p>
        {#if status.last_error}<p class="v-notice warn" role="status">{backendText(status.last_error)}</p>{/if}
      {/if}
      <div class="flex gap-2">
        <button type="button" class="v-btn v-btn-ghost text-xs" onclick={runNow} disabled={busy !== "" || status?.blocked_reason != null}>{t(busy === "run" ? "Prüfe …" : "Jetzt einmal prüfen")}</button>
      </div>
    </div>

    <div class="space-y-2 border-t g-border pt-4">
      <div class="flex items-center justify-between">
        <span class="text-xs font-semibold g-primary">{t("Protokoll")}</span>
        {#if log.length > 0}<button type="button" class="text-xs g-muted g-hover" onclick={clearLog}>{t("Protokoll leeren")}</button>{/if}
      </div>
      {#if log.length === 0}
        <p class="v-help">{t("Noch nichts passiert.")}</p>
      {:else}
        <ul class="space-y-1.5 max-h-72 overflow-y-auto">
          {#each log as entry, index (index)}
            <li class="flex flex-wrap items-baseline gap-x-3 gap-y-0.5 text-xs">
              <span class="g-muted tabular-nums">{formatTime(entry.at_unix_ms)}</span>
              <span class="font-medium" style={entry.kind === "error" ? "color: var(--v-danger)" : ""}>{t(KINDS[entry.kind])}</span>
              {#if entry.subject}<span class="g-secondary truncate max-w-[16rem]">{entry.subject}</span>{/if}
              {#if entry.detail}<span class="g-muted">{detailText(entry)}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
</section>

<style>
  /* Farben über die Design-Tokens, damit die Karte zum Rest der App passt und im hellen
     Modus stimmt (vorher feste Dark-Mode-Werte text-white/zinc/border-white). */
  .gmail :global(.g-primary) { color: var(--v-text-primary); }
  .gmail :global(.g-secondary) { color: var(--v-text-secondary); }
  .gmail :global(.g-muted) { color: var(--v-text-muted); }
  .gmail :global(.g-border) { border-color: var(--v-line) !important; }
  .gmail :global(.g-hover:hover) { color: var(--v-text-primary); }
  .gmail-surface { padding: var(--v-space-4); border-radius: var(--v-radius-control); background: var(--v-surface-2); border: 1px solid var(--v-line); }
</style>
