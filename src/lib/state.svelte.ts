import { api, type AppConfig, type Settings, type TreeEntry, type SyncStatus } from "./api";

export type SyncUiState =
  | "saved"
  | "saving"
  | "saved-locally"
  | "checkpointing"
  | "syncing"
  | "synced"
  | "offline"
  | "needs-review";

export interface Pane {
  paths: string[];
  active: string | null;
}

export interface Tab {
  path: string;
  name: string;
  kind: "text" | "pdf" | "image";
  content: string; // text content, or base64 for pdf/image
  dirty: boolean;
  preview: boolean; // markdown rendered view instead of editor
  /** bumped when content is replaced from outside the editor (AI, restore…) */
  version: number;
}

/** Replace a tab's content from outside the editor and force it to re-render. */
export function replaceTabContent(path: string, content: string) {
  const tab = app.tabs.find((t) => t.path === path);
  if (tab) {
    tab.content = content;
    tab.version++;
  }
}

export const DEFAULT_SETTINGS: Required<Pick<
  Settings,
  | "syncMode" | "checkpointSeconds" | "pushSeconds" | "pullOnStartup" | "pushOnExit"
  | "aiEnabled" | "modelMode" | "askBeforeSendingCode" | "askBeforeSendingPdfs"
  | "aiExcluded" | "autoCheckUpdates" | "includePrereleases"
>> = {
  syncMode: "smart",
  checkpointSeconds: 60,
  pushSeconds: 240,
  pullOnStartup: true,
  pushOnExit: true,
  aiEnabled: false,
  modelMode: "auto",
  askBeforeSendingCode: false,
  askBeforeSendingPdfs: true,
  aiExcluded: [],
  autoCheckUpdates: true,
  includePrereleases: false,
};

export const app = $state({
  loaded: false,
  config: null as AppConfig | null,
  tree: [] as TreeEntry[],
  /** Open documents (shared store — panes reference them by path). */
  tabs: [] as Tab[],
  /** Editor groups, VSCode-style: each pane has its own tab strip. */
  panes: [{ paths: [], active: null }] as Pane[],
  focused: 0,
  /** Mirror of the focused pane's active path (what side panels act on). */
  activePath: null as string | null,
  syncState: "synced" as SyncUiState,
  pendingChanges: 0,
  conflicts: [] as string[],
  panel: null as null | "search" | "settings" | "ai" | "history",
  toast: null as string | null,
});

export function settings(): Settings & typeof DEFAULT_SETTINGS {
  return { ...DEFAULT_SETTINGS, ...(app.config?.settings ?? {}) };
}

/** GitHub sync is optional — enabled only when a repo was linked. */
export function syncEnabled(): boolean {
  return !!app.config?.repo_name && workspaceManaged();
}

/** true for PugDock workspaces; false for opened folders (code-editor mode),
 *  where PugDock must never run git or scaffold anything. */
export function workspaceManaged(): boolean {
  const path = app.config?.workspace_path;
  if (!path) return true;
  return app.config?.workspaces?.find((w) => w.path === path)?.managed ?? true;
}

/** Stable, distinct-looking color per string (workspace path, file path…). */
export function colorFor(s: string): string {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0;
  return `hsl(${h % 360} 62% 58%)`;
}

export async function saveSettings(patch: Partial<Settings>) {
  if (!app.config) return;
  app.config.settings = { ...app.config.settings, ...patch };
  await api.setConfig($state.snapshot(app.config) as AppConfig);
}

export async function refreshTree() {
  app.tree = await api.listTree();
}

export function toast(msg: string) {
  app.toast = msg;
  setTimeout(() => (app.toast = null), 4000);
}

export function applyStatus(st: SyncStatus) {
  if (!syncEnabled()) {
    app.syncState = "saved";
    return;
  }
  app.conflicts = st.conflicts;
  if (st.conflicts.length > 0) {
    app.syncState = "needs-review";
  } else if (st.dirty || st.ahead > 0) {
    app.pendingChanges = st.ahead;
    app.syncState = "saved-locally";
  } else {
    app.syncState = "synced";
  }
}

