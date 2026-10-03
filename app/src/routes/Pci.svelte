<script lang="ts">
  // PCI (Personal Computer Information): Aktivität von PCs, auf denen der sichtbare Begleiter lief.
  // Der Begleiter zeichnet nur auf, welche Anwendung im Vordergrund war, deren Fenstertitel und
  // wie lange, und nur solange er sichtbar läuft. Keine Nachrichten, keine Tastatureingaben, kein
  // Bildschirm, kein Netz. Beim Einstecken des Sticks wird das Journal hierher importiert.
  import { onMount } from "svelte";
  import { t, tk, locale } from "../lib/i18n/index.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import { notify } from "../lib/notifications";
  import {
    pciStatus, pciImport, pciListComputers, pciActivity, pciDeleteHost,
    pciInstallAndStart, pciStart, pciStop, pciRemoveCollector,
  } from "../lib/ipc";
  import type { PciStatus, PciComputer, PciActivity } from "../lib/types";

  let status = $state<PciStatus | null>(null);
  let computers = $state<PciComputer[]>([]);
  let selected = $state<PciActivity | null>(null);
  let recordTitles = $state(true);
  let busy = $state<string>("");
  let error = $state<unknown>(null);

  async function refresh() {
    try {
      status = await pciStatus();
      computers = await pciListComputers();
      if (selected && !computers.some((c) => c.host === selected!.host)) selected = null;
    } catch (reason) { error = reason; }
  }

  // Der Begleiter läuft in einem eigenen Prozess; Zustand und wartende Aktivität regelmäßig neu lesen.
  onMount(() => {
    refresh();
    const timer = setInterval(async () => {
      if (busy !== "") return;
      try { status = await pciStatus(); } catch { /* beim nächsten Takt erneut */ }
    }, 10000);
    return () => clearInterval(timer);
  });

  async function guarded(kind: string, work: () => Promise<void>) {
    busy = kind; error = null;
    try { await work(); } catch (reason) { error = reason; } finally { busy = ""; }
  }

  const install = () => guarded("install", async () => { status = await pciInstallAndStart(recordTitles); notify("PCI", "Begleiter installiert und gestartet.", "success"); });
  const start = () => guarded("start", async () => { status = await pciStart(recordTitles); });
  const stop = () => guarded("stop", async () => { status = await pciStop(); });
  const remove = () => guarded("remove", async () => {
    if (!confirm(t("Begleiter wirklich von diesem PC entfernen? Bereits gespeicherte PCI bleibt erhalten."))) return;
    status = await pciRemoveCollector();
    notify("PCI", "Begleiter vom PC entfernt.", "success");
  });
  const doImport = () => guarded("import", async () => {
    selected = await pciImport();
    await refresh();
    notify("PCI", "Aktivität importiert.", "success");
  });

  async function open(host: string) {
    error = null;
    try { selected = await pciActivity(host); } catch (reason) { error = reason; }
  }
  async function del(host: string) {
    if (!confirm(t("Alle gespeicherte Aktivität von „{host}“ löschen?", { host }))) return;
    error = null;
    try { await pciDeleteHost(host); if (selected?.host === host) selected = null; await refresh(); }
    catch (reason) { error = reason; }
  }

  function duration(ms: number): string {
    const min = Math.round(ms / 60000);
    if (min < 60) return t("{n} min", { n: min });
    const h = Math.floor(min / 60);
    const rest = min % 60;
    return rest === 0 ? t("{n} h", { n: h }) : t("{h} h {m} min", { h, m: rest });
  }
  const day = (ms: number) => ms > 0 ? new Date(ms).toLocaleDateString(locale(), { dateStyle: "medium" }) : "–";
</script>

