<script lang="ts">
  import { api, errorMessage, type TreeEntry, type UpdateInfo } from "$lib/api";
  import { app, openFile, openToSide, closeTab, refreshTree, settings, syncEnabled, workspaceManaged, colorFor, toast, type Tab } from "$lib/state.svelte";
  import { switchWorkspace, addWorkspace, closeWorkspace } from "$lib/workspaces";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import MarkdownView from "./MarkdownView.svelte";
  import { syncNow, startSync, pushOnExit, flushSaves } from "$lib/sync";
  import FileTree from "./FileTree.svelte";
  import CodeEditor from "./CodeEditor.svelte";
  import PdfViewer from "./PdfViewer.svelte";
  import SearchPanel from "./SearchPanel.svelte";
  import SettingsPanel from "./SettingsPanel.svelte";
  import AiPanel from "./AiPanel.svelte";
  import HistoryPanel from "./HistoryPanel.svelte";
  import ConflictDialog from "./ConflictDialog.svelte";
  import AiFab from "./AiFab.svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { openUrl } from "@tauri-apps/plugin-opener";

  const activeTab = $derived(app.tabs.find((t) => t.path === app.activePath));
  const splitTab = $derived(app.split ? app.tabs.find((t) => t.path === app.split?.path) : undefined);

  const SYNC_LABEL: Record<string, string> = {
    saved: "Saved",
    saving: "Saving…",
    "saved-locally": "Saved locally",
    checkpointing: "Creating checkpoint…",
    syncing: "Syncing…",
    synced: "Synced",
    offline: "Offline — will sync later",
    "needs-review": "Needs review",
  };

  // --- context menu ---
  let menu = $state<{ x: number; y: number; entry: TreeEntry | null } | null>(null);
  let wsMenu = $state(false);

  async function pickAndAdd(managed: boolean) {
    wsMenu = false;
    const picked = await openDialog({
      directory: true,
      title: managed ? "Choose a folder for the new workspace" : "Open folder",
    });
    if (typeof picked === "string") {
      await addWorkspace(picked, managed).catch((e) => toast(errorMessage(e)));
    }
  }

  function treeHas(path: string, entries = app.tree): boolean {
    return entries.some((e) => e.path === path || (e.children ? treeHas(path, e.children) : false));
  }

  /** Big "New note" button: create instantly, open, let the user rename later. */
  async function quickNote() {
    const dir = workspaceManaged() ? "notes/" : "";
    for (let i = 1; i < 1000; i++) {
      const path = `${dir}untitled${i === 1 ? "" : `-${i}`}.md`;
      if (!treeHas(path)) {
        await api.writeFile(path, "");
        await refreshTree();
        await openFile(path);
        return;
      }
    }
  }
  // --- inline prompt modal ---
  let modal = $state<{ title: string; value: string; onOk: (v: string) => void } | null>(null);
  let updateInfo = $state<UpdateInfo | null>(null);

  function showMenu(e: MouseEvent, entry: TreeEntry | null) {
    menu = { x: e.clientX, y: e.clientY, entry };
  }

  function ask(title: string, initial: string, onOk: (v: string) => void) {
    modal = { title, value: initial, onOk };
  }

  function dirOf(entry: TreeEntry | null): string {
    if (!entry) return "";
    return entry.is_dir ? entry.path : entry.path.split("/").slice(0, -1).join("/");
  }

  async function run(fn: () => Promise<void>) {
    try {
      await fn();
      await refreshTree();
    } catch (e) {
      toast(errorMessage(e));
    }
  }

  const menuActions = {
    newFile: (entry: TreeEntry | null) =>
      ask("New file", dirOf(entry) ? dirOf(entry) + "/" : "notes/", (v) =>
        run(async () => {
          await api.writeFile(v, "");
          await openFile(v);
        }),
      ),
    newFolder: (entry: TreeEntry | null) =>
      ask("New folder", dirOf(entry) ? dirOf(entry) + "/" : "", (v) => run(() => api.createFolder(v))),
    rename: (entry: TreeEntry) =>
      ask("Rename / move", entry.path, (v) =>
        run(async () => {
          await api.renamePath(entry.path, v);
          const tab = app.tabs.find((t) => t.path === entry.path);
          if (tab) {
            tab.path = v;
            tab.name = v.split("/").pop() ?? v;
          }
          if (app.activePath === entry.path) app.activePath = v;
          api.removeFromIndex(entry.path).catch(() => {});
          api.indexFile(v).catch(() => {});
        }),
      ),
    duplicate: (entry: TreeEntry) => run(async () => void (await api.duplicateFile(entry.path))),
    del: (entry: TreeEntry) => {
      if (!confirm(`Delete "${entry.path}"? A copy stays in checkpoint history.`)) return;
      run(async () => {
        await api.deletePath(entry.path);
        closeTab(entry.path);
        api.removeFromIndex(entry.path).catch(() => {});
      });
    },
    reveal: (entry: TreeEntry) => api.reveal(entry.path),
  };

  // Drag & drop files from the OS into the workspace.
  $effect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(async (event) => {
      if (event.payload.type !== "drop") return;
      for (const src of event.payload.paths) {
        const name = src.split(/[/\\]/).pop() ?? "file";
        const folder = name.toLowerCase().endsWith(".pdf") ? "pdfs" : "inbox";
        await api.importFile(src, `${folder}/${name}`).catch((e) => toast(errorMessage(e)));
        api.indexFile(`${folder}/${name}`).catch(() => {});
      }
      await refreshTree();
      toast("Imported into workspace");
    });
    return () => void unlisten.then((u) => u());
  });

  // Push on exit.
  $effect(() => {
    const unlisten = getCurrentWindow().onCloseRequested(async () => {
      await pushOnExit();
    });
    return () => void unlisten.then((u) => u());
  });

  // Startup: pull, index, schedulers, update check.
  $effect(() => {
    (async () => {
      startSync();
      if (syncEnabled() && settings().pullOnStartup) await syncNow().catch(() => {});
      api.rebuildIndex().catch(() => {});
      if (settings().autoCheckUpdates) {
        updateInfo = await api.checkUpdates(settings().includePrereleases).catch(() => null);
      }
    })();
  });

  function onKeydown(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key === "p") {
      e.preventDefault();
      app.panel = app.panel === "search" ? null : "search";
    } else if (mod && e.key === "s") {
      e.preventDefault();
      flushSaves();
    } else if (mod && e.key === "w" && app.activePath) {
      e.preventDefault();
      closeTab(app.activePath);
    } else if (mod && e.key === "\\") {
      e.preventDefault();
      if (app.split) app.split = null;
      else if (app.activePath) openToSide(app.activePath);
    }
  }
