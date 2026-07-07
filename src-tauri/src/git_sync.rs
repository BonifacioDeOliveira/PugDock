use crate::error::{AppError, Result};
use crate::{secrets, workspace};
use serde::Serialize;
use std::path::Path;
use tokio::process::Command;

/// Credential helper that reads the token from the environment, so it never
/// touches disk or the process argument list.
const CRED_HELPER: &str = "!f() { echo username=x-access-token; echo password=$PUGDOCK_GH_TOKEN; }; f";

async fn run_git(root: &Path, args: &[&str]) -> Result<String> {
    let token = secrets::get(secrets::GITHUB_TOKEN)?.unwrap_or_default();
    let out = Command::new("git")
        .arg("-c").arg(format!("credential.helper={CRED_HELPER}"))
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("PUGDOCK_GH_TOKEN", token)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound { AppError::GitMissing } else { e.into() }
        })?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if out.status.success() {
        Ok(stdout)
    } else if stderr.contains("Could not resolve host") || stderr.contains("unable to access") {
        Err(AppError::Offline)
    } else if stderr.contains("Authentication failed") || stderr.contains("403") {
        Err(AppError::GithubAuthExpired)
    } else {
        Err(AppError::Other(format!("git {}: {}", args.first().unwrap_or(&""), stderr.trim())))
    }
}

#[derive(Serialize)]
pub struct SyncStatus {
    pub dirty: bool,
    pub ahead: u32,
    pub behind: u32,
    pub conflicts: Vec<String>,
    pub merging: bool,
}

async fn status(root: &Path) -> Result<SyncStatus> {
    let out = run_git(root, &["status", "--porcelain=v2", "--branch"]).await?;
    let mut s = SyncStatus { dirty: false, ahead: 0, behind: 0, conflicts: vec![], merging: root.join(".git/MERGE_HEAD").exists() };
    for line in out.lines() {
        if let Some(ab) = line.strip_prefix("# branch.ab ") {
            for part in ab.split(' ') {
                if let Some(n) = part.strip_prefix('+') { s.ahead = n.parse().unwrap_or(0); }
                if let Some(n) = part.strip_prefix('-') { s.behind = n.parse().unwrap_or(0); }
            }
        } else if line.starts_with('u') {
            // unmerged entry: last field is the path
            if let Some(p) = line.split(' ').nth(10) { s.conflicts.push(p.to_string()); }
        } else if line.starts_with('1') || line.starts_with('2') || line.starts_with('?') {
            s.dirty = true;
        }
    }
    Ok(s)
}

// ---- Commands ----

/// Initialize the workspace repo. With a remote URL, wires origin and pushes;
/// without one (local-only mode) it still creates local checkpoint history.
#[tauri::command]
pub async fn git_init_workspace(
    app: tauri::AppHandle,
    remote_url: Option<String>,
    user_name: String,
    user_email: String,
) -> Result<()> {
    let mut root = workspace::workspace_root(&app)?;
    if !root.join(".git").exists() {
        // If the folder lives inside an existing repo (e.g. a sub-workspace),
        // NEVER init a nested repo: link the enclosing repo instead so the
        // whole tree syncs as one.
        match run_git(&root, &["rev-parse", "--show-toplevel"]).await {
            Ok(top) => {
                let top = std::path::PathBuf::from(top.trim());
                if top != root && root.starts_with(&top) {
                    root = top;
                } else {
                    run_git(&root, &["init", "-b", "main"]).await?;
                }
            }
            Err(_) => {
                run_git(&root, &["init", "-b", "main"]).await?;
            }
        }
    }
    run_git(&root, &["config", "user.name", &user_name]).await?;
    run_git(&root, &["config", "user.email", &user_email]).await?;
    if let Some(url) = &remote_url {
        // set-url if origin exists, add otherwise
        if run_git(&root, &["remote", "set-url", "origin", url]).await.is_err() {
            run_git(&root, &["remote", "add", "origin", url]).await?;
        }
    }
    run_git(&root, &["add", "-A"]).await?;
    let st = status(&root).await?;
    if st.dirty {
        run_git(&root, &["commit", "-m", "pugdock: initialize workspace"]).await?;
    }
    if remote_url.is_some() {
        // The remote may already hold notes from another device (the shared
        // PugDockNotes repo). Merge them in, preferring the remote versions
        // for any overlapping scaffold files, then push.
        if run_git(&root, &["fetch", "origin", "main"]).await.is_ok() {
            let _ = run_git(
                &root,
                &["merge", "--no-edit", "--allow-unrelated-histories", "-X", "theirs", "origin/main"],
            )
            .await;
        }
        run_git(&root, &["push", "-u", "origin", "main"]).await?;
    }
    Ok(())
}

