<script lang="ts">
  import { t, tk, i18n, LANGUAGES, setLanguage } from "../lib/i18n/index.svelte";
  import { setUiLanguage } from "../lib/ipc";
  import { onMount } from "svelte";
  import { deleteVault, listVaults, unlockVault } from "../lib/ipc";
  import type { BootstrapStatus, SettingsSnapshot, VaultInfo } from "../lib/types";
  import Wordmark from "../lib/components/Wordmark.svelte";
  import GlowLogo from "../lib/components/GlowLogo.svelte";
  import logoUrl from "../lib/assets/iap-logo.png";
  import ScanlineOverlay from "../lib/components/ScanlineOverlay.svelte";
  import InfoTooltip from "../lib/components/InfoTooltip.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";

  interface Props {
    bootstrap: BootstrapStatus;
    onUnlocked: (snapshot: SettingsSnapshot) => void;
  }

  let { bootstrap, onUnlocked }: Props = $props();

  let availableVaults = $state<VaultInfo[]>([]);
  let selectedVaultPath = $state<string>("");
  let isCreatingNew = $state(false);
  let customVaultName = $state("");
  let passphrase = $state("");
  let confirm = $state("");
  let recoverFromHost = $state(false);
  let showAdvanced = $state(false);
  let showVaultPicker = $state(false);
  let submitting = $state(false);
  let error = $state<string | null>(null);

  // Sprache wird sofort umgestellt und auf dem Stick gespeichert, noch vor dem Entsperren.
  async function chooseLanguage(code: string) {
    setLanguage(code);
    try { setLanguage(await setUiLanguage(code)); } catch (reason) { console.error(reason); }
  }
  let passphraseVisible = $state(false);

  // Tresor löschen: Passwort und Name des Tresors sind Pflicht; das Backend prüft beides.
  let deleteOpen = $state(false);
  let deletePassphrase = $state("");
  let deleteName = $state("");
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);
  let notice = $state<string | null>(null);

  onMount(async () => {
    try {
      const vaults = await listVaults();
      availableVaults = vaults;
      if (vaults.length > 0) {
        selectedVaultPath = vaults[0].path;
        isCreatingNew = !vaults[0].initialized;
      } else {
        isCreatingNew = !bootstrap.vault_initialized;
      }
    } catch {
      isCreatingNew = !bootstrap.vault_initialized;
    }
  });

  function formatGiB(bytes: number): string {
    return Math.round(bytes / 1073741824) + " GB";
  }

  const selectedVault = $derived(
    availableVaults.find((v) => v.path === selectedVaultPath)
  );

  const passwordsMatch = $derived(
    confirm.length > 0 && passphrase === confirm
  );
  const passwordsMismatch = $derived(
    confirm.length > 0 && passphrase !== confirm
  );

  function handleVaultChange(path: string) {
    if (path === "__new__") {
      isCreatingNew = true;
      selectedVaultPath = "";
      customVaultName = "arbeit.db";
    } else {
      selectedVaultPath = path;
      const found = availableVaults.find((v) => v.path === path);
      if (found) {
        isCreatingNew = !found.initialized;
      }
    }
    error = null;
    confirm = "";
  }

  async function submit(event: Event) {
    event.preventDefault();
    error = null;
    if (!passphrase) {
      error = t("Bitte gib ein Passwort ein.");
      return;
    }
    if (isCreatingNew && passphrase !== confirm) {
      error = t("Die Passwörter stimmen nicht überein.");
      return;
    }

    let targetPath: string | null = null;
    if (selectedVaultPath) {
      targetPath = selectedVaultPath;
    } else if (customVaultName.trim()) {
      let clean = customVaultName.trim();
      if (!clean.endsWith(".db")) clean += ".db";
      // Im AI/data Verzeichnis anlegen
      const defaultVault = bootstrap.default_model ? availableVaults[0]?.path : "";
      if (defaultVault) {
        const lastSlash = Math.max(defaultVault.lastIndexOf("/"), defaultVault.lastIndexOf("\\"));
        if (lastSlash > 0) {
          targetPath = defaultVault.slice(0, lastSlash + 1) + clean;
        }
      }
    }

    submitting = true;
    try {
      const snapshot = await unlockVault(passphrase, isCreatingNew, recoverFromHost, targetPath);
      passphrase = "";
      confirm = "";
      onUnlocked(snapshot);
    } catch (reason: any) {
      const msg = String(reason?.message || reason);
      error = msg;
      if (msg.includes("nicht gefunden") || msg.includes("existiert nicht") || msg.includes("create_if_missing")) {
        isCreatingNew = true;
        confirm = passphrase;
      }
    } finally {
      submitting = false;
    }
  }

  function openDelete() {
    deleteOpen = true;
    deletePassphrase = "";
    deleteName = "";
    deleteError = null;
    notice = null;
  }

  function closeDelete() {
    deleteOpen = false;
    deletePassphrase = "";
    deleteName = "";
    deleteError = null;
  }

  async function confirmDelete(event: Event) {
    event.preventDefault();
    const vault = selectedVault;
    if (!vault || deleting) return;
    deleting = true;
    deleteError = null;
    try {
      await deleteVault(vault.path, deletePassphrase, deleteName);
      const name = vault.name;
      closeDelete();
      passphrase = "";
      confirm = "";
      error = null;
      availableVaults = await listVaults();
      const remaining = availableVaults.filter((entry) => entry.initialized);
      if (remaining.length > 0) {
        selectedVaultPath = remaining[0].path;
        isCreatingNew = false;
      } else {
        selectedVaultPath = "";
        customVaultName = "vault.db";
        isCreatingNew = true;
      }
      notice = t("Tresor „{name}“ wurde gelöscht.", { name });
    } catch (reason: any) {
      deleteError = String(reason?.message || reason);
    } finally {
      deleting = false;
    }
  }

  function toggleMode() {
    isCreatingNew = !isCreatingNew;
    error = null;
    confirm = "";
  }
