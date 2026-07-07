import { api } from "./api";
import { app, refreshTree, settings, syncEnabled, workspaceManaged, toast, type Tab, type Pane } from "./state.svelte";
import { flushSaves, startSync, syncNow } from "./sync";

// Per-workspace UI state, kept while the app is open so switching tabs
// restores exactly what you had (open files, editor groups, focus).
const uiCache = new Map<string, { tabs: Tab[]; panes: Pane[]; focused: number }>();

function stashCurrent() {
  const current = app.config?.workspace_path;
  if (current) {
    uiCache.set(current, { tabs: app.tabs, panes: app.panes, focused: app.focused });
  }
}

async function activate(path: string) {
  const cached = uiCache.get(path);
  app.tabs = cached?.tabs ?? [];
  app.panes = cached?.panes ?? [{ paths: [], active: null }];
  app.focused = cached?.focused ?? 0;
  app.activePath = app.panes[app.focused]?.active ?? null;
  app.conflicts = [];
  app.pendingChanges = 0;
  app.syncState = "saved";
  app.tree = [];
  app.selectedDir = "";
  await refreshTree().catch(() => {
    toast("Could not load this workspace's files.");
  });
  startSync();
  if (syncEnabled() && settings().pullOnStartup) syncNow().catch(() => {});
  api.rebuildIndex().catch(() => {});
}

export async function switchWorkspace(path: string) {
  if (path === app.config?.workspace_path) return;
  await flushSaves().catch(() => {});
  if (workspaceManaged()) api.gitCheckpoint().catch(() => {});
  stashCurrent();
  app.config = await api.setActiveWorkspace(path);
  await activate(path);
}

/** The sync root: the TOPMOST managed workspace, where .git and the
 *  remote live. New workspaces and GitHub linking always target it. */
export function repoRoot(): string | null {
  const managed = (app.config?.workspaces ?? []).filter((w) => w.managed);
  if (!managed.length) return app.config?.workspace_path ?? null;
  return managed.reduce((a, b) => (a.path.split("/").length <= b.path.split("/").length ? a : b)).path;
}

/** Create a named workspace inside the synced repo root. No location to
 *  pick and no git of its own: the root repository syncs everything. */
export async function addWorkspace(name: string) {
  const root = repoRoot();
  if (!root) throw new Error("No workspace root yet.");
  const path = `${root}/${name}`;
  await flushSaves().catch(() => {});
  stashCurrent();
  app.config = await api.addWorkspace(path, true);
  await activate(path);
}

export async function closeWorkspace(path: string) {
  uiCache.delete(path);
  app.config = await api.removeWorkspace(path);
  if (!app.config.workspace_path) {
    location.reload(); // back to onboarding
    return;
  }
  await activate(app.config.workspace_path);
}
