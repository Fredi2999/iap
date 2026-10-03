<script lang="ts">
  import { friendlyError } from "../errors";
  import { t } from "../i18n/index.svelte";

  interface Props {
    error: unknown;
    onRetry?: () => void;
    onDismiss?: () => void;
  }

  let { error, onRetry, onDismiss }: Props = $props();
  let friendly = $derived(friendlyError(error));
</script>

<div class="v-error" role="alert">
  <svg class="v-error-icon" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7.5v5.5M12 16.5h.01"/></svg>
  <div class="v-error-body">
    <p>{t(friendly.message)}</p>
    {#if friendly.detail && friendly.detail !== friendly.message}
      <details><summary>{t("Technische Details")}</summary><code>{friendly.detail}</code></details>
    {/if}
  </div>
  <div class="v-error-actions">
    {#if onRetry}<button type="button" class="v-btn v-btn-ghost" onclick={onRetry}>{t("Erneut versuchen")}</button>{/if}
    {#if onDismiss}
      <button type="button" class="v-btn-icon" aria-label={t("Hinweis schließen")} onclick={onDismiss}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg>
      </button>
    {/if}
  </div>
</div>

<style>
  .v-error { display: flex; align-items: flex-start; gap: var(--v-space-3); margin-bottom: var(--v-space-4); padding: var(--v-space-3) var(--v-space-4); border: 1px solid color-mix(in oklab, var(--v-danger) 38%, transparent); border-radius: var(--v-radius-field); background: var(--v-danger-soft); }
  .v-error-icon { flex: 0 0 auto; margin-top: 1px; color: var(--v-danger); }
  .v-error-body { flex: 1 1 auto; min-width: 0; }
  p { margin: 0; color: var(--v-text-primary); font-size: var(--v-text-sm); line-height: 1.5; }
  details { margin-top: var(--v-space-1); }
  summary { cursor: pointer; color: var(--v-text-muted); font-size: var(--v-text-xs); }
  code { display: block; margin-top: var(--v-space-1); color: var(--v-text-secondary); font-family: var(--v-font-mono); font-size: var(--v-text-xs); overflow-wrap: anywhere; white-space: pre-wrap; }
  .v-error-actions { display: flex; align-items: center; gap: var(--v-space-1); flex: 0 0 auto; }
</style>
