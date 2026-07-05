use crate::error::{AppError, Result};
use crate::secrets;
use serde::{Deserialize, Serialize};
use serde_json::json;

const API: &str = "https://api.anthropic.com/v1";
const VERSION: &str = "2023-06-01";

fn client() -> reqwest::Client {
    reqwest::Client::builder().user_agent("PugDock").build().expect("reqwest client")
}

fn key() -> Result<String> {
    secrets::get(secrets::ANTHROPIC_KEY)?.ok_or(AppError::AiNotConnected)
}

#[derive(Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub display_name: String,
}

async fn list_models(api_key: &str) -> Result<Vec<Model>> {
    let resp = client()
        .get(format!("{API}/models?limit=100"))
        .header("x-api-key", api_key)
        .header("anthropic-version", VERSION)
        .send()
        .await?;
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
    let models = list_models(&api_key).await?;
    secrets::set(secrets::ANTHROPIC_KEY, &api_key)?;
    Ok(models)
}

#[tauri::command]
pub async fn anthropic_models() -> Result<Vec<Model>> {
    list_models(&key()?).await
}

/// Run one AI task. Model selection (Auto/Fast/Balanced/Deep/Custom) is
/// resolved by the frontend, which owns the settings; this stays a thin,
/// key-holding proxy so the API key never reaches the webview.
#[tauri::command]
pub async fn anthropic_run(model: String, system: String, prompt: String, max_tokens: Option<u32>) -> Result<String> {
    let resp = client()
        .post(format!("{API}/messages"))
        .header("x-api-key", key()?)
        .header("anthropic-version", VERSION)
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
