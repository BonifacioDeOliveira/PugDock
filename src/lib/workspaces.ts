import { api } from "./api";
import { app, refreshTree, settings, syncEnabled, workspaceManaged, type Tab, type Pane } from "./state.svelte";
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
  await refreshTree().catch(() => {});
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

/** The on-disk home of the synced repository (chosen once in onboarding). */
export function repoRoot(): string | null {
  return app.config?.workspaces.find((w) => w.managed)?.path ?? app.config?.workspace_path ?? null;
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
