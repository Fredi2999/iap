<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "../lib/i18n/index.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import EmptyState from "../lib/components/EmptyState.svelte";
  import ErrorNotice from "../lib/components/ErrorNotice.svelte";
  import { notify } from "../lib/notifications";
  import { commandName, loadTemplates, saveTemplates, PLACEHOLDER, type PromptTemplate } from "../lib/templates";

  let templates = $state<PromptTemplate[]>([]);
  let selectedId = $state<string | null>(null);
  let name = $state("");
  let text = $state("");
  let saving = $state(false);
  let loaded = $state(false);
  let error = $state<unknown>(null);

  const duplicate = $derived(templates.some((item) => item.id !== selectedId && commandName(item.name) === commandName(name)));

  onMount(async () => {
    try { templates = await loadTemplates(); } catch (reason) { error = reason; }
    loaded = true;
  });

  function edit(template: PromptTemplate | null) {
    selectedId = template?.id ?? null;
    name = template?.name ?? "";
    text = template?.text ?? "";
  }

  async function persist(next: PromptTemplate[], message: string) {
    saving = true;
    error = null;
    try {
      await saveTemplates(next);
      templates = next;
      notify(message, "", "success");
    } catch (reason) {
      error = reason;
    } finally {
      saving = false;
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!name.trim() || !text.trim() || duplicate) return;
    const entry: PromptTemplate = { id: selectedId ?? crypto.randomUUID(), name: name.trim(), text };
    const next = selectedId ? templates.map((item) => (item.id === selectedId ? entry : item)) : [...templates, entry];
    await persist(next, "Vorlage gespeichert");
    selectedId = entry.id;
  }

  async function remove() {
    if (!selectedId) return;
    await persist(templates.filter((item) => item.id !== selectedId), "Vorlage gelöscht");
    edit(null);
  }
</script>

<div class="v-page">
  <PageHeader title={t("Vorlagen")} description="Häufige Aufträge als Vorlage speichern und im Chat mit „/“ abrufen.">
    {#snippet actions()}<button class="v-btn v-btn-ghost" onclick={() => edit(null)}>{t("Neue Vorlage")}</button>{/snippet}
    {#snippet help()}{t("Tippe im Chat „/“ und den Namen der Vorlage. Steht {{text}} in der Vorlage, landet der Cursor genau dort.")}{/snippet}
  </PageHeader>

  {#if error}<ErrorNotice {error} onDismiss={() => (error = null)} />{/if}

  <div class="v-grid-aside v-templates">
    <form class="v-card v-stack" onsubmit={save}>
      <h2 class="v-card-title">{t(selectedId ? "Vorlage bearbeiten" : "Neue Vorlage")}</h2>
      <label class="v-field"><span class="v-label">{t("Name")}</span>
        <input bind:value={name} maxlength="48" placeholder={t("Zum Beispiel: Elternbrief")} />
        <span class="v-help">{t("Im Chat: {command}", { command: commandName(name || t("Name")) })}</span>
        {#if duplicate}<span class="v-help v-warn-text">{t("Diesen Befehl gibt es schon.")}</span>{/if}
      </label>
      <label class="v-field"><span class="v-label">{t("Text")}</span>
        <textarea bind:value={text} rows="8" placeholder={t("Zum Beispiel: Schreibe einen freundlichen Elternbrief zu folgendem Anlass: {{text}}")}></textarea>
        <span class="v-help">{t("{placeholder} markiert die Stelle für deine Eingabe.", { placeholder: PLACEHOLDER })}</span>
      </label>
      <div class="v-row v-row-end">
        {#if selectedId}<button type="button" class="v-btn v-btn-ghost" onclick={remove} disabled={saving}>{t("Löschen")}</button>{/if}
        <button type="submit" class="v-btn v-btn-primary" disabled={saving || !name.trim() || !text.trim() || duplicate}>{t(saving ? "Speichere …" : "Speichern")}</button>
      </div>
    </form>

    <section class="v-card">
      <h2 class="v-card-title">{t("Gespeicherte Vorlagen")}</h2>
      {#if loaded && templates.length === 0}
        <EmptyState title={t("Noch keine Vorlagen")} text="Lege links deine erste Vorlage an." />
      {:else}
        <ul class="v-list">
          {#each templates as template (template.id)}
            <li><button type="button" class="v-template" class:active={template.id === selectedId} onclick={() => edit(template)}>
              <strong>{template.name}</strong><span class="v-help">{commandName(template.name)}</span>
            </button></li>
          {/each}
        </ul>
      {/if}
    </section>
  </div>
</div>

<style>
  .v-template { display: flex; flex-direction: column; align-items: flex-start; gap: 2px; width: 100%; padding: var(--v-space-2) 0; border: 0; background: transparent; color: var(--v-text-secondary); text-align: left; cursor: pointer; }
  .v-template strong { color: var(--v-text-primary); font-weight: 500; }
  .v-template.active strong { color: var(--v-accent-blue); }
  .v-template .v-help { font-family: var(--v-font-mono); }
  .v-warn-text { color: var(--v-warning); }
  .v-templates textarea { resize: vertical; }
</style>