/// Commit all pending changes as a checkpoint. Returns true if a commit was made.
#[tauri::command]
pub async fn git_checkpoint(app: tauri::AppHandle, message: Option<String>) -> Result<bool> {
    let root = workspace::workspace_root(&app)?;
    let st = status(&root).await?;
    if !st.conflicts.is_empty() {
        return Err(AppError::SyncConflict);
    }
    if !st.dirty {
        return Ok(false);
    }
    run_git(&root, &["add", "-A"]).await?;
    let msg = message.unwrap_or_else(|| {
        format!("pugdock: checkpoint {}", chrono::Local::now().format("%Y-%m-%d %H:%M"))
    });
    run_git(&root, &["commit", "-m", &msg]).await?;
    Ok(true)
}

#[tauri::command]
pub async fn git_push(app: tauri::AppHandle) -> Result<()> {
    let root = workspace::workspace_root(&app)?;
    run_git(&root, &["push", "origin", "main"]).await?;
    Ok(())
}

/// Pull from GitHub. Returns the resulting status; if it contains conflicts,
/// the merge is left in progress for the user to resolve file by file.
#[tauri::command]
pub async fn git_pull(app: tauri::AppHandle) -> Result<SyncStatus> {
    let root = workspace::workspace_root(&app)?;
    // Checkpoint local edits first so the merge never eats uncommitted work.
    git_checkpoint(app.clone(), None).await?;
    run_git(&root, &["fetch", "origin", "main"]).await?;
    let st = status(&root).await?;
    if st.behind > 0 {
        let merge = run_git(&root, &["merge", "--no-edit", "origin/main"]).await;
        if merge.is_err() {
            let st = status(&root).await?;
            if !st.conflicts.is_empty() {
                return Ok(st); // needs review - UI resolves per file
            }
            merge?;
        }
    }
    status(&root).await
}

/// Resolve one conflicted file, keeping either the local or the GitHub version.
#[tauri::command]
pub async fn git_resolve_conflict(app: tauri::AppHandle, path: String, keep: String) -> Result<SyncStatus> {
    let root = workspace::workspace_root(&app)?;
    let side = if keep == "local" { "--ours" } else { "--theirs" };
    run_git(&root, &["checkout", side, "--", &path]).await?;
    run_git(&root, &["add", "--", &path]).await?;
    let st = status(&root).await?;
    if st.conflicts.is_empty() && st.merging {
        run_git(&root, &["commit", "--no-edit"]).await?;
    }
    status(&root).await
}

/// Both sides of a conflicted file, for the "Compare changes" view.
#[tauri::command]
pub async fn git_conflict_versions(app: tauri::AppHandle, path: String) -> Result<(String, String)> {
    let root = workspace::workspace_root(&app)?;
    let local = run_git(&root, &["show", &format!(":2:{path}")]).await.unwrap_or_default();
    let remote = run_git(&root, &["show", &format!(":3:{path}")]).await.unwrap_or_default();
    Ok((local, remote))
}

#[tauri::command]
pub async fn git_status(app: tauri::AppHandle) -> Result<SyncStatus> {
    let root = workspace::workspace_root(&app)?;
    status(&root).await
}

