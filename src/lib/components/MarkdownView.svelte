<script lang="ts">
  import { marked } from "marked";
  import DOMPurify from "dompurify";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { app, type Tab } from "$lib/state.svelte";

  let { tab }: { tab: Tab } = $props();

  /** Workspace-relative image paths render through the asset protocol. */
  function fixLocalImages(rendered: string): string {
    const root = app.config?.workspace_path;
    if (!root) return rendered;
    return rendered.replace(/(<img[^>]+src=")([^"]+)(")/g, (m, pre, src, post) => {
      if (/^(https?:|data:|asset:|blob:)/.test(src)) return m;
      return pre + convertFileSrc(`${root}/${decodeURIComponent(src)}`) + post;
    });
  }

  // Sanitized: notes sync from GitHub, so treat file content as untrusted.
  const html = $derived(fixLocalImages(DOMPurify.sanitize(marked.parse(tab.content, { async: false }))));
</script>

<div class="md-view">
  <!-- eslint-disable-next-line svelte/no-at-html-tags - sanitized above -->
  {@html html}
</div>

<style>
  .md-view {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 60px;
    max-width: 820px;
    margin: 0 auto;
    line-height: 1.65;
    font-size: 14px;
  }
  .md-view :global(h1),
  .md-view :global(h2),
  .md-view :global(h3) {
    margin: 1.2em 0 0.5em;
    line-height: 1.3;
  }
  .md-view :global(h1) {
    font-size: 1.7em;
    border-bottom: 1px solid var(--border);
    padding-bottom: 0.3em;
  }
  .md-view :global(h2) {
    font-size: 1.35em;
  }
  .md-view :global(h3) {
    font-size: 1.1em;
  }
  .md-view :global(p) {
    margin: 0.6em 0;
  }
  .md-view :global(a) {
    color: var(--accent);
  }
  .md-view :global(code) {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 1px 5px;
    font-size: 0.85em;
  }
  .md-view :global(pre) {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px 14px;
    overflow-x: auto;
  }
  .md-view :global(pre code) {
    background: none;
    border: none;
    padding: 0;
  }
  .md-view :global(blockquote) {
    margin: 0.8em 0;
    padding: 2px 14px;
    border-left: 3px solid var(--accent);
    color: var(--text-dim);
  }
  .md-view :global(ul),
  .md-view :global(ol) {
    padding-left: 1.6em;
  }
  .md-view :global(li) {
    margin: 0.25em 0;
  }
  .md-view :global(table) {
    border-collapse: collapse;
    margin: 0.8em 0;
    display: block;
    overflow-x: auto;
  }
  .md-view :global(th),
  .md-view :global(td) {
    border: 1px solid var(--border);
    padding: 6px 12px;
  }
  .md-view :global(th) {
    background: var(--bg-panel);
  }
  .md-view :global(img) {
    max-width: 100%;
  }
  .md-view :global(hr) {
    border: none;
    border-top: 1px solid var(--border);
    margin: 1.5em 0;
  }
  .md-view :global(input[type="checkbox"]) {
    margin-right: 6px;
  }
</style>