<div class="v-page">
  <PageHeader title={t("PCI")} description="Aktivität von PCs, auf denen der sichtbare Begleiter lief. Er zeichnet nur auf, welche App im Vordergrund war, den Fenstertitel und wie lange, und nur solange er läuft. Keine Nachrichten, keine Tastatureingaben, kein Bildschirm.">
    {#snippet help()}{t("Der Begleiter läuft lokal auf einem PC, während der Stick nicht steckt. Beim Einstecken holst du die Aktivität mit „Importieren“ hierher. Du kannst den Begleiter jederzeit stoppen und mit einem Klick vom PC entfernen.")}{/snippet}
  </PageHeader>

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  <section class="v-card v-stack">
    <h2 class="v-card-title">{t("Begleiter auf diesem PC")}</h2>
    {#if status && !status.supported}
      <p class="v-notice warn">{t("Der PCI-Begleiter läuft derzeit nur unter Windows.")}</p>
    {:else if status}
      <div class="v-row v-pci-state">
        <span class="v-chip" class:accent={status.running}>{t(status.running ? "Läuft (zeichnet auf)" : status.installed ? "Installiert, gestoppt" : "Nicht installiert")}</span>
        <span class="v-help">{t("Dieser PC: {host}", { host: status.host })}</span>
        {#if status.journal_pending}<span class="v-chip warn">{t("Journal wartet: {d} Aktivität", { d: duration(status.journal_active_ms) })}</span>{/if}
      </div>
      <label class="v-row v-help"><input type="checkbox" bind:checked={recordTitles} disabled={status.running} /> {t("Fenstertitel mitschreiben (zeigt z. B. besuchte Seiten)")}</label>
      {#if status.running && !status.journal_pending}
        <p class="v-help">{t("Der Begleiter läuft. Die ersten Daten erscheinen nach etwa 30 Sekunden.")}</p>
      {/if}
      <div class="v-row v-pci-actions">
        {#if !status.installed}
          <button class="v-btn v-btn-primary" onclick={install} disabled={busy !== ""}>{t(busy === "install" ? "Installiere …" : "Installieren und starten")}</button>
        {:else}
          {#if status.running}
            <button class="v-btn v-btn-ghost" onclick={stop} disabled={busy !== ""}>{t(busy === "stop" ? "Stoppe …" : "Stoppen")}</button>
          {:else}
            <button class="v-btn v-btn-primary" onclick={start} disabled={busy !== ""}>{t(busy === "start" ? "Starte …" : "Starten")}</button>
          {/if}
          <button class="v-btn v-btn-danger" onclick={remove} disabled={busy !== ""}>{t(busy === "remove" ? "Entferne …" : "Vom PC entfernen")}</button>
        {/if}
        {#if status.installed || status.journal_pending}
          <button class="v-btn v-btn-ghost" onclick={doImport} disabled={busy !== "" || !status.journal_pending}>{t(busy === "import" ? "Importiere …" : "Jetzt importieren")}</button>
        {/if}
      </div>
    {/if}
  </section>

  <div class="v-grid-aside">
    <section class="v-card v-stack" aria-label={t("PCs")}>
      <h2 class="v-card-title">{t("Gespeicherte PCs")}</h2>
      {#if computers.length === 0}
        <EmptyState title={t("Noch keine Aktivität")} text="Starte den Begleiter auf einem PC und importiere später, wenn der Stick wieder steckt." icon="M4 5h16v10H4z" />
      {:else}
        <ul class="v-list">
          {#each computers as pc (pc.host)}
            <li>
              <button type="button" class="v-list-main v-pci-pc" class:active={selected?.host === pc.host} onclick={() => open(pc.host)}>
                <strong>{pc.host}</strong>
                <span>{t("{d} aktiv · zuletzt {date}", { d: duration(pc.total_active_ms), date: day(pc.last_unix_ms) })}</span>
              </button>
              <button class="v-btn-icon" title={t("Löschen")} aria-label={t("Löschen")} onclick={() => del(pc.host)}>
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3m-9 0 1 13h10l1-13"/></svg>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <section class="v-card v-stack" aria-label={t("Aktivität")}>
      {#if !selected}
        <p class="v-card-text">{t("Wähle links einen PC, um seine Aktivität zu sehen.")}</p>
      {:else}
        <h2 class="v-card-title">{selected.host}</h2>
        <p class="v-help">{t("{d} aktiv · {from} bis {to}", { d: duration(selected.total_active_ms), from: day(selected.first_unix_ms), to: day(selected.last_unix_ms) })}</p>
        {#if selected.apps.length === 0}
          <p class="v-card-text">{t("Keine Aktivität gespeichert.")}</p>
        {:else}
          <ul class="v-list">
            {#each selected.apps as app (app.app)}
              <li class="v-pci-app">
                <div class="v-list-main">
                  <strong>{app.app}</strong>
                  {#if app.sample_titles.length > 0}<span class="v-pci-titles">{app.sample_titles.join(" · ")}</span>{/if}
                </div>
                <span class="v-chip">{duration(app.total_ms)}</span>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}
    </section>
  </div>
</div>

<style>
  .v-pci-state { flex-wrap: wrap; align-items: center; gap: var(--v-space-2) var(--v-space-3); }
  .v-pci-actions { flex-wrap: wrap; gap: var(--v-space-2); }
  .v-pci-pc { display: grid; gap: 2px; border: 0; background: transparent; text-align: left; cursor: pointer; padding: 0; }
  .v-pci-pc.active strong { color: var(--v-accent-blue); }
  .v-pci-app { align-items: flex-start !important; }
  .v-pci-titles { display: block; overflow: hidden; color: var(--v-text-muted); font-size: var(--v-text-xs); text-overflow: ellipsis; white-space: nowrap; }
</style>
