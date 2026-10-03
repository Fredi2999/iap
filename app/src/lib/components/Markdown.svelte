<script lang="ts">
  import { t } from "../i18n/index.svelte";
  interface Props {
    content: string;
    isStreaming?: boolean;
  }

  let { content, isStreaming = false }: Props = $props();

  let copiedIndex = $state<number | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;

  async function copyCode(code: string, index: number) {
    try {
      await navigator.clipboard.writeText(code);
      copiedIndex = index;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copiedIndex = null;
      }, 2000);
    } catch {
      // Fallback ignore
    }
  }

  function escapeHtml(str: string): string {
    return str
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#039;");
  }

  export function formatInline(raw: string): string {
    let text = escapeHtml(raw);

    // Inline code `...`
    text = text.replace(/`([^`]+)`/g, (_match, code) => {
      return `<code class="px-1.5 py-0.5 rounded font-mono text-[12px] bg-white/[0.08] text-white border border-white/15">${code}</code>`;
    });

    // Bold + Italic ***...*** or ___...___
    text = text.replace(/(\*\*\*|___)(.*?)\1/g, (_match, _delim, content) => {
      return `<strong class="font-bold text-white"><em class="italic text-zinc-200">${content}</em></strong>`;
    });

    // Bold **...** or __...__
    text = text.replace(/(\*\*|__)(.*?)\1/g, (_match, _delim, bold) => {
      return `<strong class="font-bold text-white">${bold}</strong>`;
    });

    // Italic *...* or _..._
    text = text.replace(/(?<!\*)\*([^*\n]+)\*(?!\*)|(?<!_)_([^_\n]+)_(?!_)/g, (_match, p1, p2) => {
      const italic = p1 || p2;
      return `<em class="italic text-zinc-200">${italic}</em>`;
    });

    // Strikethrough ~~...~~
    text = text.replace(/~~(.*?)~~/g, (_match, del) => {
      return `<del class="line-through text-zinc-400">${del}</del>`;
    });

    return text;
  }

  type Block =
    | { type: "code"; lang: string; code: string }
    | { type: "heading"; level: number; text: string }
    | { type: "quote"; text: string }
    | { type: "ul"; items: string[] }
    | { type: "ol"; items: string[]; start: number }
    | { type: "hr" }
    | { type: "p"; text: string };

  const parsedBlocks = $derived.by<Block[]>(() => {
    if (!content) return [];
    const lines = content.split("\n");
    const blocks: Block[] = [];

    let inCode = false;
    let codeLang = "";
    let codeBuffer: string[] = [];

    let currentUl: string[] | null = null;
    let currentOl: string[] | null = null;
    let orderedStart = 1;
    let currentParagraph: string[] = [];

    function flushParagraph() {
      if (currentParagraph.length > 0) {
        blocks.push({
          type: "p",
          text: currentParagraph.join("\n"),
        });
        currentParagraph = [];
      }
    }

    function flushUl() {
      if (currentUl && currentUl.length > 0) {
        blocks.push({ type: "ul", items: currentUl });
        currentUl = null;
      }
    }

    function flushOl() {
      if (currentOl && currentOl.length > 0) {
        blocks.push({ type: "ol", items: currentOl, start: orderedStart });
        currentOl = null;
      }
    }

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];

      // Fenced code block delimiter
      if (line.trim().startsWith("```")) {
        if (!inCode) {
          flushParagraph();
          flushUl();
          flushOl();
          inCode = true;
          codeLang = line.trim().slice(3).trim();
          codeBuffer = [];
        } else {
          inCode = false;
          blocks.push({
            type: "code",
            lang: codeLang || "text",
            code: codeBuffer.join("\n"),
          });
          codeBuffer = [];
          codeLang = "";
        }
        continue;
      }

      if (inCode) {
        codeBuffer.push(line);
        continue;
      }

      const trimmed = line.trim();

      // Empty line -> separation of blocks
      if (!trimmed) {
        flushParagraph();
        flushUl();
        flushOl();
        continue;
      }

      // Horizontal rule
      if (trimmed === "---" || trimmed === "***" || trimmed === "___") {
        flushParagraph();
        flushUl();
        flushOl();
        blocks.push({ type: "hr" });
        continue;
      }

      // Headings #, ##, ###
      if (line.startsWith("#")) {
        const match = line.match(/^(#{1,6})\s+(.*)$/);
        if (match) {
          flushParagraph();
          flushUl();
          flushOl();
          const level = Math.min(3, match[1].length);
          blocks.push({ type: "heading", level, text: match[2] });
          continue;
        }
      }

      // Blockquotes
      if (line.startsWith(">")) {
        flushParagraph();
        flushUl();
        flushOl();
        const quoteText = line.replace(/^>\s?/, "");
        blocks.push({ type: "quote", text: quoteText });
        continue;
      }

      // Unordered list items: * or -
      const ulMatch = line.match(/^(\s*)[*-]\s+(.*)$/);
      if (ulMatch) {
        flushParagraph();
        flushOl();
        if (!currentUl) currentUl = [];
        currentUl.push(ulMatch[2]);
        continue;
      }

      // Ordered list items: 1. , 2.
      const olMatch = line.match(/^(\s*)(\d+)\.\s+(.*)$/);
      if (olMatch) {
        flushParagraph();
        flushUl();
        if (!currentOl) {
          currentOl = [];
          orderedStart = Number(olMatch[2]);
        }
        currentOl.push(olMatch[3]);
        continue;
      }

      // Normal paragraph text
      flushUl();
      flushOl();
      currentParagraph.push(line);
    }

    // Flush any pending blocks
    if (inCode) {
      blocks.push({
        type: "code",
        lang: codeLang || "text",
        code: codeBuffer.join("\n"),
      });
    }
    flushParagraph();
    flushUl();
    flushOl();

    return blocks;
  });
