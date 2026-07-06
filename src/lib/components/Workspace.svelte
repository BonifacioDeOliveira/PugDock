<script lang="ts">
  import { api, errorMessage, type TreeEntry } from "$lib/api";
  import { checkForUpdate, type AvailableUpdate } from "$lib/update";
  import { app, openFile, openToSide, closeTab, focusTab, moveTabToPane, collapseSplit, renameOpenPath, refreshTree, settings, syncEnabled, workspaceManaged, colorFor, togglePin, saveSettings, toast, type Tab } from "$lib/state.svelte";
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

  // --- tab drag & drop (VSCode-style split) ---
  let dragPath = $state<string | null>(null);
  let splitHint = $state(false);

  function onTabDragStart(e: DragEvent, path: string) {
    dragPath = path;
    e.dataTransfer?.setData("text/plain", path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }

  function onPaneDragOver(e: DragEvent, paneIndex: number) {
    if (!dragPath) return;
    e.preventDefault();
    if (app.panes.length === 1 && paneIndex === 0) {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      splitHint = e.clientX > r.left + r.width * 0.7;
    }
  }

  function onPaneDrop(e: DragEvent, paneIndex: number) {
    if (!dragPath) return;
    e.preventDefault();
    const target = splitHint && app.panes.length === 1 ? 1 : paneIndex;
    moveTabToPane(dragPath, target);
    dragPath = null;
    splitHint = false;
  }

  const SYNC_LABEL: Record<string, string> = {
    saved: "Saved",
    saving: "Saving…",
    "saved-locally": "Saved locally",
    checkpointing: "Creating checkpoint…",
    syncing: "Syncing…",
    synced: "Synced",
    offline: "Offline. Will sync later",
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
  let updateInfo = $state<AvailableUpdate | null>(null);
  let updating = $state(false);

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

  async function moveFile(from: string, toDir: string) {
    const name = from.split("/").pop() ?? from;
    const dest = toDir ? `${toDir}/${name}` : name;
    if (dest === from) return;
    await run(async () => {
      await api.renamePath(from, dest);
      renameOpenPath(from, dest);
      api.removeFromIndex(from).catch(() => {});
      api.indexFile(dest).catch(() => {});
    });
  }

  function renameTab(path: string) {
    menuActions.rename({ path, name: path.split("/").pop() ?? path, is_dir: false, children: null });
  }

  async function toggleSyncExcluded(entry: TreeEntry) {
    const excluded = app.syncExcluded.includes(entry.path);
    try {
      app.syncExcluded = await api.setSyncExcluded(entry.path, !excluded);
      toast(excluded ? `${entry.name} will sync again` : `${entry.name} is now local only`);
    } catch (e) {
      toast(errorMessage(e));
    }
  }

  function toggleAiExcluded(entry: TreeEntry) {
    const list = settings().aiExcluded;
    const excluded = list.includes(entry.path);
    saveSettings({ aiExcluded: excluded ? list.filter((p) => p !== entry.path) : [...list, entry.path] });
    toast(excluded ? `${entry.name} visible to AI again` : `${entry.name} excluded from AI`);
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
          renameOpenPath(entry.path, v);
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
      // If dropped over a folder in the tree, import there; otherwise inbox/pdfs.
      let targetDir: string | null = null;
      const pos = event.payload.position;
      const el = document.elementFromPoint(pos.x / window.devicePixelRatio, pos.y / window.devicePixelRatio);
      const row = el?.closest("[data-drop-dir]");
      if (row) targetDir = row.getAttribute("data-drop-dir");
      let last = "";
      for (const src of event.payload.paths) {
        const name = src.split(/[/\\]/).pop() ?? "file";
        const fallback = name.toLowerCase().endsWith(".pdf") ? "pdfs" : "inbox";
        const dir = targetDir ?? (workspaceManaged() ? fallback : "");
        const dest = dir ? `${dir}/${name}` : name;
        await api.importFile(src, dest).catch((e) => toast(errorMessage(e)));
        api.indexFile(dest).catch(() => {});
        last = dir || "workspace";
      }
      await refreshTree();
      toast(`Imported into ${last}`);
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
        updateInfo = await checkForUpdate(settings().includePrereleases).catch(() => null);
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
      if (app.panes.length > 1) collapseSplit();
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
          ? `, ${app.pendingChanges} change${app.pendingChanges > 1 ? "s" : ""} waiting`
          : ""}
      </button>
    {:else if workspaceManaged()}
      <button
        class="ghost sync"
        onclick={() => (app.panel = "settings")}
        title="Sync is off. Connect GitHub in Settings"
      >
        {app.syncState === "saving" ? "Saving…" : "Local only"}
      </button>
    {:else}
      <span class="ghost sync" title="Opened folder: PugDock edits files but never touches this folder's git">
        {app.syncState === "saving" ? "Saving…" : "Folder"}
      </span>
    {/if}
    <button class="ghost" onclick={() => (app.panel = app.panel === "history" ? null : "history")}>History</button>
    <button class="ghost" onclick={() => (app.panel = app.panel === "ai" ? null : "ai")}>AI</button>
    <button
      class="ghost settings-btn"
      title="Settings"
      onclick={() => (app.panel = app.panel === "settings" ? null : "settings")}
    >
      <svg viewBox="0 0 24 24" width="19" height="19" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
      </svg>
    </button>
  </header>

  <div class="body">
    <aside oncontextmenu={(e) => { e.preventDefault(); showMenu(e, null); }}>
      <div class="create-row">
        <button class="new-note" onclick={() => quickNote().catch((e) => toast(errorMessage(e)))}>
          <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
            <line x1="12" y1="18" x2="12" y2="12" />
            <line x1="9" y1="15" x2="15" y2="15" />
          </svg>
          New note
        </button>
        <button class="new-folder" title="New folder" onclick={() => menuActions.newFolder(null)}>
          <svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
            <line x1="12" y1="11" x2="12" y2="17" />
            <line x1="9" y1="14" x2="15" y2="14" />
          </svg>
        </button>
      </div>
      {#if app.pins.length}
        <div class="aside-head"><span>Pinned</span></div>
        <div class="quick-list">
          {#each app.pins as p (p)}
            <button class="quick-item" onclick={() => openFile(p)} title={p}>📌 {p.split("/").pop()}</button>
          {/each}
        </div>
      {/if}
      {#if app.recent.length > 1}
        <div class="aside-head"><span>Recent</span></div>
        <div class="quick-list">
          {#each app.recent.slice(0, 5) as p (p)}
            <button class="quick-item" onclick={() => openFile(p)} title={p}>{p.split("/").pop()}</button>
          {/each}
        </div>
      {/if}
      <div class="aside-head">
        <span>Files</span>
        <button class="ghost" title="New file" onclick={() => menuActions.newFile(null)}>＋</button>
      </div>
      <div class="tree">
        <FileTree entries={app.tree} onmenu={showMenu} onrename={(e) => menuActions.rename(e)} onmove={moveFile} />
      </div>
    </aside>

    <main>
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

      <div class="content">
        {#each app.panes as pane, pi (pi)}
          {@const paneTab = app.tabs.find((t) => t.path === pane.active)}
          <div
            class="pane"
            class:focused={app.panes.length > 1 && app.focused === pi}
            role="group"
            ondragover={(e) => onPaneDragOver(e, pi)}
            ondragleave={() => (splitHint = false)}
            ondrop={(e) => onPaneDrop(e, pi)}
          >
            {#if pane.paths.length}
              <div class="tabs">
                <div class="tab-list">
                  {#each pane.paths as path (path)}
                    {@const tab = app.tabs.find((t) => t.path === path)}
                    {#if tab}
                      <div
                        class="tab"
                        class:active={path === pane.active}
                        style="--tab-color: {colorFor(path)}"
                        draggable="true"
                        role="presentation"
                        ondragstart={(e) => onTabDragStart(e, path)}
                        ondragend={() => {
                          dragPath = null;
                          splitHint = false;
                        }}
                      >
                        <button class="tab-name" onclick={() => focusTab(pi, path)} ondblclick={() => renameTab(path)}>
                          {tab.dirty ? "● " : ""}{tab.name}
                        </button>
                        <button class="tab-close" onclick={() => closeTab(path)}>×</button>
                      </div>
                    {/if}
                  {/each}
                </div>
                {#if paneTab}
                  <div class="tab-actions">
                    {#if paneTab.path.endsWith(".md")}
                      <button
                        class="ghost"
                        title={paneTab.preview ? "Edit" : "Preview"}
                        onclick={() => (paneTab.preview = !paneTab.preview)}
                      >
                        {paneTab.preview ? "✏️ Edit" : "👁 Preview"}
                      </button>
                    {/if}
                    <button
                      class="ghost"
                      title="Move to the other group (⌘\)"
                      onclick={() => moveTabToPane(paneTab.path, pi === 0 ? 1 : 0)}
                    >
                      ⫽
                    </button>
                  </div>
                {/if}
              </div>
            {/if}
            <div class="pane-content">
              {#if paneTab}
                {#key `${paneTab.path}:${paneTab.version}:${paneTab.preview ? "p" : "e"}`}
                  {@render fileView(paneTab, paneTab.preview)}
                {/key}
              {:else}
                <div class="empty">
                  <p>🐾</p>
                  <p>Open a file, drop one here, or press <kbd>⌘P</kbd> to search.</p>
                </div>
              {/if}
            </div>
            {#if splitHint && pi === 0 && app.panes.length === 1}
              <div class="drop-right"></div>
            {/if}
          </div>
        {/each}
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
      <button onclick={() => togglePin(entry.path)}>
        {app.pins.includes(entry.path) ? "Unpin" : "Pin"}
      </button>
      {#if workspaceManaged()}
        <button onclick={() => toggleSyncExcluded(entry)}>
          {app.syncExcluded.includes(entry.path) ? "Include in sync" : "Exclude from sync (local only)"}
        </button>
      {/if}
      <button onclick={() => toggleAiExcluded(entry)}>
        {settings().aiExcluded.includes(entry.path) ? "Include in AI" : "Exclude from AI"}
      </button>
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
      <p class="dim">Latest: v{updateInfo.version}</p>
      {#if updateInfo.notes}<pre class="notes">{updateInfo.notes.slice(0, 2000)}</pre>{/if}
      <div class="btns">
        <button onclick={() => (updateInfo = null)} disabled={updating}>Later</button>
        {#if updateInfo.install}
          <button
            class="primary"
            disabled={updating}
            onclick={async () => {
              if (!updateInfo?.install) return;
              updating = true;
              try {
                await updateInfo.install();
              } catch (e) {
                toast(errorMessage(e));
                updating = false;
              }
            }}
          >
            {updating ? "Updating…" : "Update now"}
          </button>
        {:else if updateInfo.url}
          <button
            class="primary"
            onclick={() => {
              if (updateInfo?.url) openUrl(updateInfo.url);
              updateInfo = null;
            }}
          >
            View release
          </button>
        {/if}
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
  .settings-btn {
    display: flex;
    align-items: center;
    padding: 5px 8px;
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
  .create-row {
    display: flex;
    gap: 6px;
    margin: 10px 12px 2px;
  }
  .new-note {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 9px 12px;
    font-size: 13px;
    font-weight: 600;
    background: var(--accent);
    border: none;
    border-radius: 8px;
    color: #10121a;
    transition: filter 0.12s ease, transform 0.12s ease;
  }
  .new-note:hover {
    filter: brightness(1.1);
    transform: translateY(-1px);
  }
  .new-folder {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 38px;
    padding: 0;
    background: var(--bg-hover);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-dim);
    transition: color 0.12s ease, transform 0.12s ease;
  }
  .new-folder:hover {
    color: var(--text);
    transform: translateY(-1px);
  }
  .quick-list {
    display: flex;
    flex-direction: column;
    padding-bottom: 4px;
  }
  .quick-item {
    background: none;
    border: none;
    text-align: left;
    padding: 3px 14px;
    font-size: 12px;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .quick-item:hover {
    background: var(--bg-hover);
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
  .pane {
    position: relative;
  }
  .pane + .pane {
    border-left: 1px solid var(--border);
  }
  .pane.focused {
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .pane-content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .pane-content > :global(*) {
    flex: 1;
    min-height: 0;
  }
  .drop-right {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 30%;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    border-left: 2px solid var(--accent);
    pointer-events: none;
    z-index: 5;
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
