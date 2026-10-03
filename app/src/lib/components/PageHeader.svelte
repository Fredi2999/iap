<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "../i18n/index.svelte";

  interface Props {
    title: string;
    description?: string;
    actions?: Snippet;
    help?: Snippet;
  }

  let { title, description, actions, help }: Props = $props();
  let helpOpen = $state(false);
</script>

<header class="v-page-header">
  <div class="v-page-header-text">
    <div class="v-page-header-title">
      <h1>{t(title)}</h1>
      {#if help}
        <button type="button" class="v-btn-icon v-page-help-toggle" aria-expanded={helpOpen} aria-label={t("Hilfe zu diesem Bereich")} onclick={() => (helpOpen = !helpOpen)}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M9.8 9a2.3 2.3 0 1 1 3.7 1.8c-1 .8-1.5 1.2-1.5 2.3M12 17h.01"/></svg>
        </button>
      {/if}
    </div>
    {#if description}<p>{t(description)}</p>{/if}
  </div>
  {#if actions}<div class="v-page-header-actions">{@render actions()}</div>{/if}
</header>
{#if help && helpOpen}
  <div class="v-page-help">{@render help()}</div>
{/if}

<style>
  .v-page-header { display: flex; align-items: flex-start; justify-content: space-between; gap: var(--v-space-4); margin-bottom: var(--v-space-5); }
  .v-page-header-text { min-width: 0; }
  .v-page-header-title { display: flex; align-items: center; gap: var(--v-space-2); }
  h1 { margin: 0; font-size: var(--v-text-2xl); font-weight: 600; letter-spacing: -.02em; line-height: 1.2; color: var(--v-text-primary); }
  p { margin: var(--v-space-1) 0 0; max-width: 60ch; color: var(--v-text-muted); font-size: var(--v-text-md); line-height: 1.5; }
  .v-page-header-actions { display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: var(--v-space-2); flex: 0 0 auto; }
  .v-page-help { margin: calc(var(--v-space-3) * -1) 0 var(--v-space-5); padding: var(--v-space-3) var(--v-space-4); border: 1px solid var(--v-line); border-radius: var(--v-radius-field); background: rgb(var(--v-tint) / .04); color: var(--v-text-secondary); font-size: var(--v-text-sm); line-height: 1.55; }
  @media (max-width: 640px) {
    .v-page-header { flex-direction: column; }
    .v-page-header-actions { justify-content: flex-start; }
  }
</style>
