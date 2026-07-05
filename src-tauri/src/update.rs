use crate::error::Result;
use serde::Serialize;

/// Public repo that hosts releases. Set at build time:
///   PUGDOCK_UPDATE_REPO=youruser/pugdock npm run tauri build
const UPDATE_REPO: &str = match option_env!("PUGDOCK_UPDATE_REPO") {
    Some(r) => r,
    None => "",
};

#[derive(Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub notes: String,
    pub url: String,
}

fn version_key(v: &str) -> Vec<u32> {
    v.trim_start_matches('v')
        .split(['.', '-'])
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// Check GitHub Releases for a newer version. Returns None when up to date
/// (or when no update repo is configured for this build).
#[tauri::command]
pub async fn check_updates(app: tauri::AppHandle, include_prerelease: Option<bool>) -> Result<Option<UpdateInfo>> {
    if UPDATE_REPO.is_empty() {
        return Ok(None);
    }
    let current = app.package_info().version.to_string();
    let releases: Vec<serde_json::Value> = reqwest::Client::builder()
        .user_agent("PugDock")
        .build()
        .expect("reqwest client")
        .get(format!("https://api.github.com/repos/{UPDATE_REPO}/releases?per_page=10"))
        .send()
        .await?
        .json()
        .await?;
    let latest = releases.iter().find(|r| {
        !r["draft"].as_bool().unwrap_or(false)
            && (include_prerelease.unwrap_or(false) || !r["prerelease"].as_bool().unwrap_or(false))
    });
    let Some(rel) = latest else { return Ok(None) };
    let tag = rel["tag_name"].as_str().unwrap_or_default();
    if version_key(tag) <= version_key(&current) {
        return Ok(None);
    }
    Ok(Some(UpdateInfo {
        current,
        latest: tag.into(),
        notes: rel["body"].as_str().unwrap_or("").into(),
        url: rel["html_url"].as_str().unwrap_or("").into(),
    }))
}

#[cfg(test)]
mod tests {
    use super::version_key;

    #[test]
    fn version_ordering() {
        assert!(version_key("v0.1.1") > version_key("0.1.0"));
        assert!(version_key("1.0.0") > version_key("0.9.9"));
        assert!(version_key("0.1.0") <= version_key("v0.1.0"));
    }
}
