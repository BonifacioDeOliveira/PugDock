<script lang="ts">
  import { api, type SearchHit } from "$lib/api";
  import { openFile, app } from "$lib/state.svelte";

  let query = $state("");
  let hits = $state<SearchHit[]>([]);
  let searching = $state(false);
  let timer: ReturnType<typeof setTimeout>;

  function onInput() {
    clearTimeout(timer);
    timer = setTimeout(async () => {
      if (!query.trim()) {
        hits = [];
        return;
      }
      searching = true;
      try {
        hits = await api.search(query.trim());
      } finally {
        searching = false;
      }
    }, 200);
  }

  function renderSnippet(s: string): string {
    const esc = s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    return esc.replaceAll("", "<mark>").replaceAll("", "</mark>");
  }
</script>

<div class="panel">
  <!-- svelte-ignore a11y_autofocus -->
  <input
    placeholder="Search files and content…"
    bind:value={query}
    oninput={onInput}
    autofocus
  />
  <div class="results">
    {#if searching}
      <p class="dim">Searching…</p>
    {:else if query && !hits.length}
      <p class="dim">No results.</p>
    {/if}
    {#each hits as hit (hit.path)}
      <button
        class="hit"
        onclick={() => {
          openFile(hit.path);
          app.panel = null;
        }}
      >
        <div class="path">{hit.path}</div>
        {#if hit.snippet}
          <!-- eslint-disable-next-line svelte/no-at-html-tags - escaped above -->
          <div class="snippet">{@html renderSnippet(hit.snippet)}</div>
        {/if}
      </button>
    {/each}
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
    padding: 12px;
  }
  .results {
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .hit {
    text-align: left;
    background: none;
    border: none;
    padding: 6px 8px;
    border-radius: var(--radius);
  }
  .hit:hover {
    background: var(--bg-hover);
  }
  .path {
    color: var(--accent);
    font-size: 12px;
  }
  .snippet {
    color: var(--text-dim);
    font-size: 12px;
    margin-top: 2px;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .snippet :global(mark) {
    background: color-mix(in srgb, var(--accent) 30%, transparent);
    color: var(--text);
    border-radius: 2px;
  }
  .dim {
    color: var(--text-dim);
  }
</style>
