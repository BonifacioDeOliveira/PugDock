use crate::error::{AppError, Result};
use crate::secrets;
use serde::{Deserialize, Serialize};
use serde_json::json;

const API: &str = "https://api.anthropic.com/v1";
const VERSION: &str = "2023-06-01";

fn client() -> reqwest::Client {
    reqwest::Client::builder().user_agent("PugDock").build().expect("reqwest client")
}

/// How the user is authenticated with Anthropic.
enum Auth {
    /// API key from the OS keychain (x-api-key header).
    Key(String),
    /// Short-lived OAuth token minted by the official `ant` CLI
    /// (Authorization: Bearer + oauth beta header).
    OAuth(String),
}

fn apply_auth(req: reqwest::RequestBuilder, auth: &Auth) -> reqwest::RequestBuilder {
    match auth {
        Auth::Key(k) => req.header("x-api-key", k),
        Auth::OAuth(t) => req
            .bearer_auth(t)
            .header("anthropic-beta", "oauth-2025-04-20"),
    }
}

/// Find a binary by name: common install locations first (GUI apps get a
/// minimal PATH on macOS and Linux), then PATH. Cross-platform.
fn find_bin(name: &str, extra: &[String]) -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut candidates: Vec<String> = extra.to_vec();
    candidates.extend([
        format!("/opt/homebrew/bin/{name}"),
        format!("/usr/local/bin/{name}"),
        format!("{home}/.local/bin/{name}"),
        format!("/home/linuxbrew/.linuxbrew/bin/{name}"),
        format!("/usr/bin/{name}"),
    ]);
    for c in candidates {
        let p = std::path::PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.exists())
    })
}

fn ant_path() -> Option<std::path::PathBuf> {
    find_bin("ant", &[])
}

/// The Claude Code CLI - the primary AI provider, exactly like claude-mem:
/// it authenticates via the user's existing Claude Code sign-in (their
/// Anthropic account), so PugDock needs no key and no extra login.
fn claude_code_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    find_bin("claude", &[format!("{home}/.claude/local/claude")])
}

/// Run one prompt through Claude Code in headless print mode.
async fn run_claude_code(model: Option<&str>, system: &str, prompt: &str) -> Result<String> {
    run_claude_code_in(model, system, prompt, None).await
}