</script>

<section class="unlock-screen relative flex min-h-screen items-center justify-center overflow-hidden px-6 py-12">
  <ScanlineOverlay paused={true} scanlineStrength={0.035} />

  <div class="relative w-full max-w-md">
    <!-- Header -->
    <header class="mb-7 flex flex-col items-center text-center">
      <div class="mb-2"><GlowLogo src={logoUrl} size={48} /></div>
        <div class="unlock-wordmark"><Wordmark text="IAP" /></div>

      <div class="flex items-center justify-center gap-2">
        <h1 class="text-2xl font-semibold tracking-tight text-white">
          {t(isCreatingNew ? "IAP einrichten" : "Willkommen zurück")}
        </h1>
        <InfoTooltip
          title={t("IAP Datentresor")}
          badge="100% Offline"
          description="Dein privater Speicher auf diesem USB-Stick. Vollständig isoliert mit 256-Bit SQLCipher und Argon2id-Schlüsselableitung."
          features={[
            tk("100 % offline: keine Schlüssel in einer Cloud"),
            tk("Mehrere Tresore: Arbeit, Privates und Projekte getrennt"),
            tk("Kein Zurücksetzen möglich: sicher vor fremdem Zugriff")
          ]}
          tip="Du kannst mehrere getrennte Tresore auf demselben USB-Stick betreiben."
        />
      </div>
      <p class="mt-2 text-xs text-zinc-400 leading-relaxed max-w-sm mx-auto">
        {#if isCreatingNew}
          {t("Erstelle dein Master-Passwort. Alle Konversationen, Daten und Konfigurationen werden damit lokal auf deinem USB-Stick verschlüsselt.")}
        {:else}
          {t("Gib dein Master-Passwort ein, um deinen privaten Datentresor zu entsperren.")}
        {/if}
      </p>
    </header>

    <div class="unlock-language" role="group" aria-label={t("Sprache")}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18ZM3 12h18M12 3c2.5 2.7 3.8 5.7 3.8 9s-1.3 6.3-3.8 9c-2.5-2.7-3.8-5.7-3.8-9S9.5 5.7 12 3Z"/></svg>
      <div class="v-segmented">
        {#each LANGUAGES as language (language.id)}
          <button type="button" lang={language.id} class:active={i18n.lang === language.id} aria-pressed={i18n.lang === language.id} title={language.label} onclick={() => chooseLanguage(language.id)}>{language.short}</button>
        {/each}
      </div>
    </div>

    <p class="unlock-hardware">{t("Dieser PC: {ram} Arbeitsspeicher · Leistungsstufe {tier}", { ram: formatGiB(bootstrap.hardware.total_ram_bytes), tier: bootstrap.plan.tier.toUpperCase() })}</p>

    <!-- Multi-Vault Selector Card -->
    {#if availableVaults.length > 0}
      <div class="mb-4 rounded-2xl border border-white/15 bg-white/[0.03] p-3.5 backdrop-blur-xl">
        <div class="flex items-center justify-between text-xs mb-2">
          <div class="flex items-center gap-1.5 text-zinc-300 font-medium">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="text-white"><ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/><path d="M3 12A9 3 0 0 0 21 12"/></svg>
            <span>{t("Tresor")}</span>
          </div>
          <button
            type="button"
            class="text-xs text-zinc-300 hover:text-white transition-colors underline-offset-2 hover:underline"
            onclick={() => (showVaultPicker = !showVaultPicker)}
          >
            {t(showVaultPicker ? "Ausblenden" : "Tresor wechseln")}
          </button>
        </div>

        {#if showVaultPicker}
          <div class="mt-2 space-y-2 v-fade-in border-t border-white/10 pt-2.5">
            <select
              class="w-full bg-[#121215] border border-white/20 rounded-xl px-3 py-2 text-xs text-white focus:outline-none"
              value={selectedVaultPath || "__new__"}
              onchange={(e) => handleVaultChange(e.currentTarget.value)}
            >
              {#each availableVaults as vault}
                <option value={vault.path}>
                  {vault.name} {vault.is_default ? "(Standard)" : ""} {vault.initialized ? "· Vorhanden" : "· Noch nicht angelegt"}
                </option>
              {/each}
              <option value="__new__">{t("Neuen Tresor anlegen …")}</option>
            </select>

            {#if !selectedVaultPath}
              <div class="mt-2">
                <label for="custom_name" class="block text-xs text-zinc-300 mb-1">{t("Dateiname des neuen Tresors:")}</label>
                <input
                  id="custom_name"
                  type="text"
                  bind:value={customVaultName}
                  placeholder={t("arbeit.db")}
                  class="w-full bg-white/[0.04] border border-white/15 rounded-xl px-3 py-1.5 text-xs text-white focus:outline-none"
                />
              </div>
            {/if}
          </div>
        {:else}
          <div class="flex items-center justify-between text-xs text-zinc-400">
            <span class="text-white font-semibold truncate max-w-[240px]">
              {selectedVault?.name ?? (customVaultName || "vault.db")}
            </span>
            <span class="text-zinc-400">
              {t(isCreatingNew ? "Wird neu angelegt" : "Bereit zum Entsperren")}
            </span>
          </div>
        {/if}

        {#if selectedVault?.initialized && !isCreatingNew && !deleteOpen}
          <button type="button" class="delete-link mt-2.5 text-zinc-400 hover:text-red-300 transition-colors underline-offset-2 hover:underline" onclick={openDelete}>{t("Tresor löschen …")}</button>
        {/if}

        {#if deleteOpen && selectedVault}
          <form class="mt-3 space-y-2.5 border-t border-red-400/30 pt-3 v-fade-in" onsubmit={confirmDelete}>
            <p class="text-xs font-semibold text-red-300">{t("„{name}“ unwiderruflich löschen?", { name: selectedVault.name })}</p>
            <p class="text-xs leading-relaxed text-zinc-300">{t("Der Tresor und alles darin (Unterhaltungen, Gedächtnis, Kalender, Einstellungen) wird gelöscht und lässt sich nicht wiederherstellen. Dateien im Arbeitsordner bleiben erhalten.")}</p>
            <label class="block text-xs text-zinc-300">
              {t("Passwort dieses Tresors")}
              <input type="password" bind:value={deletePassphrase} autocomplete="off" disabled={deleting} class="mt-1 w-full bg-white/[0.04] border border-white/15 rounded-xl px-3 py-1.5 text-xs text-white focus:outline-none" />
            </label>
            <label class="block text-xs text-zinc-300">
              {t("Zur Bestätigung den Namen eingeben: {name}", { name: selectedVault.name })}
              <input type="text" bind:value={deleteName} autocomplete="off" disabled={deleting} class="mt-1 w-full bg-white/[0.04] border border-white/15 rounded-xl px-3 py-1.5 text-xs text-white focus:outline-none" />
            </label>
            {#if deleteError}<p class="text-xs text-red-300" role="alert">{deleteError}</p>{/if}
            <div class="flex justify-end gap-2">
              <button type="button" class="delete-link rounded-xl border border-white/20 px-3 py-1.5 text-zinc-200 hover:bg-white/10" onclick={closeDelete} disabled={deleting}>{t("Abbrechen")}</button>
              <button type="submit" class="delete-link rounded-xl border border-red-400/50 bg-red-500/20 px-3 py-1.5 font-semibold text-red-200 hover:bg-red-500/30 disabled:opacity-50" disabled={deleting || !deletePassphrase || deleteName.trim() !== selectedVault.name}>{t(deleting ? "Wird gelöscht …" : "Endgültig löschen")}</button>
            </div>
          </form>
        {/if}
      </div>
    {/if}

    {#if notice}<p class="mb-3 text-center text-xs text-emerald-300" role="status">{notice}</p>{/if}

    <!-- Form card with Double-Bezel -->
    <div class="v-bezel-outer">
      <form class="v-bezel-inner p-6 shadow-2xl" onsubmit={submit}>
        <div class="mb-4">
          <label for="passphrase" class="mb-2 block text-sm font-medium text-zinc-300">
            {t(isCreatingNew ? "Neues Master-Passwort" : "Master-Passwort")}
          </label>
          <div class="relative">
            <input
              id="passphrase"
              type={passphraseVisible ? "text" : "password"}
              bind:value={passphrase}
              autocomplete={isCreatingNew ? "new-password" : "current-password"}
              placeholder={t(isCreatingNew ? "Neues Passwort wählen" : "Passwort eingeben")}
              class="w-full pr-10 bg-white/[0.04] border border-white/15 focus:border-white/40 rounded-xl px-3.5 py-2.5 text-sm text-white placeholder-zinc-500 transition focus:outline-none"
            />
            <button
              type="button"
              class="absolute right-2.5 top-1/2 -translate-y-1/2 rounded-lg p-1.5 text-zinc-400 hover:text-white transition-colors"
              onclick={() => (passphraseVisible = !passphraseVisible)}
              aria-label={t(passphraseVisible ? "Passwort verbergen" : "Passwort anzeigen")}
            >
              {#if passphraseVisible}
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m3 3 18 18M10.7 5.1c.4-.1.8-.1 1.3-.1 6 0 9 7 9 7-.5 1-1.2 2-2 2.9m-3.8 2.3c-1 .5-2.1.8-3.2.8-6 0-9-7-9-7 .8-1.8 2-3.3 3.4-4.5"/><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2"/></svg>
              {:else}
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/></svg>
              {/if}
            </button>
          </div>
        </div>

        {#if isCreatingNew}
          <div class="mb-4 v-fade-in">
            <div class="flex items-center justify-between mb-2">
              <label for="confirm" class="block text-sm font-medium text-zinc-300">
                {t("Passwort wiederholen")}
              </label>
              {#if passwordsMatch}
                <span class="text-xs text-emerald-400 flex items-center gap-1">
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
                  {t("Stimmt überein")}
                </span>
              {:else if passwordsMismatch}
                <span class="text-xs text-amber-400">{t("Stimmt noch nicht")}</span>
              {/if}
            </div>
            <input
              id="confirm"
              type="password"
              bind:value={confirm}
              autocomplete="new-password"
              placeholder={t("Passwort wiederholen")}
              class="w-full bg-white/[0.04] border border-white/15 focus:border-white/40 rounded-xl px-3.5 py-2.5 text-sm text-white placeholder-zinc-500 transition focus:outline-none"
            />
            <div class="mt-2.5 flex items-start gap-2 p-2.5 rounded-xl bg-white/[0.03] border border-white/10 text-xs text-zinc-400 leading-relaxed">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" class="mt-0.5 shrink-0 text-white"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/></svg>
              <span>{t("Merk dir das Passwort gut. Ohne Passwort lassen sich die Daten nicht wiederherstellen.")}</span>
            </div>
          </div>
        {/if}

        <div class="flex items-center justify-between mb-4 text-xs">
          {#if availableVaults.some(v => v.initialized)}
            <button
              type="button"
              class="text-zinc-400 hover:text-white transition-colors underline-offset-2 hover:underline"
              onclick={toggleMode}
            >
              {t(isCreatingNew ? "Vorhandenen Tresor öffnen" : "Neuen Tresor anlegen")}
            </button>
          {:else}
            <span class="text-zinc-400 text-xs">{t("Erster Start auf diesem Stick")}</span>
          {/if}

          <button
            type="button"
            class="flex items-center gap-1.5 text-zinc-400 hover:text-white transition-colors ml-auto"
            onclick={() => (showAdvanced = !showAdvanced)}
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" class="transition-transform duration-200" style={showAdvanced ? "transform: rotate(90deg)" : ""}><path d="m9 6 6 6-6 6"/></svg>
            {t("Optionen")}
          </button>
        </div>

        {#if showAdvanced}
          <div class="mb-4 space-y-2 rounded-xl border border-white/10 bg-white/[0.02] p-3 v-fade-in text-xs text-zinc-300">
            <label class="flex items-start gap-2 cursor-pointer">
              <input type="checkbox" bind:checked={recoverFromHost} class="mt-0.5 h-3.5 w-3.5 accent-white" />
              <span>{t("Wenn Stick und PC-Kopie verschieden sind: die neuere Kopie von diesem PC verwenden")}</span>
            </label>
          </div>
        {/if}

        {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

        <!-- Button-in-Button CTA -->
        <button
          type="submit"
          disabled={submitting || (isCreatingNew && passphrase.length > 0 && passphrase !== confirm)}
          class="v-btn-nested w-full"
        >
          <span>
            {#if submitting}
              {t(isCreatingNew ? "Lege Tresor an …" : "Entsperre …")}
            {:else}
              {t(isCreatingNew ? "Tresor anlegen und starten" : "Entsperren")}
            {/if}
          </span>
          <div class="v-icon-circle">
            {#if submitting}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" class="animate-spin"><path d="M21 12a9 9 0 1 1-6.2-8.6"/></svg>
            {:else}
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 5l7 7-7 7"/></svg>
            {/if}
          </div>
        </button>
      </form>
    </div>

    <footer class="mt-5 space-y-1 text-center text-xs">
      <p class="text-zinc-400">
        {t("Vollständig offline · verschlüsselt · Modell: {model}", { model: bootstrap.default_model.display_name })}
      </p>
      <p class="text-zinc-400 text-xs">
        {t("IAP Sovereign AI · Deine Daten bleiben physisch auf diesem Datenträger.")}
      </p>
    </footer>
  </div>
</section>

<style>
  /* Unlayered `button { font: inherit }` in style.css schlägt `text-xs`; deshalb hier explizit. */
  .delete-link { font-size: var(--v-text-xs); }
</style>
