<script lang="ts">
  // Meldung am Rand: verschwindet nach einer Frist, bei Mauskontakt wartet sie, und ein Wisch
  // nach rechts oder das Kreuz schließt sie sofort.
  import { onMount } from "svelte";
  import { t } from "../i18n/index.svelte";
  import type { Notice } from "../notifications";
  interface Props { notice: Notice; onClose: (id: number) => void; duration?: number; }
  let { notice, onClose, duration = 4200 }: Props = $props();
  const DISMISS_PX = 64;
  const EXIT_MS = 200;

  let left = 0;
  let startedAt = 0;
  let timer: number | null = null;
  let startX = $state<number | null>(null);
  let offset = $state(0);
  let hold = $state(false);
  let closing = $state(false);

  function close() {
    if (closing) return;
    closing = true;
    if (timer) clearTimeout(timer);
    window.setTimeout(() => onClose(notice.id), matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : EXIT_MS);
  }
  function arm() {
    if (duration <= 0 || closing) return;
    startedAt = performance.now();
    timer = window.setTimeout(close, left);
  }
  function pause() {
    if (hold) return;
    hold = true;
    if (timer) clearTimeout(timer);
    left = Math.max(0, left - (performance.now() - startedAt));
  }
  function resume() {
    if (!hold || closing) return;
    hold = false;
    arm();
  }
  function down(event: PointerEvent) {
    if ((event.target as Element).closest("button") || event.button !== 0) return;
    startX = event.clientX;
    pause();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }
  function move(event: PointerEvent) {
    if (startX === null) return;
    const delta = event.clientX - startX;
    offset = delta > 0 ? delta : delta / 4;
  }
  function up() {
    if (startX === null) return;
    if (offset >= DISMISS_PX) close();
    else { offset = 0; resume(); }
    startX = null;
  }
  onMount(() => { left = duration; arm(); return () => { if (timer) clearTimeout(timer); }; });
</script>

<div class={`toast ${notice.tone}`} class:closing class:dragging={startX !== null && offset !== 0} style={`--offset:${offset}px;--life:${duration}ms`}
  role={notice.tone === "error" ? "alert" : "status"} onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up}
  onpointerenter={pause} onpointerleave={() => { if (startX === null) resume(); }}>
  <span class="mark" aria-hidden="true"><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/>{#if notice.tone === "error"}<path d="M12 7v6m0 4h.01"/>{:else if notice.tone === "success"}<path d="m7 12 3 3 7-7"/>{:else}<path d="M12 11v6m0-10h.01"/>{/if}</svg></span>
  <div class="copy"><strong>{notice.title}</strong>{#if notice.description}<span>{notice.description}</span>{/if}</div>
  <button type="button" onclick={close} aria-label={t("Hinweis schließen")}><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M5 5l14 14M19 5 5 19"/></svg></button>
  <span class="life" class:hold aria-hidden="true"></span>
</div>

<style>
  .toast { position: relative; width: min(22rem, 100%); display: flex; align-items: flex-start; gap: .7rem; overflow: hidden; padding: .85rem .8rem 1rem; border: 1px solid var(--v-line-strong); border-radius: var(--v-radius-card); background: var(--v-surface-solid); color: var(--v-text-primary); box-shadow: 0 16px 38px rgb(var(--v-shade) / .3); touch-action: pan-y; transform: translateX(var(--offset)); transition: transform 280ms ease, opacity 200ms ease; }
  @starting-style { .toast { transform: translateX(calc(100% + 1rem)); opacity: 0; } }
  .toast.dragging { transition: none; }
  .toast.closing { transform: translateX(calc(100% + 1rem)); opacity: 0; }
  .toast.error { border-color: color-mix(in oklab, var(--v-danger) 55%, transparent); }
  .mark { flex: 0 0 1.25rem; display: grid; place-items: center; width: 1.25rem; height: 1.25rem; color: var(--v-accent-blue); }
  .error .mark { color: var(--v-danger); }
  .copy { min-width: 0; flex: 1; display: grid; gap: .25rem; font-size: var(--v-text-sm); line-height: 1.4; }
  .copy strong { font-weight: 620; }
  .copy span { color: var(--v-text-secondary); overflow-wrap: anywhere; }
  button { flex: 0 0 auto; width: 1.8rem; height: 1.8rem; display: grid; place-items: center; padding: 0; border: 0; border-radius: .4rem; background: transparent; color: var(--v-text-muted); cursor: pointer; }
  button:hover, button:focus-visible { color: var(--v-text-primary); }
  .life { position: absolute; left: 0; bottom: 0; width: 100%; height: 2px; transform-origin: left; background: var(--v-accent-blue); animation: toast-life var(--life) linear forwards; }
  .error .life { background: var(--v-danger); }
  .life.hold { animation-play-state: paused; }
  @keyframes toast-life { to { transform: scaleX(0); } }
  @media (prefers-reduced-motion: reduce) { .toast { transition: opacity 120ms ease; } .toast.closing { transform: none; } .life { animation: none; } }
</style>
