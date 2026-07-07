use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

const GITIGNORE: &str = "\
# PugDock local state - never synced
.pugdock/
*.sqlite

# Secrets - never synced
.env
.env.*
!.env.example
*.pem
*.key
id_rsa
id_ed25519
credentials.*
secrets.*
token
tokens
*.token

.DS_Store
";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceEntry {
    pub path: String,
    pub name: String,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    /// true = PugDock-managed (scaffold, checkpoints, sync).
    /// false = an opened folder - PugDock never touches its git or structure.
    pub managed: bool,
}

/// App-level config, stored in the OS app-config dir (never in the workspace repo).
/// The legacy top-level fields (`workspace_path`, `repo_owner`, `repo_name`)
/// always mirror the ACTIVE workspace so existing frontend code keeps working.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub workspace_path: Option<String>,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    pub workspaces: Vec<WorkspaceEntry>,
    pub onboarding_done: bool,
    /// Free-form UI settings owned by the frontend (sync intervals, AI prefs, …).
    pub settings: serde_json::Value,
}

impl AppConfig {
    /// Migrate single-workspace configs and keep mirror fields consistent.
    fn normalize(&mut self) {
        if self.workspaces.is_empty() {
            if let Some(path) = &self.workspace_path {
                self.workspaces.push(WorkspaceEntry {
                    path: path.clone(),
                    name: self
                        .repo_name
                        .clone()
                        .or_else(|| Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()))
                        .unwrap_or_else(|| "Workspace".into()),
                    repo_owner: self.repo_owner.clone(),
                    repo_name: self.repo_name.clone(),
                    managed: true,
                });
            }
        }
        // Push legacy (active-workspace) fields into the matching entry.
        if let Some(path) = self.workspace_path.clone() {
            if let Some(entry) = self.workspaces.iter_mut().find(|w| w.path == path) {
                entry.repo_owner = self.repo_owner.clone();
                entry.repo_name = self.repo_name.clone();
            }
        }
    }
}

/// The sync root: the topmost managed workspace (where .git and the
/// remote live). Every other workspace is a folder inside it.
pub fn sync_root(cfg: &AppConfig) -> Option<PathBuf> {
    cfg.workspaces
        .iter()
        .filter(|w| w.managed)
        .map(|w| PathBuf::from(&w.path))
        .min_by_key(|p| p.components().count())
}

const REGISTRY: &str = ".pugdock-workspaces.json";

/// The workspace list is part of the synced data: written at the sync root
/// so other devices reconstruct the same tabs.
fn write_registry(cfg: &AppConfig) {
    let Some(root) = sync_root(cfg) else { return };
    let entries: Vec<serde_json::Value> = cfg
        .workspaces
        .iter()
        .filter(|w| w.managed)
        .filter_map(|w| {
            let rel = Path::new(&w.path).strip_prefix(&root).ok()?;
            Some(serde_json::json!({ "name": w.name, "path": rel.to_string_lossy().replace('\\', "/") }))
        })
        .collect();
    let body = serde_json::to_string_pretty(&entries).unwrap_or_default() + "\n";
    // avoid needless checkpoint churn
    if fs::read_to_string(root.join(REGISTRY)).map(|c| c == body).unwrap_or(false) {
        return;
    }
    let _ = fs::write(root.join(REGISTRY), body);
}

/// Adopt workspaces found in the synced registry (e.g. after cloning on a
/// new device): entries missing locally are added, never removed.
fn merge_registry(cfg: &mut AppConfig) {
    let Some(root) = sync_root(cfg) else { return };
    let Ok(content) = fs::read_to_string(root.join(REGISTRY)) else { return };
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&content) else { return };
    let (owner, repo) = cfg
        .workspaces
        .iter()
        .find(|w| Path::new(&w.path) == root)
        .map(|w| (w.repo_owner.clone(), w.repo_name.clone()))
        .unwrap_or((None, None));
    for e in entries {
        let Some(rel) = e["path"].as_str() else { continue };
        let abs = if rel.is_empty() { root.clone() } else { root.join(rel) };
        let abs_s = abs.to_string_lossy().to_string();
        if abs.is_dir() && !cfg.workspaces.iter().any(|w| w.path == abs_s) {
            cfg.workspaces.push(WorkspaceEntry {
                path: abs_s,
                name: e["name"].as_str().unwrap_or("Workspace").into(),
                repo_owner: owner.clone(),
                repo_name: repo.clone(),
                managed: true,
            });
        }
    }
}

