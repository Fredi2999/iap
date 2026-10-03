<script lang="ts">
  import { t } from "../i18n/index.svelte";
  import { formatBytes } from "../documents";
  import type { PetStyle, SystemStats } from "../types";

  // Zeigt die vom Nutzer gewählten Werte unter dem Pet. Reine Darstellung der gemessenen
  // Daten; nichts davon wird gespeichert oder gesendet.
  interface Props {
    style: PetStyle;
    stats: SystemStats | null;
    task: { label: string; waiting: number } | null;
  }
  let { style, stats, task }: Props = $props();

  function percent(used: number, total: number): number {
    return total > 0 ? Math.min(100, Math.round((used / total) * 100)) : 0;
  }
  const ram = $derived(stats ? percent(stats.ram_used_bytes, stats.ram_total_bytes) : 0);
  const disk = $derived(stats ? percent(stats.disk_used_bytes, stats.disk_total_bytes) : 0);
  const cpu = $derived(stats ? Math.min(100, Math.round(stats.cpu_percent)) : 0);
</script>

<dl class="v-pet-stats">
  {#if style.show_task}
    <div class="row task">
      <dt>{t("Aufgabe")}</dt>
      <dd title={task?.label ?? ""}>{task ? task.label : t("Keine Aufgabe")}{#if task && task.waiting > 0} <span class="more">+{task.waiting}</span>{/if}</dd>
    </div>
  {/if}
  {#if style.show_cpu}
    <div class="row" title={stats?.cpu_name ?? ""}>
      <dt>{t("CPU")}</dt>
      <dd>{#if stats}{cpu} %{:else}…{/if}<span class="bar" aria-hidden="true"><span style={`width:${cpu}%`}></span></span></dd>
    </div>
  {/if}
  {#if style.show_ram}
    <div class="row" title={stats ? `${formatBytes(stats.ram_used_bytes)} / ${formatBytes(stats.ram_total_bytes)}` : ""}>
      <dt>{t("RAM")}</dt>
      <dd>{#if stats}{ram} %{:else}…{/if}<span class="bar" aria-hidden="true"><span style={`width:${ram}%`}></span></span></dd>
    </div>
  {/if}
  {#if style.show_storage}
    <div class="row" title={stats ? `${formatBytes(stats.disk_used_bytes)} / ${formatBytes(stats.disk_total_bytes)}` : ""}>
      <dt>{t("Speicher")}</dt>
      <dd>{#if stats && stats.disk_total_bytes > 0}{disk} %{:else}{stats ? "–" : "…"}{/if}<span class="bar" aria-hidden="true"><span style={`width:${disk}%`}></span></span></dd>
    </div>
  {/if}
</dl>

<style>
  .v-pet-stats { display: grid; gap: 2px; width: 100%; margin: 0; font-size: var(--v-text-xs); }
  .row { display: grid; grid-template-columns: 3.4rem minmax(0, 1fr); align-items: center; gap: 6px; height: 16px; }
  dt { color: var(--v-text-muted); }
  dd { display: flex; align-items: center; justify-content: flex-end; gap: 6px; margin: 0; color: var(--v-text-primary); font-variant-numeric: tabular-nums; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .task dd { justify-content: flex-start; }
  .more { color: var(--v-text-muted); }
  .bar { flex: 0 0 34px; height: 5px; border-radius: 3px; background: var(--v-line); overflow: hidden; }
  .bar > span { display: block; height: 100%; background: var(--v-accent-blue); }
</style>
