<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "../i18n/index.svelte";

  interface Props {
    title: string;
    text?: string;
    icon?: string;
    action?: Snippet;
  }

  let { title, text, icon, action }: Props = $props();
</script>

<div class="v-empty">
  {#if icon}
    <span class="v-empty-icon" aria-hidden="true">
      <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d={icon}/></svg>
    </span>
  {/if}
  <strong>{t(title)}</strong>
  {#if text}<p>{t(text)}</p>{/if}
  {#if action}<div class="v-empty-action">{@render action()}</div>{/if}
</div>

<style>
  .v-empty { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: var(--v-space-2); padding: var(--v-space-6) var(--v-space-4); border: 1px dashed var(--v-line-strong); border-radius: var(--v-radius-card); text-align: center; }
  .v-empty-icon { display: grid; place-items: center; width: 2.75rem; height: 2.75rem; margin-bottom: var(--v-space-1); border-radius: 50%; background: var(--v-accent-blue-soft); color: var(--v-accent-blue); }
  strong { color: var(--v-text-primary); font-size: var(--v-text-md); font-weight: 600; }
  p { max-width: 46ch; margin: 0; color: var(--v-text-muted); font-size: var(--v-text-sm); line-height: 1.5; }
  .v-empty-action { margin-top: var(--v-space-2); }
</style>