fn config_file(app: &tauri::AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Other(e.to_string()))?;
    fs::create_dir_all(&dir)?;
    Ok(dir.join("config.json"))
}

pub fn load_config(app: &tauri::AppHandle) -> Result<AppConfig> {
    let path = config_file(app)?;
    let mut cfg: AppConfig = match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    };
    cfg.normalize();
    merge_registry(&mut cfg);
    Ok(cfg)
}

pub fn save_config(app: &tauri::AppHandle, cfg: &AppConfig) -> Result<()> {
    let mut cfg = cfg.clone();
    cfg.normalize();
    write_registry(&cfg);
    let path = config_file(app)?;
    fs::write(path, serde_json::to_string_pretty(&cfg).unwrap())?;
    Ok(())
}

pub fn workspace_root(app: &tauri::AppHandle) -> Result<PathBuf> {
    load_config(app)?
        .workspace_path
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .ok_or(AppError::NoWorkspace)
}

/// Join a UI-supplied relative path onto the workspace root, rejecting escapes.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf> {
    let rel = rel.trim_start_matches('/');
    if rel.split('/').any(|c| c == "..") {
        return Err(AppError::PathOutsideWorkspace);
    }
    Ok(root.join(rel))
}

#[derive(Serialize)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Option<Vec<TreeEntry>>,
}

/// Heavy build/dependency dirs - skipped in the tree and the search index so
/// opened code folders stay fast.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "dist", "build", ".next", ".nuxt", "__pycache__",
    ".venv", "venv", ".gradle", "Pods", "DerivedData", "vendor", ".svelte-kit",
];

fn read_tree(dir: &Path, root: &Path, skip_abs: &[PathBuf]) -> Result<Vec<TreeEntry>> {
    let mut entries: Vec<TreeEntry> = Vec::new();
    for e in fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().to_string();
        // Hidden files (.gitignore, .git, .pugdock, .DS_Store, ...) are
        // plumbing, not notes: never shown in the tree.
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let path = e.path();
        // Other workspaces living under this one stay out of its view.
        if skip_abs.iter().any(|s| s == &path) {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        let is_dir = path.is_dir();
        entries.push(TreeEntry {
            children: if is_dir { Some(read_tree(&path, root, skip_abs)?) } else { None },
            name,
            path: rel,
            is_dir,
        });
    }
    entries.sort_by(|a, b| (!a.is_dir, a.name.to_lowercase()).cmp(&(!b.is_dir, b.name.to_lowercase())));
    Ok(entries)
}

/// Paths of OTHER workspaces that live inside `root` (must be hidden from it).
pub fn nested_workspace_paths(app: &tauri::AppHandle, root: &Path) -> Vec<PathBuf> {
    load_config(app)
        .map(|cfg| {
            cfg.workspaces
                .iter()
                .map(|w| PathBuf::from(&w.path))
                .filter(|p| p != root && p.starts_with(root))
                .collect()
        })
        .unwrap_or_default()
}

/// Create the standard PugDock folder structure inside `path` (idempotent).
pub fn scaffold(path: &Path) -> Result<()> {
    // The visible tree starts EMPTY: folders like notes/ are created on
    // demand when something is saved into them (write_file creates parents).
    // Only hidden internals are prepared here.
    fs::create_dir_all(path)?;
    fs::create_dir_all(path.join(".pugdock/cache"))?;
    if !path.join(".gitignore").exists() {
        fs::write(path.join(".gitignore"), GITIGNORE)?;
    }
    Ok(())
}

// ---- Commands ----

#[tauri::command]
pub fn get_app_config(app: tauri::AppHandle) -> Result<AppConfig> {
    load_config(&app)
}