</script>

<div class="v-markdown space-y-2 text-sm leading-relaxed text-zinc-200">
  {#each parsedBlocks as block, idx}
    {#if block.type === "p"}
      <p class="leading-6 break-words whitespace-pre-wrap">{@html formatInline(block.text)}</p>
    {:else if block.type === "ul"}
      <ul class="my-2 space-y-1.5 pl-1.5">
        {#each block.items as item}
          <li class="flex items-start gap-2 text-sm leading-6 break-words">
            <span class="mt-2 h-1.5 w-1.5 shrink-0 rounded-full bg-zinc-400"></span>
            <div class="flex-1">{@html formatInline(item)}</div>
          </li>
        {/each}
      </ul>
    {:else if block.type === "ol"}
      <ol start={block.start} class="my-2 space-y-1.5 pl-5 list-decimal list-outside text-sm leading-6">
        {#each block.items as item}
          <li class="break-words">{@html formatInline(item)}</li>
        {/each}
      </ol>
    {:else if block.type === "heading"}
      {#if block.level === 1}
        <h1 class="mt-4 mb-2 text-base font-bold text-white tracking-tight border-b border-white/10 pb-1">
          {@html formatInline(block.text)}
        </h1>
      {:else if block.level === 2}
        <h2 class="mt-3 mb-1.5 text-sm font-bold text-white tracking-tight">
          {@html formatInline(block.text)}
        </h2>
      {:else}
        <h3 class="mt-2.5 mb-1 text-xs font-semibold text-zinc-100 uppercase tracking-wider font-mono">
          {@html formatInline(block.text)}
        </h3>
      {/if}
    {:else if block.type === "quote"}
      <blockquote class="my-2 border-l-2 border-white/30 bg-white/[0.02] pl-3 py-1 text-zinc-300 italic rounded-r">
        {@html formatInline(block.text)}
      </blockquote>
    {:else if block.type === "hr"}
      <hr class="my-3 border-white/10" />
    {:else if block.type === "code"}
      <div class="my-3 overflow-hidden rounded-xl border border-white/15 bg-black/60 shadow-lg">
        <div class="flex items-center justify-between border-b border-white/10 bg-white/[0.04] px-3.5 py-1.5 text-xs text-zinc-400 font-mono">
          <span class="uppercase text-[10px] tracking-wider text-zinc-300 font-semibold">{block.lang || "code"}</span>
          <button
            type="button"
            class="flex items-center gap-1.5 text-[11px] text-zinc-400 hover:text-white transition-colors px-2 py-0.5 rounded-md hover:bg-white/10"
            onclick={() => copyCode(block.code, idx)}
            title={t("Code in Zwischenablage kopieren")}
          >
            {#if copiedIndex === idx}
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" class="text-emerald-400"><path d="M20 6 9 17l-5-5"/></svg>
              <span class="text-emerald-400 font-medium">{t("Kopiert!")}</span>
            {:else}
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>
              <span>{t("Kopieren")}</span>
            {/if}
          </button>
        </div>
        <pre class="overflow-x-auto p-3.5 font-mono text-[12px] leading-5 text-zinc-200 selection:bg-white/20"><code>{block.code}</code></pre>
      </div>
    {/if}
  {/each}
</div>
