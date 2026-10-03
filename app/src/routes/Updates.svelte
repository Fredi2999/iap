<script lang="ts">
  import { t, tk, locale } from "../lib/i18n/index.svelte";
  import { onMount } from "svelte";
  import { listBackups, verifyUpdateBundle, createFullExport } from "../lib/ipc";
  import type { BundleVerification } from "../lib/types";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";

  let backups = $state<string[]>([]);
  let bundlePath = $state<string>("");
  let verification = $state<BundleVerification | null>(null);
  let exportPath = $state<string | null>(null);
  let error = $state<unknown>(null);
  let busy = $state(false);

  onMount(async () => {
    try { backups = await listBackups(); }
    catch (reason) { error = reason; }
  });

  async function doVerify(event: SubmitEvent) {
    event.preventDefault();
    error = null; verification = null; busy = true;
    try { verification = await verifyUpdateBundle(bundlePath); }
    catch (reason) { error = reason; }
    finally { busy = false; }
  }

  async function doExport() {
    error = null; busy = true;
    try { exportPath = await createFullExport(); }
    catch (reason) { error = reason; }
    finally { busy = false; }
  }

  function fmtBytes(b: number): string {
    if (b < 1024) return `${b} B`;
    if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
    if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
    return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }

  const SIGNATURE_LABEL = { ok: tk("Gültig"), skipped: tk("Nicht geprüft"), not_provided: tk("Keine Signatur vorhanden") } as const;
</script>

<div class="v-page">
  <PageHeader title={t("Updates und Sicherung")} description="Neue Versionen prüfen, Sicherungen einsehen und alle Daten in offenen Formaten exportieren." />

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  <div class="v-grid-2">
    <form class="v-card v-stack" onsubmit={doVerify}>
      <h2 class="v-card-title">{t("Update prüfen")}</h2>
      <p class="v-card-text">{t("Gib den Ordner eines entpackten Updates an. IAP prüft jede Datei, bevor sie verwendet wird.")}</p>
      <label class="v-field"><span class="v-label">{t("Ordner des Updates")}</span><input placeholder={t("Zum Beispiel E:\\Updates\\iap-neu")} bind:value={bundlePath} /></label>
      <div class="v-row v-row-end"><button type="submit" class="v-btn v-btn-primary" disabled={!bundlePath.trim() || busy}>{t("Prüfen")}</button></div>
      {#if verification}
        <ul class="v-list">
          <li><div class="v-list-main"><strong>{t("Geprüfte Dateien")}</strong></div><span class="v-num">{verification.verified_files}</span></li>
          <li><div class="v-list-main"><strong>{t("Geprüfte Datenmenge")}</strong></div><span class="v-num">{fmtBytes(verification.verified_bytes)}</span></li>
          <li><div class="v-list-main"><strong>{t("Signatur")}</strong>{#if verification.signature.kind === "skipped"}<span>{verification.signature.reason}</span>{/if}</div><span class="v-chip" class:accent={verification.signature.kind === "ok"} class:warn={verification.signature.kind !== "ok"}>{t(SIGNATURE_LABEL[verification.signature.kind])}</span></li>
        </ul>
      {/if}
    </form>

    <div class="v-stack">
      <section class="v-card v-stack">
        <h2 class="v-card-title">{t("Sicherungen")} <small>{backups.length}</small></h2>
        {#if backups.length === 0}
          <EmptyState title={t("Noch keine Sicherungen")} text="Sie entstehen automatisch, wenn eine Sitzung endet." icon="M12 3v12m0 0-4-4m4 4 4-4M5 21h14" />
        {:else}
          <ul class="v-list">{#each backups as b (b)}<li><div class="v-list-main"><strong title={b}>{b.split(/[\\/]/).pop()}</strong></div></li>{/each}</ul>
        {/if}
      </section>
      <section class="v-card v-stack">
        <h2 class="v-card-title">{t("Alles exportieren")}</h2>
        <p data-hint class="v-card-text">{t("Legt auf dem Stick einen Exportordner mit Unterhaltungen, Gedächtnis und Kalender in offenen Formaten an.")}</p>
        <div class="v-row v-row-end"><button class="v-btn v-btn-ghost" onclick={doExport} disabled={busy}>{t("Export anlegen")}</button></div>
        {#if exportPath}<div class="v-notice">{t("Export liegt unter")} <code class="v-num">{exportPath}</code></div>{/if}
      </section>
    </div>
  </div>
</div>