#[tauri::command]
pub fn set_app_config(app: tauri::AppHandle, config: AppConfig) -> Result<()> {
    save_config(&app, &config)
}

#[derive(Serialize)]
pub struct FolderInspection {
    pub exists: bool,
    pub is_empty: bool,
    pub is_git_repo: bool,
}

#[tauri::command]
pub fn inspect_folder(path: String) -> Result<FolderInspection> {
    let p = PathBuf::from(&path);
    let exists = p.is_dir();
    Ok(FolderInspection {
        exists,
        is_empty: !exists || fs::read_dir(&p)?.next().is_none(),
        is_git_repo: p.join(".git").exists(),
    })
}

#[tauri::command]
pub fn create_workspace(app: tauri::AppHandle, path: String) -> Result<()> {
    let p = PathBuf::from(&path);
    scaffold(&p)?;
    let mut cfg = load_config(&app)?;
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Workspace".into());
    if !cfg.workspaces.iter().any(|w| w.path == path) {
        cfg.workspaces.push(WorkspaceEntry {
            path: path.clone(),
            name,
            repo_owner: None,
            repo_name: None,
            managed: true,
        });
    }
    cfg.workspace_path = Some(path);
    cfg.repo_owner = None;
    cfg.repo_name = None;
    save_config(&app, &cfg)
}

/// Add a workspace tab: `managed = true` scaffolds a new PugDock workspace;
/// `managed = false` opens an existing folder untouched (code-editor mode).
#[tauri::command]
pub fn add_workspace(app: tauri::AppHandle, path: String, managed: bool) -> Result<AppConfig> {
    let p = PathBuf::from(&path);
    if managed {
        scaffold(&p)?;
    } else if !p.is_dir() {
        return Err(AppError::Other("Folder not found.".into()));
    }
    let mut cfg = load_config(&app)?;
    if !cfg.workspaces.iter().any(|w| w.path == path) {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Folder".into());
        cfg.workspaces.push(WorkspaceEntry { path: path.clone(), name, repo_owner: None, repo_name: None, managed });
    }
    set_active(&mut cfg, &path);
    save_config(&app, &cfg)?;
    load_config(&app)
}

fn set_active(cfg: &mut AppConfig, path: &str) {
    if let Some(entry) = cfg.workspaces.iter().find(|w| w.path == path) {
        cfg.workspace_path = Some(entry.path.clone());
        cfg.repo_owner = entry.repo_owner.clone();
        cfg.repo_name = entry.repo_name.clone();
    }
}

