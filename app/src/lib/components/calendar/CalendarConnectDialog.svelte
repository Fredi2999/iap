<script lang="ts">
  // Einen Kalender verbinden: Apple (iCloud) mit Apple-ID und app-spezifischem Passwort, oder Google
  // über die geheime iCal-Adresse. Es wird nichts abgerufen, bevor du hier bestätigst, und bei
  // eingeschaltetem Air Gap geht gar nichts. Passwort und Adresse gehen nur hinein und nie zurück.
  import { t } from "../../i18n/index.svelte";
  import { calendarAddGoogle, calendarAddIcloud, calendarIcloudDiscover, calendarSync } from "../../ipc";
  import { calendarError, localOffsetMinutes } from "../../calendarItems";
  import type { CalendarRef } from "../../types";
  import Modal from "./Modal.svelte";

  interface Props {
    airGap: boolean;
    /** Nach dem Verbinden (und dem ersten Abruf). */
    onDone: () => void;
    onClose: () => void;
  }
  let { airGap, onDone, onClose }: Props = $props();

  type Step = "choose" | "apple" | "apple-pick" | "google";
  let step = $state<Step>("choose");
  let busy = $state(false);
  let failure = $state<string | null>(null);

  let appleId = $state("");
  let appPassword = $state("");
  let found = $state<CalendarRef[]>([]);
  let picked = $state<string[]>([]);
  let label = $state("");
  let googleUrl = $state("");

  const pickedCalendars = $derived(found.filter((calendar) => picked.includes(calendar.href)));

  async function run(work: () => Promise<void>) {
    busy = true;
    failure = null;
    try { await work(); } catch (reason) { failure = calendarError(reason); } finally { busy = false; }
  }

  const discover = () => run(async () => {
    found = await calendarIcloudDiscover(appleId, appPassword);
    picked = found.map((calendar) => calendar.href);
    step = "apple-pick";
  });

  const connectApple = () => run(async () => {
    const source = await calendarAddIcloud(label, appleId, appPassword, pickedCalendars);
    appPassword = "";
    // Der erste Abruf gehört zum Verbinden; scheitert er, zeigt die Karte den Fehler an der Verbindung.
    await calendarSync(source.id, localOffsetMinutes());
    onDone();
  });

  const connectGoogle = () => run(async () => {
    const source = await calendarAddGoogle(label, googleUrl);
    googleUrl = "";
    await calendarSync(source.id, localOffsetMinutes());
    onDone();
  });

  function toggle(href: string) {
    picked = picked.includes(href) ? picked.filter((entry) => entry !== href) : [...picked, href];
  }
</script>

