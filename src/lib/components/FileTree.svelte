<script lang="ts">
  import FileTree from "./FileTree.svelte";
  import { app, openFile, displayName } from "$lib/state.svelte";
  import type { TreeEntry } from "$lib/api";

  let {
    entries,
    level = 0,
    onmenu,
    onrename,
    onmove,
    onopen,
  }: {
    entries: TreeEntry[];
    level?: number;
    onmenu: (e: MouseEvent, entry: TreeEntry) => void;
    onrename: (entry: TreeEntry) => void;
    onmove: (from: string, toDir: string) => void;
    /** override for the All view: open in the owning workspace */
    onopen?: (path: string) => void;
  } = $props();

  let open = $state<Record<string, boolean>>({});
  let dropDir = $state<string | null>(null);

  function dirOf(entry: TreeEntry): string {
    return entry.is_dir ? entry.path : entry.path.split("/").slice(0, -1).join("/");
  }

  function onDrop(e: DragEvent, entry: TreeEntry) {
    e.preventDefault();
    e.stopPropagation();
    dropDir = null;
    const from = e.dataTransfer?.getData("text/pugdock-file");
    app.treeDrag = null;
    if (from) onmove(from, dirOf(entry));
  }
</script>

<ul style="--level: {level}">
  {#each entries as entry (entry.path)}
    <li>
      <button
        class="row"
        class:active={app.activePath === entry.path}
        class:selected={entry.is_dir && app.selectedDir === entry.path}
        class:droptarget={dropDir === entry.path || (entry.is_dir && app.osDropTarget === entry.path)}
        class:localonly={app.syncExcluded.includes(entry.path)}
        data-drop-dir={dirOf(entry)}
        draggable={true}
        onclick={() => {
          if (!onopen) app.selectedDir = dirOf(entry);
          if (entry.is_dir) open[entry.path] = !open[entry.path];
          else (onopen ?? openFile)(entry.path);
        }}
        ondblclick={() => onrename(entry)}
        oncontextmenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onmenu(e, entry);
        }}
        ondragstart={(e) => {
          e.dataTransfer?.setData("text/pugdock-file", entry.path);
          app.treeDrag = entry.path;
        }}
        ondragover={(e) => {
          if (e.dataTransfer?.types.includes("text/pugdock-file")) {
            e.preventDefault();
            e.stopPropagation();
            dropDir = entry.path;
          }
        }}
        ondragleave={() => (dropDir = dropDir === entry.path ? null : dropDir)}
        ondrop={(e) => onDrop(e, entry)}
      >
        <span class="chevron">{entry.is_dir ? (open[entry.path] ? "▾" : "▸") : ""}</span>
        <span class="name" class:dim={entry.is_dir}>{entry.is_dir ? entry.name : displayName(entry.name)}</span>
        {#if app.pins.includes(entry.path)}<span class="badge" data-tip="Pinned">📌</span>{/if}
        {#if app.syncExcluded.includes(entry.path)}<span class="badge" data-tip="Local only, not synced">⛔</span>{/if}
      </button>
      {#if entry.is_dir && open[entry.path] && entry.children}
        <FileTree entries={entry.children} level={level + 1} {onmenu} {onrename} {onmove} {onopen} />
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
  .row.selected {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .row.selected .name {
    color: var(--text);
  }
  .row.droptarget {
    background: color-mix(in srgb, var(--accent) 20%, var(--bg));
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .row.localonly .name {
    opacity: 0.65;
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
    flex: 1;
  }
  .name.dim {
    color: var(--text-dim);
  }
  .badge {
    font-size: 9px;
    flex-shrink: 0;
  }
</style>