#[derive(Serialize)]
pub struct Checkpoint {
    pub hash: String,
    pub message: String,
    pub date: String,
}

#[tauri::command]
pub async fn git_history(app: tauri::AppHandle, path: Option<String>, limit: Option<u32>) -> Result<Vec<Checkpoint>> {
    let root = workspace::workspace_root(&app)?;
    let n = format!("-{}", limit.unwrap_or(50));
    let mut args = vec!["log", &n, "--pretty=format:%h\x1f%s\x1f%ci"];
    if let Some(p) = &path {
        args.push("--");
        args.push(p);
    }
    let out = match run_git(&root, &args).await {
        Ok(o) => o,
        Err(_) => return Ok(vec![]), // no commits yet
    };
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\x1f');
            Some(Checkpoint {
                hash: f.next()?.into(),
                message: f.next()?.into(),
                date: f.next()?.into(),
            })
        })
        .collect())
}

const LOCAL_ONLY_HEADER: &str = "# pugdock:local-only";

fn read_exclusions(root: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(root.join(".gitignore")) else { return vec![] };
    let mut in_block = false;
    let mut out = Vec::new();
    for line in content.lines() {
        if line.trim() == LOCAL_ONLY_HEADER {
            in_block = true;
            continue;
        }
        if in_block {
            if line.trim().is_empty() || line.starts_with('#') {
                in_block = false;
            } else {
                out.push(line.trim().trim_start_matches('/').to_string());
            }
        }
    }
    out
}

/// Paths marked "local only" (kept on disk, never synced).
#[tauri::command]
pub fn sync_exclusions(app: tauri::AppHandle) -> Result<Vec<String>> {
    let root = workspace::workspace_root(&app)?;
    Ok(read_exclusions(&root))
}

/// Toggle a file/folder as local-only: writes a managed block in .gitignore
/// and untracks it from git so it stops syncing (the local copy stays).
#[tauri::command]
pub async fn set_sync_excluded(app: tauri::AppHandle, path: String, excluded: bool) -> Result<Vec<String>> {
    let root = workspace::workspace_root(&app)?;
    let mut list = read_exclusions(&root);
    if excluded && !list.contains(&path) {
        list.push(path.clone());
    } else if !excluded {
        list.retain(|p| p != &path);
    }
    // rewrite the managed block at the end of .gitignore
    let gi_path = root.join(".gitignore");
    let content = std::fs::read_to_string(&gi_path).unwrap_or_default();
    let mut kept: Vec<&str> = Vec::new();
    let mut in_block = false;
    for line in content.lines() {
        if line.trim() == LOCAL_ONLY_HEADER {
            in_block = true;
            continue;
        }
        if in_block {
            if line.trim().is_empty() || line.starts_with('#') {
                in_block = false;
                if line.starts_with('#') {
                    kept.push(line);
                }
            }
            continue;
        }
        kept.push(line);
    }
    let mut new_content = kept.join("\n").trim_end().to_string();
    if !list.is_empty() {
        new_content.push_str(&format!("\n\n{LOCAL_ONLY_HEADER}\n"));
        new_content.push_str(&list.iter().map(|p| format!("/{p}")).collect::<Vec<_>>().join("\n"));
    }
    new_content.push('\n');
    std::fs::write(&gi_path, new_content)?;
    if excluded && root.join(".git").exists() {
        // stop tracking without deleting the local copy
        let _ = run_git(&root, &["rm", "-r", "--cached", "--ignore-unmatch", "--", &path]).await;
    }
    Ok(list)
}

/// File content at a given checkpoint, for version history preview.
#[tauri::command]
pub async fn git_file_at(app: tauri::AppHandle, hash: String, path: String) -> Result<String> {
    let root = workspace::workspace_root(&app)?;
    run_git(&root, &["show", &format!("{hash}:{path}")]).await
}