</script>

<svelte:window onkeydown={onKeydown} onclick={() => { menu = null; wsMenu = false; }} />

<div class="workspace">
  <header>
    <span class="brand">🐾</span>
    <div class="ws-tabs">
      {#each app.config?.workspaces ?? [] as ws (ws.path)}
        {@const active = ws.path === app.config?.workspace_path}
        {@const color = colorFor(ws.path)}
        <div
          class="ws-tab"
          class:active
          style="--ws-color: {color}"
        >
          <button class="ws-name" onclick={() => switchWorkspace(ws.path).catch((e) => toast(errorMessage(e)))}>
            <span class="ws-dot"></span>{ws.name}{ws.managed ? "" : " 📂"}
          </button>
          {#if active && (app.config?.workspaces.length ?? 0) > 1}
            <button
              class="ws-close"
              title="Close workspace (files stay on disk)"
              onclick={() => closeWorkspace(ws.path).catch((e) => toast(errorMessage(e)))}
            >×</button>
          {/if}
        </div>
      {/each}
      <button class="ghost ws-add" title="New workspace / open folder" onclick={(e) => { e.stopPropagation(); wsMenu = !wsMenu; }}>＋</button>
      {#if wsMenu}
        <div class="ctx ws-menu">
          <button onclick={() => pickAndAdd(true)}>New workspace…</button>
          <button onclick={() => pickAndAdd(false)}>Open folder…</button>
        </div>
      {/if}
    </div>
    <button class="ghost" onclick={() => (app.panel = app.panel === "search" ? null : "search")}>
      Search <kbd>⌘P</kbd>
    </button>
    <div class="spacer"></div>
    {#if syncEnabled()}
      <button
        class="ghost sync"
        class:warn={app.syncState === "offline" || app.syncState === "needs-review"}
        onclick={() => syncNow().catch((e) => toast(errorMessage(e)))}
        title="Sync now"
      >
        {SYNC_LABEL[app.syncState]}{app.syncState === "offline" && app.pendingChanges
          ? ` — ${app.pendingChanges} change${app.pendingChanges > 1 ? "s" : ""} waiting`
          : ""}
      </button>
    {:else if workspaceManaged()}
      <button
        class="ghost sync"
        onclick={() => (app.panel = "settings")}
        title="Sync is off — connect GitHub in Settings"
      >
        {app.syncState === "saving" ? "Saving…" : "Local only"}
      </button>
    {:else}
      <span class="ghost sync" title="Opened folder — PugDock edits files but never touches this folder's git">
        {app.syncState === "saving" ? "Saving…" : "Folder"}
      </span>
    {/if}
    <button class="ghost" onclick={() => (app.panel = app.panel === "history" ? null : "history")}>History</button>
    <button class="ghost" onclick={() => (app.panel = app.panel === "ai" ? null : "ai")}>AI</button>
    <button class="ghost" onclick={() => (app.panel = app.panel === "settings" ? null : "settings")}>⚙</button>
  </header>

  <div class="body">
    <aside oncontextmenu={(e) => { e.preventDefault(); showMenu(e, null); }}>
      <button class="new-note" onclick={() => quickNote().catch((e) => toast(errorMessage(e)))}>
        ＋ New note
      </button>
      <div class="aside-head">
        <span>Files</span>
        <button class="ghost" title="New file" onclick={() => menuActions.newFile(null)}>＋</button>
      </div>
      <div class="tree">
        <FileTree entries={app.tree} onmenu={showMenu} />
      </div>
    </aside>

    <main>
      {#if app.tabs.length}
        <div class="tabs">
          <div class="tab-list">
            {#each app.tabs as tab (tab.path)}
              <div
                class="tab"
                class:active={tab.path === app.activePath}
                style="--tab-color: {colorFor(tab.path)}"
              >
                <button class="tab-name" onclick={() => (app.activePath = tab.path)}>
                  {tab.dirty ? "● " : ""}{tab.name}
                </button>
                <button class="tab-close" onclick={() => closeTab(tab.path)}>×</button>
              </div>
            {/each}
          </div>
          {#if activeTab}
            <div class="tab-actions">
              {#if activeTab.path.endsWith(".md")}
                <button
                  class="ghost"
                  title={activeTab.preview ? "Edit" : "Preview"}
                  onclick={() => activeTab && (activeTab.preview = !activeTab.preview)}
                >
                  {activeTab.preview ? "✏️ Edit" : "👁 Preview"}
                </button>
              {/if}
              <button
                class="ghost"
                title="Open to the side (⌘\)"
                onclick={() => activeTab && openToSide(activeTab.path)}
              >
                ⫽ Split
              </button>
            </div>
          {/if}
        </div>
      {/if}

      {#snippet fileView(tab: Tab, preview: boolean)}
        {#if tab.kind === "text" && preview && tab.path.endsWith(".md")}
          <MarkdownView {tab} />
        {:else if tab.kind === "text"}
          <CodeEditor {tab} />
        {:else if tab.kind === "pdf"}
          <PdfViewer {tab} />
        {:else}
          <div class="img-wrap">
            <img src={`data:image;base64,${tab.content}`} alt={tab.name} />
          </div>
        {/if}
      {/snippet}

      <div class="content" class:split={!!splitTab}>
        <div class="pane">
          {#if activeTab}
            {#key `${activeTab.path}:${activeTab.version}:${activeTab.preview ? "p" : "e"}`}
              {@render fileView(activeTab, activeTab.preview)}
            {/key}
          {:else}
            <div class="empty">
              <p>🐾</p>
              <p>Open a file, drop one here, or press <kbd>⌘P</kbd> to search.</p>
            </div>
          {/if}
        </div>
        {#if splitTab && app.split}
          <div class="pane side">
            <div class="pane-head">
              <span class="pane-title">{splitTab.name}</span>
              {#if splitTab.path.endsWith(".md")}
                <button class="ghost" onclick={() => app.split && (app.split.preview = !app.split.preview)}>
                  {app.split.preview ? "✏️" : "👁"}
                </button>
              {/if}
              <button class="ghost" title="Close split" onclick={() => (app.split = null)}>×</button>
            </div>
            <div class="pane-body">
              {#key `${splitTab.path}:${splitTab.version}:${app.split.preview ? "p" : "e"}`}
                {@render fileView(splitTab, app.split.preview)}
              {/key}
            </div>
          </div>
        {/if}
      </div>
    </main>

    {#if app.panel}
      <section class="side-panel">
        <div class="panel-head">
          <span>{app.panel === "ai" ? "Ask PugDock" : app.panel[0].toUpperCase() + app.panel.slice(1)}</span>
          <button class="ghost" onclick={() => (app.panel = null)}>×</button>
        </div>
        {#if app.panel === "search"}<SearchPanel />
        {:else if app.panel === "settings"}<SettingsPanel />
        {:else if app.panel === "ai"}<AiPanel />
        {:else if app.panel === "history"}<HistoryPanel />{/if}
      </section>
    {/if}
  </div>
</div>

{#if menu}
  <div class="ctx" style="left:{menu.x}px; top:{menu.y}px">
    <button onclick={() => menu && menuActions.newFile(menu.entry)}>New file</button>
    <button onclick={() => menu && menuActions.newFolder(menu.entry)}>New folder</button>
    {#if menu.entry}
      {@const entry = menu.entry}
      <hr />
      {#if !entry.is_dir}
        <button onclick={() => openToSide(entry.path)}>Open to the side</button>
      {/if}
      <button onclick={() => menuActions.rename(entry)}>Rename / move</button>
      {#if !entry.is_dir}
        <button onclick={() => menuActions.duplicate(entry)}>Duplicate</button>
      {/if}
      <button onclick={() => menuActions.reveal(entry)}>Reveal in file manager</button>
      <hr />
      <button class="danger" onclick={() => menuActions.del(entry)}>Delete</button>
    {/if}
  </div>
{/if}

{#if modal}
  <div class="overlay" role="presentation" onclick={() => (modal = null)}>
    <form
      class="modal"
      role="presentation"
      onclick={(e) => e.stopPropagation()}
      onsubmit={(e) => {
        e.preventDefault();
        const m = modal;
        modal = null;
        if (m && m.value.trim()) m.onOk(m.value.trim());
      }}
    >
      <h4>{modal.title}</h4>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={modal.value} autofocus spellcheck="false" />
      <div class="btns">
        <button type="button" onclick={() => (modal = null)}>Cancel</button>
        <button type="submit" class="primary">OK</button>
      </div>
    </form>
  </div>
{/if}

{#if updateInfo}
  <div class="overlay" role="presentation" onclick={() => (updateInfo = null)}>
    <div class="modal" role="presentation" onclick={(e) => e.stopPropagation()}>
      <h4>A new version of PugDock is available.</h4>
      <p class="dim">Current: v{updateInfo.current} · Latest: {updateInfo.latest}</p>
      {#if updateInfo.notes}<pre class="notes">{updateInfo.notes.slice(0, 2000)}</pre>{/if}
      <div class="btns">
        <button onclick={() => (updateInfo = null)}>Later</button>
        <button
          class="primary"
          onclick={() => {
            if (updateInfo) openUrl(updateInfo.url);
            updateInfo = null;
          }}
        >
          View release
        </button>
      </div>
    </div>
  </div>
{/if}

{#if app.conflicts.length}
  <ConflictDialog />
{/if}

<AiFab />

{#if app.toast}
  <div class="toast">{app.toast}</div>
{/if}

<style>
  .workspace {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }
  header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-panel);
  }
  .brand {
    font-weight: 600;
    margin-right: 8px;
  }
  .ws-tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    position: relative;
    max-width: 50%;
    overflow-x: auto;
  }
  .ws-tab {
    display: flex;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    flex-shrink: 0;
  }
  .ws-tab.active {
    background: color-mix(in srgb, var(--ws-color) 18%, var(--bg));
    border-color: color-mix(in srgb, var(--ws-color) 55%, var(--border));
  }
  .ws-name {
    display: flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: none;
    padding: 4px 10px;
    font-size: 12px;
    color: var(--text-dim);
    white-space: nowrap;
  }
  .ws-tab.active .ws-name {
    color: var(--text);
    font-weight: 600;
  }
  .ws-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--ws-color);
    flex-shrink: 0;
  }
  .ws-close {
    background: none;
    border: none;
    color: var(--text-dim);
    padding: 2px 7px 2px 0;
  }
  .ws-close:hover {
    color: var(--danger);
  }
  .ws-add {
    flex-shrink: 0;
  }
  .ws-menu {
    position: absolute;
    top: 30px;
    right: 0;
    left: auto;
  }
  .spacer {
    flex: 1;
  }
  .sync {
    font-size: 12px;
  }
  .sync.warn {
    color: var(--warn);
  }
  kbd {
    font-size: 10px;
    color: var(--text-dim);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 4px;
  }
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  aside {
    width: 220px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    background: var(--bg-panel);
    display: flex;
    flex-direction: column;
  }
  .new-note {
    margin: 10px 12px 2px;
    padding: 9px 12px;
    font-size: 13px;
    font-weight: 600;
    background: var(--accent);
    border: none;
    border-radius: 8px;
    color: #10121a;
  }
  .new-note:hover {
    filter: brightness(1.1);
  }
  .aside-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px 4px;
    color: var(--text-dim);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
  }
  .tree {
    flex: 1;
    overflow-y: auto;
    padding-bottom: 20px;
  }
  main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .tabs {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--border);
    background: var(--bg-panel);
  }
  .tab-list {
    display: flex;
    overflow-x: auto;
    flex: 1;
    min-width: 0;
  }
  .tab-actions {
    display: flex;
    gap: 2px;
    padding: 0 8px;
    flex-shrink: 0;
  }
  .tab-actions button {
    font-size: 11px;
    padding: 3px 8px;
    white-space: nowrap;
  }
  .tab {
    display: flex;
    align-items: center;
    border-right: 1px solid var(--border);
    border-top: 2px solid color-mix(in srgb, var(--tab-color) 45%, transparent);
    flex-shrink: 0;
  }
  .tab.active {
    background: color-mix(in srgb, var(--tab-color) 12%, var(--bg));
    border-top-color: var(--tab-color);
  }
  .tab-name {
    background: none;
    border: none;
    padding: 7px 4px 7px 12px;
    font-size: 12px;
    color: var(--text-dim);
  }
  .tab.active .tab-name {
    color: var(--text);
  }
  .tab-close {
    background: none;
    border: none;
    color: var(--text-dim);
    padding: 4px 8px 4px 2px;
  }
  .tab-close:hover {
    color: var(--danger);
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
  }
  .pane {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .pane > :global(*) {
    flex: 1;
    min-height: 0;
  }
  .pane.side {
    border-left: 1px solid var(--border);
  }
  .pane-head {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 8px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-panel);
    flex: 0 0 auto;
  }
  .pane-title {
    flex: 1;
    font-size: 12px;
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pane-body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .pane-body > :global(*) {
    flex: 1;
    min-height: 0;
  }
  .empty {
    display: grid;
    place-content: center;
    text-align: center;
    color: var(--text-dim);
    height: 100%;
  }
  .empty p:first-child {
    font-size: 40px;
    margin: 0;
  }
  .img-wrap {
    display: grid;
    place-items: center;
    overflow: auto;
    height: 100%;
  }
  .img-wrap img {
    max-width: 90%;
    max-height: 90%;
  }
  .side-panel {
    width: 340px;
    flex-shrink: 0;
    border-left: 1px solid var(--border);
    background: var(--bg-panel);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .panel-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 8px 4px 14px;
    color: var(--text-dim);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
  }
  .side-panel > :global(:last-child) {
    flex: 1;
    min-height: 0;
  }
  .ctx {
    position: fixed;
    z-index: 200;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.5);
    display: flex;
    flex-direction: column;
    min-width: 180px;
    padding: 4px;
  }
  .ctx button {
    background: none;
    border: none;
    text-align: left;
    padding: 6px 10px;
    font-size: 12.5px;
    border-radius: 4px;
  }
  .ctx button:hover {
    background: var(--bg-hover);
  }
  .ctx button.danger {
    color: var(--danger);
  }
  .ctx hr {
    border: none;
    border-top: 1px solid var(--border);
    margin: 4px 0;
  }
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: grid;
    place-items: center;
    z-index: 150;
  }
  .modal {
    background: var(--bg-panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 18px;
    width: 440px;
    max-width: 90vw;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .modal h4 {
    margin: 0;
    font-size: 14px;
  }
  .modal .btns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .notes {
    max-height: 200px;
    overflow: auto;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px;
    font-size: 11px;
    white-space: pre-wrap;
  }
  .dim {
    color: var(--text-dim);
    margin: 0;
  }
  .toast {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg-active);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 8px 16px;
    font-size: 12.5px;
    z-index: 300;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
  }
</style>
