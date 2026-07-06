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

/// Locate the official Anthropic CLI. GUI apps get a minimal PATH on macOS,
/// so check the common install locations explicitly.
fn ant_path() -> Option<std::path::PathBuf> {
    let candidates = [
        "/opt/homebrew/bin/ant",
        "/usr/local/bin/ant",
        "/usr/bin/ant",
    ];
    for c in candidates {
        let p = std::path::PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    // fall back to PATH
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("ant"))
            .find(|p| p.exists())
    })
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
/// "key" | "oauth" (logged in via Anthropic account) | "ant" (CLI installed,
/// not logged in) | "none" (no CLI, no key).
#[tauri::command]
pub async fn anthropic_auth_status() -> Result<String> {
    if secrets::get(secrets::ANTHROPIC_KEY)?.is_some() {
        return Ok("key".into());
    }
    if oauth_token().await.is_some() {
        return Ok("oauth".into());
    }
    Ok(if ant_path().is_some() { "ant" } else { "none" }.into())
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
    list_models(&auth().await?).await
}

/// Run one AI task. Model selection (Auto/Fast/Balanced/Deep/Custom) is
/// resolved by the frontend, which owns the settings; this stays a thin,
/// key-holding proxy so the API key never reaches the webview.
#[tauri::command]
pub async fn anthropic_run(model: String, system: String, prompt: String, max_tokens: Option<u32>) -> Result<String> {
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
