<script lang="ts">
  // Passwort des geöffneten Tresors ändern. Das Backend prüft das aktuelle Passwort und
  // schlüsselt den Tresor alles-oder-nichts um; hier stehen nur die Eingaben und klare Meldungen.
  import { t } from "../i18n/index.svelte";
  import { changeVaultPassword } from "../ipc";
  import { notify } from "../notifications";
  import ErrorNotice from "./ErrorNotice.svelte";

  const MIN_CHARS = 8;

  let current = $state("");
  let next = $state("");
  let repeat = $state("");
  let visible = $state(false);
  let busy = $state(false);
  let error = $state<unknown>(null);
  let problem = $state("");

  let mismatch = $derived(repeat.length > 0 && next !== repeat);

  function validate(): string {
    if (!current) return t("Bitte gib dein aktuelles Passwort ein.");
    if (next.length < MIN_CHARS) return t("Das neue Passwort braucht mindestens 8 Zeichen.");
    if (next === current) return t("Das neue Passwort muss sich vom aktuellen unterscheiden.");
    if (next !== repeat) return t("Die Passwörter stimmen nicht überein.");
    return "";
  }

  async function submit(event: Event) {
    event.preventDefault();
    error = null;
    problem = validate();
    if (problem) return;
    busy = true;
    try {
      await changeVaultPassword(current, next);
      current = "";
      next = "";
      repeat = "";
      notify("Tresor", "Passwort geändert.", "success");
    } catch (reason) {
      error = reason;
    } finally {
      busy = false;
    }
  }
</script>

<section class="v-card v-stack" aria-labelledby="set-password">
  <h2 id="set-password" class="v-card-title">{t("Tresor-Passwort ändern")}</h2>
  <p class="v-card-text">{t("Gilt sofort; der Stick muss stecken bleiben. Ältere Sicherungen bleiben mit dem alten Passwort lesbar. Ohne Passwort gibt es keine Wiederherstellung.")}</p>
  <form class="v-stack" onsubmit={submit}>
    <label class="v-field"><span class="v-label">{t("Aktuelles Passwort")}</span>
      <input type={visible ? "text" : "password"} bind:value={current} autocomplete="current-password" disabled={busy} />
    </label>
    <label class="v-field"><span class="v-label">{t("Neues Passwort")}</span>
      <input type={visible ? "text" : "password"} bind:value={next} autocomplete="new-password" disabled={busy} />
      <span class="v-help">{t("Mindestens 8 Zeichen.")}</span>
    </label>
    <label class="v-field"><span class="v-label">{t("Neues Passwort wiederholen")}</span>
      <input type={visible ? "text" : "password"} bind:value={repeat} autocomplete="new-password" disabled={busy} aria-invalid={mismatch} />
      {#if mismatch}<span class="v-help" role="status">{t("Stimmt noch nicht")}</span>{/if}
    </label>
    <label class="v-row v-label"><input type="checkbox" bind:checked={visible} /> {t("Passwörter anzeigen")}</label>
    {#if problem}<p class="v-notice warn" role="alert">{problem}</p>{/if}
    {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}
    <div class="v-row v-row-end">
      <button type="submit" class="v-btn v-btn-primary" disabled={busy || !current || !next || !repeat}>{t(busy ? "Ändere …" : "Passwort ändern")}</button>
    </div>
  </form>
</section>