async fn run_claude_code_in(model: Option<&str>, system: &str, prompt: &str, cwd: Option<std::path::PathBuf>) -> Result<String> {
    use tokio::io::AsyncWriteExt;
    let claude = claude_code_path().ok_or(AppError::AiNotConnected)?;
    let mut cmd = tokio::process::Command::new(claude);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.args(["-p", "--output-format", "text", "--setting-sources", ""])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(m) = model {
        cmd.args(["--model", m]);
    }
    let mut child = cmd.spawn().map_err(|e| AppError::Other(e.to_string()))?;
    let full = format!("<instructions>\n{system}\n</instructions>\n\n{prompt}");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(full.as_bytes()).await.map_err(|e| AppError::Other(e.to_string()))?;
        drop(stdin);
    }
    let out = child.wait_with_output().await.map_err(|e| AppError::Other(e.to_string()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        if err.to_lowercase().contains("log in") || err.to_lowercase().contains("login") || err.to_lowercase().contains("auth") {
            return Err(AppError::Other(
                "Claude Code is not signed in. Open Claude Code, run /login, then try again.".into(),
            ));
        }
        return Err(AppError::Other(format!(
            "Claude Code error: {}",
            err.lines().last().unwrap_or("unknown")
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Fresh OAuth access token from the `ant` CLI, if the user has logged in
/// with their Anthropic account. `print-credentials` auto-refreshes.
async fn oauth_token() -> Option<String> {
    let ant = ant_path()?;
    let out = tokio::process::Command::new(ant)
        .args(["auth", "print-credentials", "--access-token"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Resolution order: API key in keychain, then Anthropic-account OAuth.
async fn auth() -> Result<Auth> {
    if let Some(k) = secrets::get(secrets::ANTHROPIC_KEY)? {
        return Ok(Auth::Key(k));
    }
    if let Some(t) = oauth_token().await {
        return Ok(Auth::OAuth(t));
    }
    Err(AppError::AiNotConnected)
}

#[derive(Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub display_name: String,
}

async fn list_models(auth: &Auth) -> Result<Vec<Model>> {
    let req = client()
        .get(format!("{API}/models?limit=100"))
        .header("anthropic-version", VERSION);
    let resp = apply_auth(req, auth).send().await?;
    if resp.status() == 401 {
        return Err(AppError::AnthropicKeyInvalid);
    }
    if !resp.status().is_success() {
        return Err(AppError::Other(format!("Anthropic API error: {}", resp.status())));
    }
    #[derive(Deserialize)]
    struct Page { data: Vec<Model> }
    Ok(resp.json::<Page>().await?.data)
}

/// Validate the key against the API, then store it in the OS keychain.
#[tauri::command]
pub async fn anthropic_connect(api_key: String) -> Result<Vec<Model>> {
    let auth = Auth::Key(api_key.clone());
    let models = list_models(&auth).await?;
    secrets::set(secrets::ANTHROPIC_KEY, &api_key)?;
    Ok(models)
}

/// Current auth situation, for the UI to pick the right connect flow:
/// "claude" (Claude Code installed - preferred, like claude-mem) | "key" |
/// "oauth" (ant CLI profile) | "ant" (CLI installed, not logged in) | "none".
#[tauri::command]
pub async fn anthropic_auth_status() -> Result<String> {
    if claude_code_path().is_some() {
        return Ok("claude".into());
    }
    if secrets::get(secrets::ANTHROPIC_KEY)?.is_some() {
        return Ok("key".into());
    }
    if oauth_token().await.is_some() {
        return Ok("oauth".into());
    }
    Ok(if ant_path().is_some() { "ant" } else { "none" }.into())
}

/// Streamed variant of `anthropic_run` via Claude Code. Emits `ai-delta`
/// events with text chunks as they generate, then `ai-done` with the full
/// text (or `ai-error`). The `id` correlates events to the caller.
#[tauri::command]
pub async fn anthropic_run_stream(
    app: tauri::AppHandle,
    id: String,
    model: String,
    system: String,
    prompt: String,
) -> Result<()> {
    use tauri::Emitter;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let claude = claude_code_path().ok_or(AppError::AiNotConnected)?;
    let mut cmd = tokio::process::Command::new(claude);
    // Agent mode: the chat runs inside the workspace with file tools enabled,
    // so it can create folders/notes, edit content, and organize for real.
    if let Ok(root) = crate::workspace::workspace_root(&app) {
        cmd.current_dir(root);
    }
    cmd.args([
        "-p",
        "--output-format",
        "stream-json",
        "--include-partial-messages",
        "--verbose",
        "--setting-sources",
        "",
        "--allowedTools",
        "Read,Write,Edit,Glob,Grep,LS,Bash(mkdir:*)",
        "--permission-mode",
        "acceptEdits",
    ])
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
    if model != "auto" {
        cmd.args(["--model", &model]);
    }
    let mut child = cmd.spawn().map_err(|e| AppError::Other(e.to_string()))?;
    let full = format!("<instructions>\n{system}\n</instructions>\n\n{prompt}");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(full.as_bytes()).await.map_err(|e| AppError::Other(e.to_string()))?;
        drop(stdin);
    }
    let stdout = child.stdout.take().ok_or_else(|| AppError::Other("no stdout".into()))?;
    let mut lines = BufReader::new(stdout).lines();
    let mut result = String::new();
    let mut streamed = String::new();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        match v["type"].as_str() {
            Some("stream_event") => {
                if let Some(text) = v["event"]["delta"]["text"].as_str() {
                    streamed.push_str(text);
                    let _ = app.emit("ai-delta", serde_json::json!({ "id": id, "text": text }));
                }
            }
            Some("assistant") => {
                if let Some(items) = v["message"]["content"].as_array() {
                    for it in items {
                        if it["type"] == "tool_use" {
                            let name = it["name"].as_str().unwrap_or("tool");
                            let target = it["input"]["file_path"]
                                .as_str()
                                .or(it["input"]["path"].as_str())
                                .or(it["input"]["pattern"].as_str())
                                .unwrap_or("");
                            let label = match name {
                                "Write" => format!("Writing {target}"),
                                "Edit" => format!("Editing {target}"),
                                "Read" => format!("Reading {target}"),
                                "Glob" | "Grep" | "LS" => format!("Searching {target}"),
                                other => format!("{other} {target}"),
                            };
                            let _ = app.emit("ai-activity", serde_json::json!({ "id": id, "text": label.trim() }));
                        }
                    }
                }
            }
            Some("result") => {
                result = v["result"].as_str().unwrap_or(&streamed).to_string();
            }
            _ => {}
        }
    }
    let status = child.wait().await.map_err(|e| AppError::Other(e.to_string()))?;
    if !status.success() {
        let msg = "Claude Code could not complete the request. Check your sign-in and try again.";
        let _ = app.emit("ai-error", serde_json::json!({ "id": id, "message": msg }));
        return Err(AppError::Other(msg.into()));
    }
    if result.is_empty() {
        result = streamed;
    }
    let _ = app.emit("ai-done", serde_json::json!({ "id": id, "text": result }));
    Ok(())
}

/// Prove the Claude Code sign-in actually works by running a tiny prompt
/// through it (cheap model). Used by the connect flow so "Connected" means
/// verified, not just "binary found".
#[tauri::command]
pub async fn anthropic_verify() -> Result<()> {
    let reply = run_claude_code(
        Some("claude-haiku-4-5"),
        "Reply with exactly: OK",
        "ping",
    )
    .await?;
    if reply.contains("OK") {
        Ok(())
    } else {
        Err(AppError::Other("Claude Code responded unexpectedly. Try again.".into()))
    }
}

/// One-time setup for browser sign-in: installs the official Anthropic CLI
/// via Homebrew when it's missing. No-op if already installed.
#[tauri::command]
pub async fn anthropic_install_cli() -> Result<()> {
    // Serialize concurrent calls (UI pre-warm + user click) - brew can't run twice.
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = LOCK.lock().await;
    if ant_path().is_some() {
        return Ok(());
    }
    let brew = find_bin("brew", &[]).ok_or_else(|| AppError::Other(
        "Homebrew is required to set up Anthropic sign-in. Install it from https://brew.sh and try again.".into(),
    ))?;
    let out = tokio::process::Command::new(brew)
        .args(["install", "anthropics/tap/ant"])
        .output()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(AppError::Other(format!(
            "Could not install the Anthropic CLI: {}",
            err.lines().last().unwrap_or("unknown error")
        )));
    }
    if let Some(p) = ant_path() {
        // macOS quarantine on freshly downloaded brew binaries
        #[cfg(target_os = "macos")]
        {
            let _ = tokio::process::Command::new("xattr")
                .args(["-d", "com.apple.quarantine"])
                .arg(&p)
                .output()
                .await;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = p;
        Ok(())
    } else {
        Err(AppError::Other("Install finished but the Anthropic CLI was not found.".into()))
    }
}

/// "Sign in with Anthropic": runs `ant auth login`, which opens the browser
/// for the official Anthropic OAuth flow and stores a refreshable profile.
/// Resolves once login completes and the token is validated.
#[tauri::command]
pub async fn anthropic_oauth_login() -> Result<Vec<Model>> {
    let ant = ant_path().ok_or_else(|| AppError::Other(
        "The Anthropic CLI is not installed. Install it with: brew install anthropics/tap/ant".into(),
    ))?;
    let status = tokio::process::Command::new(ant)
        .args(["auth", "login"])
        .status()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;
    if !status.success() {
        return Err(AppError::Other("Anthropic login was not completed.".into()));
    }
    let token = oauth_token().await.ok_or_else(|| {
        AppError::Other("Login finished but no Anthropic credentials were found.".into())
    })?;
    list_models(&Auth::OAuth(token)).await
}

#[tauri::command]
pub async fn anthropic_models() -> Result<Vec<Model>> {
    // With an API key or OAuth token we can list live; via Claude Code we
    // offer the current well-known models (the CLI has no list command).
    if let Ok(a) = auth().await {
        if let Ok(m) = list_models(&a).await {
            return Ok(m);
        }
    }
    if claude_code_path().is_some() {
        return Ok(vec![
            Model { id: "claude-opus-4-8".into(), display_name: "Claude Opus 4.8".into() },
            Model { id: "claude-sonnet-5".into(), display_name: "Claude Sonnet 5".into() },
            Model { id: "claude-sonnet-4-6".into(), display_name: "Claude Sonnet 4.6".into() },
            Model { id: "claude-haiku-4-5".into(), display_name: "Claude Haiku 4.5".into() },
        ]);
    }
    Err(AppError::AiNotConnected)
}

/// Run one AI task. Model selection (Auto/Fast/Balanced/Deep/Custom) is
/// resolved by the frontend, which owns the settings; this stays a thin,
/// key-holding proxy so the API key never reaches the webview.
#[tauri::command]
pub async fn anthropic_run(app: tauri::AppHandle, model: String, system: String, prompt: String, max_tokens: Option<u32>) -> Result<String> {
    // Claude Code first - same mechanism as claude-mem: the local CLI runs
    // the request under the user's existing Anthropic sign-in.
    if claude_code_path().is_some() {
        let m = (model != "auto").then_some(model.as_str());
        let cwd = crate::workspace::workspace_root(&app).ok();
        return run_claude_code_in(m, &system, &prompt, cwd).await;
    }
    let auth = auth().await?;
    let req = client()
        .post(format!("{API}/messages"))
        .header("anthropic-version", VERSION);
    let resp = apply_auth(req, &auth)
        .json(&json!({
            "model": model,
            "max_tokens": max_tokens.unwrap_or(4096),
            "system": system,
            "messages": [{ "role": "user", "content": prompt }],
        }))
        .send()
        .await?;
    if resp.status() == 401 {
        return Err(AppError::AnthropicKeyInvalid);
    }
    let body: serde_json::Value = resp.json().await?;
    if let Some(err) = body["error"]["message"].as_str() {
        return Err(AppError::Other(format!("Anthropic: {err}")));
    }
    let text = body["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    Ok(text)
}
