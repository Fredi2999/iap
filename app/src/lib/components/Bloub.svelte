<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { t } from "../i18n/index.svelte";
  import { ENGINE_STATE, STATE_TEXT } from "../avatar";
  import { appState, resolvedTheme } from "../stores/app.svelte";
  import type { AvatarState } from "../types";
  import { NOTIF_BLUE } from "../vendor/bloub/decor";
  import { BotEngine, type BotFrame } from "../vendor/bloub/engine";
  import { EXPRESSION_BY_ID } from "../vendor/bloub/expressions";
  import { DEMI_VIEWBOX, RAYON } from "../vendor/bloub/repere";
  import { mixHex, SHAPE_BY_ID } from "../vendor/bloub/skins";
  import { POSES } from "../vendor/bloub/states";

  // Reine Darstellung: Zustand rein, Bild raus. Kein Zugriff auf Mikrofon,
  // Bildschirm, Werkzeuge oder IPC. Die Figur ist eine eigenständige IAP-
  // Gestaltung auf dem Animationskern von Bloub (siehe vendor/bloub/NOTICE.md).
  interface Props {
    avatarState: AvatarState;
    size?: number;
    /** Von außen anhalten, etwa wenn das Fenster nicht sichtbar ist. */
    paused?: boolean;
    /** Zeigt zusätzlich den Textstatus unter der Figur. */
    showText?: boolean;
    /** Form-ID aus vendor/bloub/skins.ts; unbekannte Werte fallen auf den Kiesel zurück. */
    shape?: string;
    /** `auto` folgt dem Farbschema, sonst ein Hexwert wie #3b93f0. */
    color?: string;
  }
  let { avatarState, size = 240, paused = false, showText = false, shape: shapeId = "galet", color = "auto" }: Props = $props();

  const R = RAYON;
  const VB = DEMI_VIEWBOX;
  // Eigene Gestalt: standardmäßig der leicht unregelmäßige Kiesel und der aufmerksame Ausdruck.
  const shape = $derived((SHAPE_BY_ID.get(shapeId) ?? SHAPE_BY_ID.get("galet"))?.radii ?? null);
  const expression = EXPRESSION_BY_ID.get("attentif") ?? null;

  let reducedMotion = $state(false);
  let pageVisible = $state(true);
  let inView = $state(true);
  const animated = $derived(appState.effects === "full" && !reducedMotion && !paused && pageVisible && inView);

  // IAP-Farbwelt statt Schwarz: Körper in Akzentblau, Augen sind Löcher zum Hintergrund.
  const light = $derived(resolvedTheme() === "light");
  const ink = $derived(/^#[0-9a-fA-F]{6}$/.test(color) ? color : light ? "#1d6fa5" : "#7cc4ea");
  const paper = $derived(light ? "#e6eff4" : "#0a1a27");

  const engineState = $derived(ENGINE_STATE[avatarState]);
  const engine = new BotEngine(R, ENGINE_STATE.ready, untrack(() => shape), expression);
  let frame = $state.raw<BotFrame>(engine.sample(POSES[ENGINE_STATE.ready]));
  let svg = $state<SVGSVGElement | null>(null);
  const uid = Math.random().toString(36).slice(2, 8);
  const maskId = `iap-bot-mask-${uid}`;

  let clock = 0;
  let raf = 0;
  let last = 0;
  let carry = 0;
  let appliedState = ENGINE_STATE.ready;

  function tick(ms: number) {
    raf = requestAnimationFrame(tick);
    // Begrenzter Zeitschritt: nach einer Pause springt die Figur nicht nach vorn.
    const dt = last ? Math.min((ms - last) / 1000, 0.064) : 0;
    last = ms;
    clock += dt;
    carry += dt;
    // 30 Bilder pro Sekunde reichen und schonen schwache Rechner.
    if (carry < 1 / 30) return;
    carry = 0;
    frame = engine.sample(clock);
  }

  function stop() {
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
    last = 0;
  }

  $effect(() => {
    if (animated) {
      if (!raf) raf = requestAnimationFrame(tick);
    } else {
      stop();
    }
  });

  // Zustandswechsel: animiert überblenden, statisch die lesbarste Pose zeigen.
  // Formwechsel laufen im selben Effekt: ohne Animation springt die Form sofort (kein halber Übergang).
  $effect(() => {
    const next = engineState;
    engine.setShape(shape, animated ? clock : clock - 1e4);
    if (next !== appliedState) {
      engine.setState(next, clock);
      appliedState = next;
    }
    if (!animated) frame = engine.sample(clock + POSES[next]);
  });

  onMount(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    reducedMotion = media.matches;
    const onMedia = () => (reducedMotion = media.matches);
    media.addEventListener("change", onMedia);
    const onVisibility = () => (pageVisible = !document.hidden);
    onVisibility();
    document.addEventListener("visibilitychange", onVisibility);
    let observer: IntersectionObserver | null = null;
    if (svg && "IntersectionObserver" in window) {
      observer = new IntersectionObserver((entries) => (inView = entries.some((entry) => entry.isIntersecting)));
      observer.observe(svg);
    }
    return () => {
      media.removeEventListener("change", onMedia);
      document.removeEventListener("visibilitychange", onVisibility);
      observer?.disconnect();
    };
  });
  onDestroy(stop);

  function dotAttrs(dot: BotFrame["dots"][number]) {
    const fill = dot.color ?? (dot.depth === undefined ? ink : mixHex(paper, ink, dot.depth));
    const common = { fill, opacity: dot.opacity };
    return dot.d
      ? { ...common, d: dot.d, transform: `translate(${dot.x} ${dot.y}) rotate(${dot.rot ?? 0}) scale(${R})` }
      : { ...common, cx: dot.x, cy: dot.y, r: dot.r };
  }
