use crate::error::{AppError, Result};
use crate::secrets;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Public OAuth-app client id (safe to embed). The OAuth app must have
/// "Device Flow" enabled. See README. Resolution order:
/// 1. runtime env var (handy in dev — no rebuild needed)
/// 2. compile-time env var (how release builds embed it)
fn client_id() -> String {
    std::env::var("PUGDOCK_GITHUB_CLIENT_ID")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| option_env!("PUGDOCK_GITHUB_CLIENT_ID").unwrap_or("").to_string())
}

/// OAuth-app client secret, needed only for the browser (authorization-code)
/// flow — GitHub has no PKCE, so desktop apps embed it (as GitHub Desktop does).
/// Without it PugDock falls back to the device-code flow.
fn client_secret() -> String {
    std::env::var("PUGDOCK_GITHUB_CLIENT_SECRET")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| option_env!("PUGDOCK_GITHUB_CLIENT_SECRET").unwrap_or("").to_string())
}

/// Which login flow this build supports: "browser" | "device" | "unconfigured".
#[tauri::command]
pub fn github_auth_mode() -> String {
    if client_id().is_empty() {
        "unconfigured".into()
    } else if client_secret().is_empty() {
        "device".into()
    } else {
        "browser".into()
    }
}

const API: &str = "https://api.github.com";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("PugDock")
        .build()
        .expect("reqwest client")
}

async fn api<T: for<'de> Deserialize<'de>>(method: reqwest::Method, path: &str, body: Option<serde_json::Value>) -> Result<T> {
    let token = secrets::get(secrets::GITHUB_TOKEN)?.ok_or(AppError::GithubAuthExpired)?;
    let mut req = client()
        .request(method, format!("{API}{path}"))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json");
    if let Some(b) = body {
        req = req.json(&b);
    }
    let resp = req.send().await?;
    let status = resp.status();
    if status == 401 {
        return Err(AppError::GithubAuthExpired);
    }
    if status == 403 && resp.headers().get("x-ratelimit-remaining").is_some_and(|v| v == "0") {
        return Err(AppError::Github("GitHub rate limit reached. Try again in a few minutes.".into()));
    }
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v["message"].as_str().map(String::from))
            .unwrap_or(text);
        return Err(AppError::Github(format!("{status}: {msg}")));
    }
    Ok(resp.json().await?)
}

// ---- Browser (authorization-code) flow ----

const CALLBACK_HTML: &str = "<!doctype html><meta charset=utf-8>\
<body style='font-family:system-ui;background:#16181d;color:#d8dce4;display:grid;place-items:center;height:100vh;margin:0'>\
<div style='text-align:center'><div style='font-size:40px'>🐾</div>\
<h2>PugDock is connected</h2><p>You can close this tab and return to the app.</p></div>";

/// Pseudo-random hex for the OAuth `state` param. RandomState is seeded from
/// OS entropy per process — enough for CSRF on a single-shot loopback listener.
fn random_state() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    format!("{:016x}{:08x}", h.finish(), std::process::id())
}

fn query_param(target: &str, key: &str) -> Option<String> {
    let query = target.split_once('?')?.1;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

/// Full browser OAuth round-trip: open GitHub authorize page, catch the
/// redirect on a loopback listener, exchange the code, store the token.
/// Resolves when the token is safely in the keychain.
#[tauri::command]
pub async fn github_oauth_start(app: tauri::AppHandle) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let client_id = client_id();
    if client_id.is_empty() {
        return Err(AppError::Github(
            "No GitHub app configured. Set PUGDOCK_GITHUB_CLIENT_ID (see README).".into(),
        ));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let state = random_state();
    let auth_url = format!(
        "https://github.com/login/oauth/authorize?client_id={client_id}&redirect_uri={redirect_uri}&scope=repo%20read:org&state={state}"
    );
    app.opener()
        .open_url(&auth_url, None::<&str>)
        .map_err(|e| AppError::Other(e.to_string()))?;

    // Wait (max 5 min) for the browser to hit /callback with a valid state.
    let code = tokio::time::timeout(std::time::Duration::from_secs(300), async {
        loop {
            let (mut stream, _) = listener.accept().await?;
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let target = req.split_whitespace().nth(1).unwrap_or("");
            if !target.starts_with("/callback") {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n").await;
                continue;
            }
            let _ = stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\n\r\n{}",
                        CALLBACK_HTML.len(),
                        CALLBACK_HTML
                    )
                    .as_bytes(),
                )
                .await;
            if query_param(target, "state").as_deref() != Some(state.as_str()) {
                continue; // ignore stray/forged requests, keep waiting
            }
            match query_param(target, "code") {
                Some(code) => return Ok::<String, AppError>(code),
                None => {
                    return Err(AppError::Github(
                        "GitHub authorization was denied or cancelled.".into(),
                    ))
                }
            }
        }
    })
    .await
    .map_err(|_| AppError::Github("GitHub login timed out. Try again.".into()))??;

    let resp: serde_json::Value = client()
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&json!({
            "client_id": client_id,
            "client_secret": client_secret(),
            "code": code,
            "redirect_uri": redirect_uri,
        }))
        .send()
        .await?
        .json()
        .await?;
    match resp["access_token"].as_str() {
        Some(token) => {
            secrets::set(secrets::GITHUB_TOKEN, token)?;
            Ok(())
        }
        None => Err(AppError::Github(
            resp["error_description"]
                .as_str()
                .unwrap_or("GitHub did not return an access token.")
                .into(),
        )),
    }
}

