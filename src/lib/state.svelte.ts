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

/** Reload every open text tab whose file changed on disk (agent edits). */
export async function refreshOpenTabs() {
  for (const t of app.tabs) {
    if (t.kind !== "text") continue;
    const disk = await api.readFile(t.path).catch(() => null);
    if (disk !== null && disk !== t.content) replaceTabContent(t.path, disk);
  }
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
  pins: [] as string[],
  recent: [] as string[],
  syncExcluded: [] as string[],
  config: null as AppConfig | null,
  tree: [] as TreeEntry[],
  /** Open documents (shared store - panes reference them by path). */
  tabs: [] as Tab[],
  /** Editor groups, VSCode-style: each pane has its own tab strip. */
  panes: [{ paths: [], active: null }] as Pane[],
  focused: 0,
  /** Mirror of the focused pane's active path (what side panels act on). */
  activePath: null as string | null,
  /** Folder selected in the tree; New note and imports target it. */
  selectedDir: "" as string,
  syncState: "synced" as SyncUiState,
  pendingChanges: 0,
  conflicts: [] as string[],
  panel: null as null | "search" | "settings" | "history",
  toast: null as string | null,
});

export function settings(): Settings & typeof DEFAULT_SETTINGS {
  return { ...DEFAULT_SETTINGS, ...(app.config?.settings ?? {}) };
}

/** GitHub sync is optional - enabled only when a repo was linked. */
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
  // Patch on top of the freshest on-disk config so a stale in-memory copy
  // can never clobber the workspace list or repo fields.
  const fresh = await api.getConfig();
  fresh.settings = { ...fresh.settings, ...patch };
  await api.setConfig(fresh);
  app.config = fresh;
}

function treeContains(path: string, entries: TreeEntry[]): boolean {
  return entries.some((e) => e.path === path || (e.children ? treeContains(path, e.children) : false));
}

export async function refreshTree() {
  app.tree = await api.listTree();
  app.syncExcluded = await api.syncExclusions().catch(() => []);
  loadPinsAndRecent();
  // Quick lists only ever show files of the CURRENT workspace: prune
  // anything that does not exist in its tree (stale or foreign entries).
  const pruneP = app.pins.filter((p) => treeContains(p, app.tree));
  const pruneR = app.recent.filter((p) => treeContains(p, app.tree));
  if (pruneP.length !== app.pins.length) {
    app.pins = pruneP;
    localStorage.setItem(wsKey("pugdock-pins"), JSON.stringify(pruneP));
  }
  if (pruneR.length !== app.recent.length) {
    app.recent = pruneR;
    localStorage.setItem(wsKey("pugdock-recent"), JSON.stringify(pruneR));
  }
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

/** A note gets the full writing experience (toolbar, preview, markdown):
 *  extensionless files count as notes, no format required. */
export function isNoteFile(path: string): boolean {
  const name = path.split("/").pop() ?? path;
  if (!name.includes(".")) return true;
  return /\.(md|markdown|txt)$/i.test(name);
}

export function isTextFile(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return TEXT_EXTS.has(ext) || !path.includes(".");
}

function wsKey(prefix: string): string {
  return `${prefix}:${app.config?.workspace_path ?? ""}`;
}

export function loadPinsAndRecent() {
  try {
    app.pins = JSON.parse(localStorage.getItem(wsKey("pugdock-pins")) ?? "[]");
    app.recent = JSON.parse(localStorage.getItem(wsKey("pugdock-recent")) ?? "[]");
  } catch {
    app.pins = [];
    app.recent = [];
  }
}

export function togglePin(path: string) {
  if (app.pins.includes(path)) app.pins = app.pins.filter((p) => p !== path);
  else app.pins = [...app.pins, path];
  localStorage.setItem(wsKey("pugdock-pins"), JSON.stringify(app.pins));
}

function trackRecent(path: string) {
  app.recent = [path, ...app.recent.filter((p) => p !== path)].slice(0, 8);
  localStorage.setItem(wsKey("pugdock-recent"), JSON.stringify(app.recent));
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

/** VSCode semantics: opening targets the FOCUSED group. The same file may
 *  be open in several groups at once (editors mirror each other live). */
export async function openFile(path: string) {
  await loadTab(path);
  const pane = app.panes[app.focused];
  if (!pane.paths.includes(path)) pane.paths.push(path);
  focusTab(app.focused, path);
  trackRecent(path);
}

/** Duplicate a file into a target group (VSCode's "split editor"). */
export async function copyTabToPane(path: string, target: number) {
  await loadTab(path);
  while (app.panes.length <= target) app.panes.push({ paths: [], active: null });
  if (!app.panes[target].paths.includes(path)) app.panes[target].paths.push(path);
  focusTab(target, path);
}

/** Move a tab between groups (drag & drop), creating the split as needed. */
export async function moveTabToPane(path: string, target: number) {
  await loadTab(path);
  while (app.panes.length <= target) app.panes.push({ paths: [], active: null });
  const src = app.focused >= 0 && app.panes[app.focused]?.paths.includes(path)
    ? app.focused
    : app.panes.findIndex((p) => p.paths.includes(path));
  if (src === target) {
    focusTab(target, path);
    return;
  }
  if (src >= 0) removeFromPane(src, path);
  const t = Math.min(target, app.panes.length);
  while (app.panes.length <= t) app.panes.push({ paths: [], active: null });
  if (!app.panes[t].paths.includes(path)) app.panes[t].paths.push(path);
  focusTab(t, path);
}

export async function openToSide(path: string) {
  await copyTabToPane(path, 1);
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

/** Close a tab in one group; the document stays while other groups use it. */
export function closeTab(path: string, paneIndex?: number) {
  const pi = paneIndex ?? (app.panes[app.focused]?.paths.includes(path)
    ? app.focused
    : app.panes.findIndex((p) => p.paths.includes(path)));
  if (pi >= 0 && app.panes[pi]) removeFromPane(pi, path);
  if (!app.panes.some((p) => p.paths.includes(path))) {
    const i = app.tabs.findIndex((t) => t.path === path);
    if (i >= 0) app.tabs.splice(i, 1);
  }
  mirror();
}

/** Close a file everywhere (used when it is deleted). */
export function closeEverywhere(path: string) {
  while (app.panes.some((p) => p.paths.includes(path))) {
    closeTab(path, app.panes.findIndex((p) => p.paths.includes(path)));
  }
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