</script>

<figure class="v-bloub" style={`--bloub-size:${size}px`}>
  <svg bind:this={svg} width={size} height={size} viewBox={`${-VB} ${-VB} ${VB * 2} ${VB * 2}`} role="img" aria-label={t(STATE_TEXT[avatarState])}>
    <defs>
      <mask id={maskId} maskUnits="userSpaceOnUse" x={-VB} y={-VB} width={VB * 2} height={VB * 2}>
        <path d={frame.bodyPath} fill="#fff" />
        {#each frame.eyes as eye, index (index)}
          <path d={eye.d} transform={eye.matrix} opacity={eye.alpha} fill="#000" />
        {/each}
        {#if frame.notch}<circle cx={frame.notch.x} cy={frame.notch.y} r={frame.notch.r} fill="#000" />{/if}
      </mask>
      {#each frame.arcs as arc (arc.id)}
        <linearGradient id={`${uid}-${arc.id}`} gradientUnits="userSpaceOnUse" x1={arc.grad.x1} y1={arc.grad.y1} x2={arc.grad.x2} y2={arc.grad.y2}>
          {#each arc.grad.stops as color, index (index)}<stop offset={index / (arc.grad.stops.length - 1)} stop-color={color} />{/each}
        </linearGradient>
      {/each}
    </defs>
    <g fill="none" stroke-linecap="round">
      {#each frame.arcs as arc (arc.id)}<path d={arc.back} stroke={`url(#${uid}-${arc.id})`} stroke-width={arc.width} opacity={arc.opacity} />{/each}
    </g>
    {#if frame.dotsBehind}
      <g>{#each frame.dots as dot, index (index)}{#if dot.d}<path {...dotAttrs(dot)} />{:else}<circle {...dotAttrs(dot)} />{/if}{/each}</g>
    {/if}
    <g opacity={frame.bodyAlpha}>
      <path d={frame.bodyPath} fill={paper} />
      <g mask={`url(#${maskId})`}><rect x={-VB} y={-VB} width={VB * 2} height={VB * 2} fill={ink} /></g>
    </g>
    {#if !frame.dotsBehind}
      <g>{#each frame.dots as dot, index (index)}{#if dot.d}<path {...dotAttrs(dot)} />{:else}<circle {...dotAttrs(dot)} />{/if}{/each}</g>
    {/if}
    {#if frame.notif}<circle cx={frame.notif.x} cy={frame.notif.y} r={frame.notif.r} fill={NOTIF_BLUE} />{/if}
    <g fill="none" stroke-linecap="round">
      {#each frame.arcs as arc (arc.id)}<path d={arc.front} stroke={`url(#${uid}-${arc.id})`} stroke-width={arc.width} opacity={arc.opacity} />{/each}
    </g>
  </svg>
  {#if showText}<figcaption class="v-bloub-text" aria-live="polite">{t(STATE_TEXT[avatarState])}</figcaption>{/if}
</figure>

<style>
  .v-bloub { display: grid; justify-items: center; gap: .5rem; margin: 0; }
  svg { width: var(--bloub-size); max-width: 100%; height: auto; aspect-ratio: 1; overflow: visible; }
  .v-bloub-text { color: var(--v-text-secondary); font-size: var(--v-text-sm); font-weight: 560; }
</style>