// ---- Device flow ----

#[derive(Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[tauri::command]
pub async fn github_device_start() -> Result<DeviceCode> {
    let client_id = client_id();
    if client_id.is_empty() {
        return Err(AppError::Github(
            "No GitHub app configured. Set the PUGDOCK_GITHUB_CLIENT_ID environment variable and restart (see README).".into(),
        ));
    }
    let resp = client()
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .json(&json!({ "client_id": client_id, "scope": "repo read:org" }))
        .send()
        .await?;
    Ok(resp.json().await?)
}

/// One poll step of the device flow: "pending" | "slow_down" | "ok" | "expired" | "denied".
#[tauri::command]
pub async fn github_device_poll(device_code: String) -> Result<String> {
    let resp: serde_json::Value = client()
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&json!({
            "client_id": client_id(),
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        }))
        .send()
        .await?
        .json()
        .await?;
    if let Some(token) = resp["access_token"].as_str() {
        secrets::set(secrets::GITHUB_TOKEN, token)?;
        return Ok("ok".into());
    }
    Ok(match resp["error"].as_str() {
        Some("authorization_pending") => "pending",
        Some("slow_down") => "slow_down",
        Some("expired_token") => "expired",
        Some("access_denied") => "denied",
        other => return Err(AppError::Github(other.unwrap_or("unknown").into())),
    }
    .into())
}

// ---- REST API ----

#[derive(Serialize, Deserialize)]
pub struct GithubUser {
    pub login: String,
    pub id: u64,
    pub name: Option<String>,
    pub avatar_url: String,
}

#[tauri::command]
pub async fn github_user() -> Result<GithubUser> {
    api(reqwest::Method::GET, "/user", None).await
}

#[derive(Serialize, Deserialize)]
pub struct Org {
    pub login: String,
}

#[tauri::command]
pub async fn github_orgs() -> Result<Vec<Org>> {
    api(reqwest::Method::GET, "/user/orgs", None).await
}

#[tauri::command]
pub async fn github_repo_exists(owner: String, name: String) -> Result<bool> {
    match api::<serde_json::Value>(reqwest::Method::GET, &format!("/repos/{owner}/{name}"), None).await {
        Ok(_) => Ok(true),
        Err(AppError::Github(msg)) if msg.starts_with("404") => Ok(false),
        Err(e) => Err(e),
    }
}

#[derive(Serialize)]
pub struct CreatedRepo {
    pub full_name: String,
    pub clone_url: String,
    pub html_url: String,
    pub private: bool,
}

/// Create a private repo under the user or an org. Refuses to proceed if the
/// created repo is somehow not private.
#[tauri::command]
pub async fn github_create_repo(owner: String, name: String, is_org: bool) -> Result<CreatedRepo> {
    let path = if is_org { format!("/orgs/{owner}/repos") } else { "/user/repos".into() };
    let body = json!({ "name": name, "private": true, "description": "PugDock workspace" });
    let repo: serde_json::Value = api(reqwest::Method::POST, &path, Some(body)).await.map_err(|e| match e {
        AppError::Github(msg) if msg.contains("already exists") => {
            AppError::Github(format!("A repository named \"{name}\" already exists for {owner}."))
        }
        AppError::Github(msg) if msg.starts_with("403") || msg.starts_with("404") => AppError::Github(
            "PugDock could not create the private GitHub repo. Check your GitHub permissions and try again.".into(),
        ),
        e => e,
    })?;
    if repo["private"] != true {
        return Err(AppError::Github("GitHub did not create a private repository. Aborting.".into()));
    }
    Ok(CreatedRepo {
        full_name: repo["full_name"].as_str().unwrap_or_default().into(),
        clone_url: repo["clone_url"].as_str().unwrap_or_default().into(),
        html_url: repo["html_url"].as_str().unwrap_or_default().into(),
        private: true,
    })
}
