import { invoke } from "@tauri-apps/api/core";

// ---- Types mirrored from Rust ----

export interface AppError {
  code: string;
  message: string;
}

export interface WorkspaceEntry {
  path: string;
  name: string;
  repo_owner: string | null;
  repo_name: string | null;
  managed: boolean;
}

export interface AppConfig {
  workspace_path: string | null;
  repo_owner: string | null;
  repo_name: string | null;
  workspaces: WorkspaceEntry[];
  onboarding_done: boolean;
  settings: Settings;
}

export interface Settings {
  syncMode?: "smart" | "manual" | "frequent";
  checkpointSeconds?: number;
  pushSeconds?: number;
  pullOnStartup?: boolean;
  pushOnExit?: boolean;
  aiEnabled?: boolean;
  modelMode?: "auto" | "fast" | "balanced" | "deep" | "custom";
  customModels?: { fast?: string; default?: string; deep?: string };
  askBeforeSendingCode?: boolean;
  askBeforeSendingPdfs?: boolean;
  aiExcluded?: string[];
  autoCheckUpdates?: boolean;
  includePrereleases?: boolean;
  themeId?: string;
  repoHtmlUrl?: string;
  githubLogin?: string;
}

export interface TreeEntry {
  name: string;
  path: string;
  is_dir: boolean;
  children: TreeEntry[] | null;
}

export interface SyncStatus {
  dirty: boolean;
  ahead: number;
  behind: number;
  conflicts: string[];
  merging: boolean;
}

export interface Checkpoint {
  hash: string;
  message: string;
  date: string;
}

export interface SearchHit {
  path: string;
  snippet: string;
}

export interface DeviceCode {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
}

export interface GithubUser {
  login: string;
  id: number;
  name: string | null;
  avatar_url: string;
}

export interface CreatedRepo {
  full_name: string;
  clone_url: string;
  html_url: string;
  private: boolean;
}

export interface Model {
  id: string;
  display_name: string;
}

export interface UpdateInfo {
  current: string;
  latest: string;
  notes: string;
  url: string;
}

export interface ThemeMeta {
  id: string;
  name: string;
  dark: boolean;
}

export interface TokenColor {
  scope?: string | string[];
  settings?: { foreground?: string; fontStyle?: string };
}

export interface ImportedTheme extends ThemeMeta {
  colors: Record<string, string>;
  tokenColors: TokenColor[];
}

export interface FolderInspection {
  exists: boolean;
  is_empty: boolean;
  is_git_repo: boolean;
}

export function errorMessage(e: unknown): string {
  const err = e as AppError;
  return err?.message ?? String(e);
}

export function errorCode(e: unknown): string {
  return (e as AppError)?.code ?? "other";
}

// ---- Commands ----