#[tauri::command]
pub fn set_active_workspace(app: tauri::AppHandle, path: String) -> Result<AppConfig> {
    let mut cfg = load_config(&app)?;
    // A managed workspace whose folder vanished (moved/deleted outside the
    // app) is recreated empty instead of leaving the UI stuck on stale data.
    if let Some(entry) = cfg.workspaces.iter().find(|w| w.path == path) {
        if entry.managed && !Path::new(&path).is_dir() {
            scaffold(Path::new(&path))?;
        }
    }
    set_active(&mut cfg, &path);
    save_config(&app, &cfg)?;
    load_config(&app)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let target = dst.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

/// Move the active workspace (files, .git history, everything) to a new
/// folder and repoint the config. Same-volume moves are a rename; across
/// volumes it falls back to copy + delete.
#[tauri::command]
pub fn move_workspace(app: tauri::AppHandle, new_path: String) -> Result<AppConfig> {
    let mut cfg = load_config(&app)?;
    let old = cfg.workspace_path.clone().ok_or(AppError::NoWorkspace)?;
    if old == new_path {
        return load_config(&app);
    }
    let old_p = PathBuf::from(&old);
    let new_p = PathBuf::from(&new_path);
    if new_p.starts_with(&old_p) {
        return Err(AppError::Other("The destination is inside the current workspace.".into()));
    }
    if new_p.exists() {
        if fs::read_dir(&new_p)?.next().is_some() {
            return Err(AppError::Other("The destination folder already exists and is not empty.".into()));
        }
        fs::remove_dir(&new_p)?; // empty dir would make rename fail on some platforms
    }
    if let Some(parent) = new_p.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(&old_p, &new_p).is_err() {
        copy_dir(&old_p, &new_p)?;
        fs::remove_dir_all(&old_p)?;
    }
    if let Some(entry) = cfg.workspaces.iter_mut().find(|w| w.path == old) {
        entry.path = new_path.clone();
    }
    cfg.workspace_path = Some(new_path);
    save_config(&app, &cfg)?;
    load_config(&app)
}

/// Remove a workspace tab from the list - never deletes any files.
#[tauri::command]
pub fn remove_workspace(app: tauri::AppHandle, path: String) -> Result<AppConfig> {
    let mut cfg = load_config(&app)?;
    cfg.workspaces.retain(|w| w.path != path);
    if cfg.workspace_path.as_deref() == Some(path.as_str()) {
        let next = cfg.workspaces.first().map(|w| w.path.clone());
        cfg.workspace_path = None;
        cfg.repo_owner = None;
        cfg.repo_name = None;
        if let Some(n) = next {
            set_active(&mut cfg, &n);
        }
    }
    save_config(&app, &cfg)?;
    load_config(&app)
}

/// Tree of any configured workspace (the All view shows every workspace).
#[tauri::command]
pub fn list_tree_at(app: tauri::AppHandle, root: String) -> Result<Vec<TreeEntry>> {
    let rootp = PathBuf::from(&root);
    if !rootp.is_dir() {
        return Ok(vec![]);
    }
    let skip = nested_workspace_paths(&app, &rootp);
    read_tree(&rootp, &rootp, &skip)
}

#[tauri::command]
pub fn list_tree(app: tauri::AppHandle) -> Result<Vec<TreeEntry>> {
    let root = workspace_root(&app)?;
    let skip = nested_workspace_paths(&app, &root);
    read_tree(&root, &root, &skip)
}

#[tauri::command]
pub fn read_file(app: tauri::AppHandle, path: String) -> Result<String> {
    let root = workspace_root(&app)?;
    Ok(fs::read_to_string(resolve(&root, &path)?)?)
}

#[tauri::command]
pub fn read_file_base64(app: tauri::AppHandle, path: String) -> Result<String> {
    let root = workspace_root(&app)?;
    Ok(base64_encode(&fs::read(resolve(&root, &path)?)?))
}

#[tauri::command]
pub fn write_file(app: tauri::AppHandle, path: String, content: String) -> Result<()> {
    let root = workspace_root(&app)?;
    let p = resolve(&root, &path)?;
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(p, content)?;
    Ok(())
}

#[tauri::command]
pub fn create_folder(app: tauri::AppHandle, path: String) -> Result<()> {
    let root = workspace_root(&app)?;
    fs::create_dir_all(resolve(&root, &path)?)?;
    Ok(())
}

#[tauri::command]
pub fn rename_path(app: tauri::AppHandle, from: String, to: String) -> Result<()> {
    let root = workspace_root(&app)?;
    let dst = resolve(&root, &to)?;
    if dst.exists() {
        return Err(AppError::Other(format!("\"{to}\" already exists.")));
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(resolve(&root, &from)?, dst)?;
    Ok(())
}

#[tauri::command]
pub fn delete_path(app: tauri::AppHandle, path: String) -> Result<()> {
    let root = workspace_root(&app)?;
    let p = resolve(&root, &path)?;
    if p == root {
        return Err(AppError::PathOutsideWorkspace);
    }
    if p.is_dir() {
        fs::remove_dir_all(p)?;
    } else {
        fs::remove_file(p)?;
    }
    Ok(())
}

#[tauri::command]
pub fn duplicate_file(app: tauri::AppHandle, path: String) -> Result<String> {
    let root = workspace_root(&app)?;
    let src = resolve(&root, &path)?;
    let stem = src.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dir = src.parent().unwrap_or(&root).to_path_buf();
    for i in 1..100 {
        let candidate = dir.join(format!("{stem} copy{}{ext}", if i == 1 { String::new() } else { format!(" {i}") }));
        if !candidate.exists() {
            fs::copy(&src, &candidate)?;
            return Ok(candidate.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
    Err(AppError::Other("Too many copies.".into()))
}

/// Import an absolute file from anywhere on disk into the workspace.
#[tauri::command]
pub fn import_file(app: tauri::AppHandle, source: String, dest: String) -> Result<()> {
    let root = workspace_root(&app)?;
    let dst = resolve(&root, &dest)?;
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(PathBuf::from(source), dst)?;
    Ok(())
}

/// Move a file or folder from the active workspace into another workspace,
/// keeping its relative path. Same-volume moves are a rename.
#[tauri::command]
pub fn move_to_workspace(app: tauri::AppHandle, path: String, target_root: String) -> Result<()> {
    let root = workspace_root(&app)?;
    let src = resolve(&root, &path)?;
    if !src.exists() {
        return Err(AppError::Other("File not found.".into()));
    }
    let target = PathBuf::from(&target_root);
    if !target.is_dir() {
        return Err(AppError::Other("Target workspace folder not found.".into()));
    }
    let dst = resolve(&target, &path)?;
    if dst.exists() {
        return Err(AppError::Other(format!("\"{path}\" already exists in that workspace.")));
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(&src, &dst).is_err() {
        if src.is_dir() {
            copy_dir(&src, &dst)?;
            fs::remove_dir_all(&src)?;
        } else {
            fs::copy(&src, &dst)?;
            fs::remove_file(&src)?;
        }
    }
    Ok(())
}

/// Plain file listing of a workspace subfolder (used for hidden app data
/// like .chats/, which the tree intentionally does not show).
#[tauri::command]
pub fn list_files(app: tauri::AppHandle, dir: String) -> Result<Vec<String>> {
    let root = workspace_root(&app)?;
    let p = resolve(&root, &dir)?;
    if !p.is_dir() {
        return Ok(vec![]);
    }
    Ok(fs::read_dir(p)?
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect())
}

/// Write a diagnostics report (no secrets) into the app config dir and
/// return its path so the UI can reveal it.
#[tauri::command]
pub fn export_diagnostics(app: tauri::AppHandle) -> Result<String> {
    use tauri::Manager;
    let cfg = load_config(&app)?;
    let mut report = String::new();
    report.push_str(&format!("PugDock diagnostics\ngenerated: {}\n\n", chrono::Local::now()));
    report.push_str(&format!("app version: {}\n", app.package_info().version));
    report.push_str(&format!("os: {} {}\n\n", std::env::consts::OS, std::env::consts::ARCH));
    report.push_str(&format!("active workspace: {:?}\n", cfg.workspace_path));
    for w in &cfg.workspaces {
        report.push_str(&format!(
            "  - {} ({}) repo: {:?}/{:?}\n",
            w.name,
            if w.managed { "managed" } else { "opened folder" },
            w.repo_owner,
            w.repo_name
        ));
    }
    report.push_str(&format!("\nsettings: {}\n", serde_json::to_string_pretty(&cfg.settings).unwrap_or_default()));
    if let Ok(root) = workspace_root(&app) {
        let git = std::process::Command::new("git")
            .args(["status", "--short", "--branch"])
            .current_dir(&root)
            .output();
        match git {
            Ok(o) => report.push_str(&format!("\ngit status:\n{}", String::from_utf8_lossy(&o.stdout))),
            Err(e) => report.push_str(&format!("\ngit unavailable: {e}\n")),
        }
    }
    let dir = app.path().app_config_dir().map_err(|e| AppError::Other(e.to_string()))?;
    let dest = dir.join("pugdock-diagnostics.txt");
    fs::write(&dest, report)?;
    Ok(dest.to_string_lossy().to_string())
}

// std has no base64; small local impl beats a dependency for one encoder.
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(CHARS[(n >> 18 & 63) as usize] as char);
        out.push(CHARS[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 { CHARS[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[(n & 63) as usize] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn resolve_rejects_traversal() {
        let root = Path::new("/tmp/ws");
        assert!(resolve(root, "../etc/passwd").is_err());
        assert!(resolve(root, "notes/../../x").is_err());
        assert_eq!(resolve(root, "notes/a.md").unwrap(), root.join("notes/a.md"));
    }
}
