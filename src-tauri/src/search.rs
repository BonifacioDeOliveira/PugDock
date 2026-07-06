use crate::error::Result;
use crate::workspace;
use rusqlite::Connection;
use serde::Serialize;
use std::fs;
use std::path::Path;

const TEXT_EXTS: &[&str] = &[
    "md", "txt", "json", "yaml", "yml", "toml", "py", "js", "ts", "tsx", "jsx", "go",
    "rs", "java", "cs", "sql", "html", "css", "log", "csv", "svg", "sh", "fish", "example",
];
const MAX_INDEX_BYTES: u64 = 2_000_000;

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Managed workspaces keep their index in `.pugdock/`; opened folders get an
/// index in the app config dir so PugDock never writes into someone's repo.
fn index_path(app: &tauri::AppHandle, root: &Path) -> Result<std::path::PathBuf> {
    use tauri::Manager;
    let local = root.join(".pugdock");
    if local.is_dir() {
        return Ok(local.join("index.sqlite"));
    }
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| crate::error::AppError::Other(e.to_string()))?
        .join("indexes");
    fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{:016x}.sqlite", fnv(&root.to_string_lossy()))))
}

fn open(app: &tauri::AppHandle, root: &Path) -> Result<Connection> {
    let conn = Connection::open(index_path(app, root)?)?;
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(path, content);
         CREATE TABLE IF NOT EXISTS files(path TEXT PRIMARY KEY, mtime INTEGER);",
    )?;
    Ok(conn)
}

fn indexable(path: &Path, size: u64) -> bool {
    size <= MAX_INDEX_BYTES
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| TEXT_EXTS.contains(&e.to_lowercase().as_str()))
}

fn upsert(conn: &Connection, rel: &str, content: &str, mtime: i64) -> Result<()> {
    conn.execute("DELETE FROM files_fts WHERE path = ?1", [rel])?;
    conn.execute("INSERT INTO files_fts(path, content) VALUES (?1, ?2)", [rel, content])?;
    conn.execute(
        "INSERT INTO files(path, mtime) VALUES (?1, ?2) ON CONFLICT(path) DO UPDATE SET mtime = ?2",
        rusqlite::params![rel, mtime],
    )?;
    Ok(())
}

fn walk(conn: &Connection, dir: &Path, root: &Path, count: &mut u32) -> Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || crate::workspace::SKIP_DIRS.contains(&name.as_str()) {
            continue; // .git, .pugdock, dotfiles, build dirs - never indexed
        }
        let path = e.path();
        if path.is_dir() {
            walk(conn, &path, root, count)?;
        } else if let Ok(meta) = e.metadata() {
            if indexable(&path, meta.len()) {
                if let Ok(content) = fs::read_to_string(&path) {
                    let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                    let mtime = meta.modified().ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    upsert(conn, &rel, &content, mtime)?;
                    *count += 1;
                }
            }
        }
    }
    Ok(())
}

// ---- Commands ----

/// Full reindex of the workspace. Returns number of files indexed.
#[tauri::command]
pub async fn rebuild_index(app: tauri::AppHandle) -> Result<u32> {
    let root = workspace::workspace_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let conn = open(&app, &root)?;
        conn.execute("DELETE FROM files_fts", [])?;
        conn.execute("DELETE FROM files", [])?;
        let mut count = 0;
        walk(&conn, &root, &root, &mut count)?;
        Ok(count)
    })
    .await
    .map_err(|e| crate::error::AppError::Other(e.to_string()))?
}

/// Index or refresh one file. `content` overrides disk content (used for
/// text extracted from PDFs by the frontend).
#[tauri::command]
pub fn index_file(app: tauri::AppHandle, path: String, content: Option<String>) -> Result<()> {
    let root = workspace::workspace_root(&app)?;
    let abs = workspace::resolve(&root, &path)?;
    let conn = open(&app, &root)?;
    let text = match content {
        Some(c) => c,
        None => {
            let meta = fs::metadata(&abs)?;
            if !indexable(&abs, meta.len()) {
                return Ok(());
            }
            fs::read_to_string(&abs).unwrap_or_default()
        }
    };
    upsert(&conn, &path, &text, chrono::Utc::now().timestamp())?;
    Ok(())
}

#[tauri::command]
pub fn remove_from_index(app: tauri::AppHandle, path: String) -> Result<()> {
    let root = workspace::workspace_root(&app)?;
    let conn = open(&app, &root)?;
    conn.execute("DELETE FROM files_fts WHERE path = ?1 OR path LIKE ?1 || '/%'", [&path])?;
    conn.execute("DELETE FROM files WHERE path = ?1 OR path LIKE ?1 || '/%'", [&path])?;
    Ok(())
}

#[derive(Serialize)]
pub struct SearchHit {
    pub path: String,
    pub snippet: String,
}

#[tauri::command]
pub fn search_workspace(app: tauri::AppHandle, query: String) -> Result<Vec<SearchHit>> {
    let root = workspace::workspace_root(&app)?;
    let conn = open(&app, &root)?;
    // Quote each term so user input can't break FTS5 query syntax.
    let fts_query = query
        .split_whitespace()
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ");
    let mut hits: Vec<SearchHit> = Vec::new();
    // Filename matches first…
    let mut stmt = conn.prepare("SELECT path FROM files WHERE path LIKE '%' || ?1 || '%' ORDER BY path LIMIT 20")?;
    let names = stmt.query_map([&query], |r| r.get::<_, String>(0))?;
    for p in names.flatten() {
        hits.push(SearchHit { path: p, snippet: String::new() });
    }
    // …then content matches.
    if !fts_query.is_empty() {
        let mut stmt = conn.prepare(
            "SELECT path, snippet(files_fts, 1, '\u{1}', '\u{2}', '…', 12)
             FROM files_fts WHERE files_fts MATCH ?1 ORDER BY rank LIMIT 40",
        )?;
        let rows = stmt.query_map([&fts_query], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for (path, snippet) in rows.flatten() {
            if !hits.iter().any(|h| h.path == path) {
                hits.push(SearchHit { path, snippet });
            }
        }
    }
    Ok(hits)
}

/// Top-N matching chunks of workspace content for AI context building.
#[tauri::command]
pub fn search_context(app: tauri::AppHandle, query: String, limit: u32) -> Result<Vec<(String, String)>> {
    let root = workspace::workspace_root(&app)?;
    let conn = open(&app, &root)?;
    let fts_query = query
        .split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ");
    if fts_query.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt = conn.prepare(
        "SELECT path, substr(content, 1, 4000) FROM files_fts WHERE files_fts MATCH ?1 ORDER BY rank LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![fts_query, limit], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    Ok(rows.flatten().collect())
}

/// All indexed content for a set of top-level folders (for Build Context).
#[tauri::command]
pub fn folder_contents(app: tauri::AppHandle, prefixes: Vec<String>, limit: u32) -> Result<Vec<(String, String)>> {
    let root = workspace::workspace_root(&app)?;
    let conn = open(&app, &root)?;
    let mut out = Vec::new();
    for prefix in prefixes {
        let mut stmt = conn.prepare(
            "SELECT path, substr(content, 1, 3000) FROM files_fts WHERE path LIKE ?1 || '%' LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![prefix, limit], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        out.extend(rows.flatten());
    }
    Ok(out)
}
