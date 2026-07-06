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

export interface Tab {
  path: string;
  name: string;
  kind: "text" | "pdf" | "image";
  content: string; // text content, or base64 for pdf/image
  dirty: boolean;
  preview: boolean; // markdown rendered view instead of editor
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
  tabs: [] as Tab[],
  activePath: null as string | null,
  /** Right-hand split pane: a path from `tabs`, with its own preview mode. */
  split: null as { path: string; preview: boolean } | null,
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
  return !!app.config?.repo_name;
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
  const tab: Tab = { path, name: path.split("/").pop() ?? path, kind, content, dirty: false, preview: false };
  app.tabs.push(tab);
  return tab;
}

export async function openFile(path: string) {
  await loadTab(path);
  app.activePath = path;
}

export async function openToSide(path: string) {
  await loadTab(path);
  // Same file on both sides: right pane becomes a live preview (md) to avoid
  // two editors fighting over one document.
  const forcePreview = path === app.activePath && path.endsWith(".md");
  app.split = { path, preview: forcePreview };
}

export function closeTab(path: string) {
  if (app.split?.path === path) app.split = null;
  const i = app.tabs.findIndex((t) => t.path === path);
  if (i === -1) return;
  app.tabs.splice(i, 1);
  if (app.activePath === path) {
    app.activePath = app.tabs[Math.min(i, app.tabs.length - 1)]?.path ?? null;
  }
}