<Modal title={t("Kalender verbinden")} {onClose}>
  {#if airGap}
    <p class="v-notice warn" role="status">{t("Air Gap ist an. Zum Verbinden zuerst ausschalten.")}</p>
  {/if}

  {#if step === "choose"}
    <p data-hint class="v-card-text">{t("IAP fragt den Kalender nur ab, wenn du „Aktualisieren“ klickst, und trägt nur Termine ein, die du speicherst.")}</p>
    <div class="v-cal-choices">
      <button type="button" class="v-cal-choice" onclick={() => (step = "apple")}>
        <strong>{t("Apple Kalender (iCloud)")}</strong>
        <span>{t("Termine holen und neue Termine eintragen. Du brauchst ein app-spezifisches Passwort.")}</span>
      </button>
      <button type="button" class="v-cal-choice" onclick={() => (step = "google")}>
        <strong>{t("Google Kalender")}</strong>
        <span>{t("Termine holen (nur Lesen). Du brauchst die geheime Adresse im iCal-Format.")}</span>
      </button>
    </div>
    <div class="v-row v-cal-actions"><span class="v-cal-spacer"></span><button type="button" class="v-btn v-btn-ghost" onclick={onClose}>{t("Abbrechen")}</button></div>

  {:else if step === "apple"}
    <p class="v-card-text">{t("Statt deines normalen Passworts brauchst du ein app-spezifisches: appleid.apple.com → „Anmeldung und Sicherheit“ → „App-spezifische Passwörter“.")}</p>
    <form class="v-cal-form" onsubmit={(event) => { event.preventDefault(); void discover(); }}>
      <label class="v-field"><span class="v-label">{t("Apple-ID")}</span><input type="email" bind:value={appleId} autocomplete="off" placeholder="name@icloud.com" /></label>
      <label class="v-field"><span class="v-label">{t("App-spezifisches Passwort")}</span><input type="password" bind:value={appPassword} autocomplete="off" placeholder="abcd-efgh-ijkl-mnop" /></label>
      <p data-hint class="v-help">{t("Das Passwort liegt verschlüsselt im Tresor und wird nie wieder angezeigt.")}</p>
      {#if failure}<p class="v-cal-problem" role="alert">{failure}</p>{/if}
      <div class="v-row v-cal-actions">
        <button type="button" class="v-btn v-btn-ghost" onclick={() => { step = "choose"; failure = null; }}>{t("Zurück")}</button>
        <span class="v-cal-spacer"></span>
        <button type="submit" class="v-btn v-btn-primary" disabled={busy || airGap || !appleId.trim() || !appPassword.trim()}>{t(busy ? "Suche Kalender …" : "Kalender suchen")}</button>
      </div>
    </form>

  {:else if step === "apple-pick"}
    <p class="v-card-text">{t("Welche Kalender soll IAP anzeigen?")}</p>
    <ul class="v-list v-cal-pick">
      {#each found as calendar (calendar.href)}
        <li>
          <label class="v-cal-pickrow">
            <input type="checkbox" checked={picked.includes(calendar.href)} onchange={() => toggle(calendar.href)} />
            <span class="v-cal-dot" style:background={calendar.color ?? "var(--v-accent-blue)"} aria-hidden="true"></span>
            <span class="v-list-main"><strong>{calendar.name}</strong>{#if !calendar.can_write}<span>{t("nur Lesen")}</span>{/if}</span>
          </label>
        </li>
      {/each}
    </ul>
    <label class="v-field"><span class="v-label">{t("Name der Verbindung")}</span><input bind:value={label} maxlength="60" placeholder={t("Apple Kalender")} /></label>
    {#if failure}<p class="v-cal-problem" role="alert">{failure}</p>{/if}
    <div class="v-row v-cal-actions">
      <button type="button" class="v-btn v-btn-ghost" onclick={() => { step = "apple"; failure = null; }}>{t("Zurück")}</button>
      <span class="v-cal-spacer"></span>
      <button type="button" class="v-btn v-btn-primary" onclick={connectApple} disabled={busy || airGap || pickedCalendars.length === 0}>{t(busy ? "Verbinde …" : "Verbinden und abrufen")}</button>
    </div>

  {:else}
    <p class="v-card-text">{t("Google Kalender im Browser: Einstellungen → Kalender wählen → „Kalender integrieren“ → „Geheime Adresse im iCal-Format“. Füge sie hier ein. Wer sie kennt, kann den Kalender lesen; IAP speichert sie verschlüsselt.")}</p>
    <form class="v-cal-form" onsubmit={(event) => { event.preventDefault(); void connectGoogle(); }}>
      <label class="v-field"><span class="v-label">{t("Geheime Adresse")}</span><input type="password" bind:value={googleUrl} autocomplete="off" placeholder="https://calendar.google.com/calendar/ical/…/basic.ics" /></label>
      <label class="v-field"><span class="v-label">{t("Name der Verbindung")}</span><input bind:value={label} maxlength="60" placeholder={t("Google Kalender")} /></label>
      <p data-hint class="v-help">{t("IAP kann über diese Adresse nur lesen. Neue Termine trägst du bei Google selbst ein.")}</p>
      {#if failure}<p class="v-cal-problem" role="alert">{failure}</p>{/if}
      <div class="v-row v-cal-actions">
        <button type="button" class="v-btn v-btn-ghost" onclick={() => { step = "choose"; failure = null; }}>{t("Zurück")}</button>
        <span class="v-cal-spacer"></span>
        <button type="submit" class="v-btn v-btn-primary" disabled={busy || airGap || !googleUrl.trim()}>{t(busy ? "Verbinde …" : "Verbinden und abrufen")}</button>
      </div>
    </form>
  {/if}
</Modal>

<style>
  .v-cal-form { display: flex; flex-direction: column; gap: var(--v-space-4); }
  .v-cal-actions { gap: var(--v-space-2); }
  .v-cal-spacer { flex: 1; }
  .v-cal-problem { margin: 0; color: var(--v-danger); font-size: var(--v-text-sm); }
  .v-cal-choices { display: grid; gap: var(--v-space-3); }
  .v-cal-choice { display: flex; flex-direction: column; gap: 4px; padding: var(--v-space-4); border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-1); text-align: left; color: var(--v-text-primary); cursor: pointer; }
  .v-cal-choice:hover { border-color: var(--v-accent-blue); }
  .v-cal-choice span { color: var(--v-text-muted); font-size: var(--v-text-sm); }
  .v-cal-pick { max-height: 14rem; overflow: auto; }
  .v-cal-pickrow { display: flex; align-items: center; gap: var(--v-space-2); width: 100%; cursor: pointer; }
  .v-cal-dot { flex: 0 0 auto; width: 10px; height: 10px; border-radius: 50%; }
</style>
