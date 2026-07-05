<script lang="ts">
  import FileTree from "./FileTree.svelte";
  import { app, openFile } from "$lib/state.svelte";
  import type { TreeEntry } from "$lib/api";

  let {
    entries,
    level = 0,
    onmenu,
  }: {
    entries: TreeEntry[];
    level?: number;
    onmenu: (e: MouseEvent, entry: TreeEntry) => void;
  } = $props();

  let open = $state<Record<string, boolean>>({});
</script>

<ul style="--level: {level}">
  {#each entries as entry (entry.path)}
    <li>
      <button
        class="row"
        class:active={app.activePath === entry.path}
        onclick={() => (entry.is_dir ? (open[entry.path] = !open[entry.path]) : openFile(entry.path))}
        oncontextmenu={(e) => {
          e.preventDefault();
          onmenu(e, entry);
        }}
      >
        <span class="chevron">{entry.is_dir ? (open[entry.path] ? "▾" : "▸") : ""}</span>
        <span class="name" class:dim={entry.is_dir}>{entry.name}</span>
      </button>
      {#if entry.is_dir && open[entry.path] && entry.children}
        <FileTree entries={entry.children} level={level + 1} {onmenu} />
      {/if}
    </li>
  {/each}
</ul>

<style>
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 3px 8px 3px calc(8px + var(--level) * 14px);
    background: none;
    border: none;
    border-radius: 0;
    text-align: left;
    cursor: pointer;
    color: var(--text);
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .row.active {
    background: var(--bg-active);
  }
  .chevron {
    width: 10px;
    color: var(--text-dim);
    font-size: 10px;
    flex-shrink: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.dim {
    color: var(--text-dim);
  }
</style>