export const api = {
  // config / workspace
  getConfig: () => invoke<AppConfig>("get_app_config"),
  setConfig: (config: AppConfig) => invoke<void>("set_app_config", { config }),
  inspectFolder: (path: string) => invoke<FolderInspection>("inspect_folder", { path }),
  createWorkspace: (path: string) => invoke<void>("create_workspace", { path }),
  addWorkspace: (path: string, managed: boolean) =>
    invoke<AppConfig>("add_workspace", { path, managed }),
  setActiveWorkspace: (path: string) => invoke<AppConfig>("set_active_workspace", { path }),
  removeWorkspace: (path: string) => invoke<AppConfig>("remove_workspace", { path }),
  listTree: () => invoke<TreeEntry[]>("list_tree"),
  readFile: (path: string) => invoke<string>("read_file", { path }),
  readFileBase64: (path: string) => invoke<string>("read_file_base64", { path }),
  writeFile: (path: string, content: string) => invoke<void>("write_file", { path, content }),
  createFolder: (path: string) => invoke<void>("create_folder", { path }),
  renamePath: (from: string, to: string) => invoke<void>("rename_path", { from, to }),
  deletePath: (path: string) => invoke<void>("delete_path", { path }),
  duplicateFile: (path: string) => invoke<string>("duplicate_file", { path }),
  importFile: (source: string, dest: string) => invoke<void>("import_file", { source, dest }),
  reveal: (path: string) => invoke<void>("reveal_in_file_manager", { path }),

  // secrets
  hasSecret: (name: string) => invoke<boolean>("has_secret", { name }),
  deleteSecret: (name: string) => invoke<void>("delete_secret", { name }),

  // git / sync
  gitInitWorkspace: (remoteUrl: string | null, userName: string, userEmail: string) =>
    invoke<void>("git_init_workspace", { remoteUrl, userName, userEmail }),
  gitCheckpoint: (message?: string) => invoke<boolean>("git_checkpoint", { message: message ?? null }),
  gitPush: () => invoke<void>("git_push"),
  gitPull: () => invoke<SyncStatus>("git_pull"),
  gitResolveConflict: (path: string, keep: "local" | "github") =>
    invoke<SyncStatus>("git_resolve_conflict", { path, keep }),
  gitConflictVersions: (path: string) => invoke<[string, string]>("git_conflict_versions", { path }),
  gitStatus: () => invoke<SyncStatus>("git_status"),
  gitHistory: (path?: string, limit?: number) =>
    invoke<Checkpoint[]>("git_history", { path: path ?? null, limit: limit ?? null }),
  gitFileAt: (hash: string, path: string) => invoke<string>("git_file_at", { hash, path }),

  // search
  rebuildIndex: () => invoke<number>("rebuild_index"),
  indexFile: (path: string, content?: string) =>
    invoke<void>("index_file", { path, content: content ?? null }),
  removeFromIndex: (path: string) => invoke<void>("remove_from_index", { path }),
  search: (query: string) => invoke<SearchHit[]>("search_workspace", { query }),
  searchContext: (query: string, limit = 12) =>
    invoke<[string, string][]>("search_context", { query, limit }),
  folderContents: (prefixes: string[], limit = 30) =>
    invoke<[string, string][]>("folder_contents", { prefixes, limit }),

  // github
  githubAuthMode: () => invoke<"browser" | "device" | "unconfigured">("github_auth_mode"),
  githubOauthStart: () => invoke<void>("github_oauth_start"),
  githubDeviceStart: () => invoke<DeviceCode>("github_device_start"),
  githubDevicePoll: (deviceCode: string) => invoke<string>("github_device_poll", { deviceCode }),
  githubUser: () => invoke<GithubUser>("github_user"),
  githubOrgs: () => invoke<{ login: string }[]>("github_orgs"),
  githubRepoExists: (owner: string, name: string) =>
    invoke<boolean>("github_repo_exists", { owner, name }),
  githubCreateRepo: (owner: string, name: string, isOrg: boolean) =>
    invoke<CreatedRepo>("github_create_repo", { owner, name, isOrg }),

  // anthropic
  anthropicConnect: (apiKey: string) => invoke<Model[]>("anthropic_connect", { apiKey }),
  anthropicAuthStatus: () => invoke<"key" | "oauth" | "ant" | "none">("anthropic_auth_status"),
  anthropicOauthLogin: () => invoke<Model[]>("anthropic_oauth_login"),
  anthropicModels: () => invoke<Model[]>("anthropic_models"),
  anthropicRun: (model: string, system: string, prompt: string, maxTokens?: number) =>
    invoke<string>("anthropic_run", { model, system, prompt, maxTokens: maxTokens ?? null }),

  // themes
  importVsixTheme: (vsixPath: string) => invoke<ThemeMeta[]>("import_vsix_theme", { vsixPath }),
  listImportedThemes: () => invoke<ThemeMeta[]>("list_imported_themes"),
  getImportedTheme: (id: string) => invoke<ImportedTheme>("get_imported_theme", { id }),
  deleteImportedTheme: (id: string) => invoke<void>("delete_imported_theme", { id }),

  // updates
  checkUpdates: (includePrerelease?: boolean) =>
    invoke<UpdateInfo | null>("check_updates", { includePrerelease: includePrerelease ?? null }),
};
