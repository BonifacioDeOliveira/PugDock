<script lang="ts">
  import { api, type Checkpoint } from "$lib/api";
  import { app } from "$lib/state.svelte";

  let checkpoints = $state<Checkpoint[]>([]);
  let preview = $state<{ hash: string; content: string } | null>(null);
  const forFile = $derived(app.activePath);

  $effect(() => {
    void forFile;
    preview = null;
    api.gitHistory(forFile ?? undefined, 50).then((c) => (checkpoints = c));
  });

  async function show(hash: string) {
    if (!forFile) return;
    try {
      preview = { hash, content: await api.gitFileAt(hash, forFile) };
    } catch {
      preview = { hash, content: "(file did not exist at this checkpoint)" };
    }
  }

  async function restore() {
    if (!preview || !forFile) return;
    await api.writeFile(forFile, preview.content);
    const tab = app.tabs.find((t) => t.path === forFile);
    if (tab) tab.content = preview.content;
    preview = null;
  }
</script>

<div class="panel">
  <h3>{forFile ? `History — ${forFile.split("/").pop()}` : "Workspace history"}</h3>
  <div class="list">
    {#each checkpoints as c (c.hash)}
      <button class="item" onclick={() => forFile && show(c.hash)}>
        <div class="msg">{c.message}</div>
        <div class="date">{c.date.slice(0, 16)}</div>
      </button>
    {:else}
      <p class="date">No checkpoints yet.</p>
    {/each}
  </div>
  {#if preview}
    <div class="preview">
      <pre>{preview.content.slice(0, 5000)}</pre>
      <button class="primary" onclick={restore}>Restore this version</button>
    </div>
  {/if}
</div>

<style>
  .panel {
    height: 100%;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: hidden;
  }
  h3 {
    margin: 0;
    font-size: 13px;
  }
  .list {
    overflow-y: auto;
    flex-shrink: 0;
    max-height: 40%;
  }
  .item {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    padding: 5px 8px;
    border-radius: var(--radius);
  }
  .item:hover {
    background: var(--bg-hover);
  }
  .msg {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .date {
    color: var(--text-dim);
    font-size: 11px;
  }
  .preview {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: hidden;
  }
  pre {
    flex: 1;
    overflow: auto;
    margin: 0;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px;
    font-size: 11px;
  }
</style>
