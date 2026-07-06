use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

pub const WORKSPACE_DIRS: &[&str] = &[
    "inbox", "notes", "snippets", "commands", "bugs", "adr", "runbooks", "pdfs",
    "references", "projects", "attachments", "context", "templates", "rag",
    ".pugdock", ".pugdock/cache", ".pugdock/embeddings", ".pugdock/thumbnails",
];

const GITIGNORE: &str = "\
# PugDock local state — never synced
.pugdock/
*.sqlite

# Secrets — never synced
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

const WORKSPACE_README: &str = "\
# PugDock Workspace

This repository is managed by [PugDock](https://github.com/pugdock) — a lightweight
desktop workspace for developers. Files here are synced automatically from the app.

- `inbox/` — unsorted captures
- `notes/` — general notes
- `snippets/` — code snippets
- `commands/` — useful commands
- `bugs/` — bug notes
- `adr/` — architecture decision records
- `runbooks/` — operational runbooks
- `pdfs/` — PDF documents
- `references/` — reference material and summaries
- `projects/` — per-project context
- `context/` — AI-generated context files
";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceEntry {
    pub path: String,
    pub name: String,
    pub repo_owner: Option<String>,
    pub repo_name: Option<String>,
    /// true = PugDock-managed (scaffold, checkpoints, sync).
    /// false = an opened folder — PugDock never touches its git or structure.
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
    Ok(cfg)
}

pub fn save_config(app: &tauri::AppHandle, cfg: &AppConfig) -> Result<()> {
    let mut cfg = cfg.clone();
    cfg.normalize();
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

/// Heavy build/dependency dirs — skipped in the tree and the search index so
/// opened code folders stay fast.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "dist", "build", ".next", ".nuxt", "__pycache__",
    ".venv", "venv", ".gradle", "Pods", "DerivedData", "vendor", ".svelte-kit",
];

fn read_tree(dir: &Path, root: &Path) -> Result<Vec<TreeEntry>> {
    let mut entries: Vec<TreeEntry> = Vec::new();
    for e in fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().to_string();
        if name == ".git" || name == ".pugdock" || name == ".DS_Store" || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let path = e.path();
        let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        let is_dir = path.is_dir();
        entries.push(TreeEntry {
            children: if is_dir { Some(read_tree(&path, root)?) } else { None },
            name,
            path: rel,
            is_dir,
        });
    }
    entries.sort_by(|a, b| (!a.is_dir, a.name.to_lowercase()).cmp(&(!b.is_dir, b.name.to_lowercase())));
    Ok(entries)
}

/// Create the standard PugDock folder structure inside `path` (idempotent).
pub fn scaffold(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    for d in WORKSPACE_DIRS {
        fs::create_dir_all(path.join(d))?;
    }
    let write_if_missing = |p: PathBuf, content: &str| -> Result<()> {
        if !p.exists() {
            fs::write(p, content)?;
        }
        Ok(())
    };
    write_if_missing(path.join(".gitignore"), GITIGNORE)?;
    write_if_missing(path.join("README.md"), WORKSPACE_README)?;
    write_if_missing(path.join("rag/manifest.json"), "{\n  \"version\": 1\n}\n")?;
    write_if_missing(path.join("rag/chunks.jsonl"), "")?;
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
    set_active(&mut cfg, &path);
    save_config(&app, &cfg)?;
    load_config(&app)
}

/// Remove a workspace tab from the list — never deletes any files.
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

#[tauri::command]
pub fn list_tree(app: tauri::AppHandle) -> Result<Vec<TreeEntry>> {
    let root = workspace_root(&app)?;
    read_tree(&root, &root)
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