const TEXT_EXTS = new Set([
  "md", "txt", "json", "yaml", "yml", "toml", "py", "js", "ts", "tsx", "jsx", "go",
  "rs", "java", "cs", "sql", "html", "css", "log", "csv", "sh", "fish", "example", "env",
  "xml", "ini", "conf", "cfg", "gitignore",
]);

export function fileKind(path: string): Tab["kind"] {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (ext === "pdf") return "pdf";
  if (["png", "jpg", "jpeg", "gif", "webp", "svg"].includes(ext)) return "image";
  return "text";
}

export function isTextFile(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return TEXT_EXTS.has(ext) || !path.includes(".");
}

async function loadTab(path: string): Promise<Tab> {
  const existing = app.tabs.find((t) => t.path === path);
  if (existing) return existing;
  const kind = fileKind(path);
  const content = kind === "text" ? await api.readFile(path) : await api.readFileBase64(path);
  const tab: Tab = { path, name: path.split("/").pop() ?? path, kind, content, dirty: false, preview: false, version: 0 };
  app.tabs.push(tab);
  return tab;
}

function mirror() {
  app.activePath = app.panes[app.focused]?.active ?? null;
}

export function focusTab(paneIndex: number, path: string) {
  app.focused = paneIndex;
  app.panes[paneIndex].active = path;
  mirror();
}

export async function openFile(path: string) {
  await loadTab(path);
  const already = app.panes.findIndex((p) => p.paths.includes(path));
  if (already >= 0) {
    focusTab(already, path);
    return;
  }
  app.panes[app.focused].paths.push(path);
  focusTab(app.focused, path);
}

/** Move (or open) a file into a target pane, creating the split as needed. */
export async function moveTabToPane(path: string, target: number) {
  await loadTab(path);
  while (app.panes.length <= target) app.panes.push({ paths: [], active: null });
  const src = app.panes.findIndex((p) => p.paths.includes(path));
  if (src === target) {
    focusTab(target, path);
    return;
  }
  if (src >= 0) removeFromPane(src, path);
  // pane indexes may have shifted if a pane collapsed
  const t = Math.min(target, app.panes.length);
  while (app.panes.length <= t) app.panes.push({ paths: [], active: null });
  if (!app.panes[t].paths.includes(path)) app.panes[t].paths.push(path);
  focusTab(t, path);
}

export async function openToSide(path: string) {
  await moveTabToPane(path, 1);
}

/** Merge the right group back into the left one. */
export function collapseSplit() {
  if (app.panes.length < 2) return;
  const [left, right] = app.panes;
  for (const p of right.paths) if (!left.paths.includes(p)) left.paths.push(p);
  left.active = right.active ?? left.active;
  app.panes = [left];
  app.focused = 0;
  mirror();
}

function removeFromPane(paneIndex: number, path: string) {
  const pane = app.panes[paneIndex];
  const i = pane.paths.indexOf(path);
  if (i === -1) return;
  pane.paths.splice(i, 1);
  if (pane.active === path) {
    pane.active = pane.paths[Math.min(i, pane.paths.length - 1)] ?? null;
  }
  if (app.panes.length > 1 && pane.paths.length === 0) {
    app.panes.splice(paneIndex, 1);
    if (app.focused >= app.panes.length) app.focused = app.panes.length - 1;
  }
  mirror();
}

export function closeTab(path: string) {
  const paneIndex = app.panes.findIndex((p) => p.paths.includes(path));
  if (paneIndex >= 0) removeFromPane(paneIndex, path);
  if (!app.panes.some((p) => p.paths.includes(path))) {
    const i = app.tabs.findIndex((t) => t.path === path);
    if (i >= 0) app.tabs.splice(i, 1);
  }
  mirror();
}

/** Keep pane references and the mirror consistent when a file is renamed. */
export function renameOpenPath(from: string, to: string) {
  const tab = app.tabs.find((t) => t.path === from);
  if (tab) {
    tab.path = to;
    tab.name = to.split("/").pop() ?? to;
  }
  for (const pane of app.panes) {
    const i = pane.paths.indexOf(from);
    if (i >= 0) pane.paths[i] = to;
    if (pane.active === from) pane.active = to;
  }
  mirror();
}
