<script lang="ts">
  import { api } from "$lib/api";
  import { app, applyStatus } from "$lib/state.svelte";

  let compare = $state<{ path: string; local: string; github: string } | null>(null);
  let busy = $state(false);

  async function keep(path: string, side: "local" | "github") {
    busy = true;
    try {
      applyStatus(await api.gitResolveConflict(path, side));
      compare = null;
      const tab = app.tabs.find((t) => t.path === path);
      if (tab && tab.kind === "text") tab.content = await api.readFile(path);
      if (app.conflicts.length === 0) {
        await api.gitPush().catch(() => {});
        applyStatus(await api.gitStatus());
      }
    } finally {
      busy = false;
    }
  }

  async function showCompare(path: string) {
    const [local, github] = await api.gitConflictVersions(path);
    compare = { path, local, github };
  }
</script>

<div class="overlay">
  <div class="dialog">
    <h2>Needs review</h2>
    <p>These files changed in two places. Choose which version to keep.</p>
    {#each app.conflicts as path (path)}
      <div class="conflict">
        <code>{path}</code>
        <div class="btns">
          <button onclick={() => keep(path, "local")} disabled={busy}>Keep local version</button>
          <button onclick={() => keep(path, "github")} disabled={busy}>Keep GitHub version</button>
          <button class="ghost" onclick={() => showCompare(path)}>Compare changes</button>
        </div>
      </div>
    {/each}
    {#if compare}
      <div class="compare">
        <div>
          <h4>Local — {compare.path}</h4>
          <pre>{compare.local.slice(0, 8000)}</pre>
        </div>
        <div>
          <h4>GitHub — {compare.path}</h4>
          <pre>{compare.github.slice(0, 8000)}</pre>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: grid;
    place-items: center;
    z-index: 100;
  }
  .dialog {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 20px;
    width: 720px;
    max-width: 92vw;
    max-height: 85vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  p {
    margin: 0;
    color: var(--text-dim);
  }
  .conflict {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .btns {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .compare {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .compare h4 {
    margin: 0 0 4px;
    font-size: 12px;
    color: var(--text-dim);
  }
  pre {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px;
    font-size: 11px;
    max-height: 300px;
    overflow: auto;
    margin: 0;
    white-space: pre-wrap;
  }
</style>
