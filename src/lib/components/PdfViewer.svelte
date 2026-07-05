<script lang="ts">
  import * as pdfjs from "pdfjs-dist";
  import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
  import type { Tab } from "$lib/state.svelte";
  import type { PDFDocumentProxy } from "pdfjs-dist";

  pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

  let { tab }: { tab: Tab } = $props();

  let canvas: HTMLCanvasElement;
  let doc: PDFDocumentProxy | null = null;
  let page = $state(1);
  let pages = $state(0);
  let zoom = $state(1.2);
  let rendering = false;

  /** Full text of the PDF, used for search indexing and AI summaries. */
  export async function extractText(): Promise<string> {
    if (!doc) return "";
    const parts: string[] = [];
    for (let i = 1; i <= doc.numPages; i++) {
      const p = await doc.getPage(i);
      const content = await p.getTextContent();
      parts.push(content.items.map((it) => ("str" in it ? it.str : "")).join(" "));
    }
    return parts.join("\n\n");
  }

  async function render() {
    if (!doc || rendering) return;
    rendering = true;
    try {
      const p = await doc.getPage(page);
      const viewport = p.getViewport({ scale: zoom * devicePixelRatio });
      canvas.width = viewport.width;
      canvas.height = viewport.height;
      canvas.style.width = `${viewport.width / devicePixelRatio}px`;
      await p.render({ canvas, viewport }).promise;
    } finally {
      rendering = false;
    }
  }

  $effect(() => {
    const bytes = Uint8Array.from(atob(tab.content), (c) => c.charCodeAt(0));
    let cancelled = false;
    pdfjs.getDocument({ data: bytes }).promise.then((d) => {
      if (cancelled) return void d.destroy();
      doc = d;
      pages = d.numPages;
      page = 1;
      render();
    });
    return () => {
      cancelled = true;
      doc?.destroy();
      doc = null;
    };
  });

  $effect(() => {
    void page, zoom;
    render();
  });
</script>

<div class="pdf">
  <div class="toolbar">
    <button class="ghost" onclick={() => (page = Math.max(1, page - 1))} disabled={page <= 1}>‹</button>
    <span>{page} / {pages}</span>
    <button class="ghost" onclick={() => (page = Math.min(pages, page + 1))} disabled={page >= pages}>›</button>
    <span class="sep"></span>
    <button class="ghost" onclick={() => (zoom = Math.max(0.4, zoom - 0.2))}>−</button>
    <span>{Math.round(zoom * 100)}%</span>
    <button class="ghost" onclick={() => (zoom = Math.min(4, zoom + 0.2))}>+</button>
  </div>
  <div class="canvas-wrap">
    <canvas bind:this={canvas}></canvas>
  </div>
</div>

<style>
  .pdf {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
    color: var(--text-dim);
    font-size: 12px;
  }
  .sep {
    width: 1px;
    height: 16px;
    background: var(--border);
  }
  .canvas-wrap {
    flex: 1;
    overflow: auto;
    display: flex;
    justify-content: center;
    padding: 16px;
    background: #101216;
  }
  canvas {
    box-shadow: 0 2px 16px rgba(0, 0, 0, 0.5);
    align-self: flex-start;
  }
</style>
