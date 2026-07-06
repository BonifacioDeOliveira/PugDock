import { api } from "./api";
import { app, refreshTree, settings, syncEnabled, workspaceManaged, type Tab } from "./state.svelte";
import { flushSaves, startSync, syncNow } from "./sync";

// Per-workspace UI state, kept while the app is open so switching tabs
// restores exactly what you had (open files, active file, split).
const uiCache = new Map<string, { tabs: Tab[]; activePath: string | null; split: typeof app.split }>();

function stashCurrent() {
  const current = app.config?.workspace_path;
  if (current) {
    uiCache.set(current, { tabs: app.tabs, activePath: app.activePath, split: app.split });
  }
}

async function activate(path: string) {
  const cached = uiCache.get(path);
  app.tabs = cached?.tabs ?? [];
  app.activePath = cached?.activePath ?? null;
  app.split = cached?.split ?? null;
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

/** New PugDock workspace (scaffolded, local checkpoints) or opened folder. */
export async function addWorkspace(path: string, managed: boolean) {
  await flushSaves().catch(() => {});
  stashCurrent();
  app.config = await api.addWorkspace(path, managed);
  if (managed) {
    // Local checkpoint history for new workspaces; ignore if git is missing.
    await api.gitInitWorkspace(null, "PugDock", "pugdock@local").catch(() => {});
  }
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
