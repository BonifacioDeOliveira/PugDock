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
