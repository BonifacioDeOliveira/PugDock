use crate::error::{AppError, Result};

const SERVICE: &str = "com.pugdock.app";

pub const GITHUB_TOKEN: &str = "github_token";
pub const ANTHROPIC_KEY: &str = "anthropic_api_key";

fn entry(name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, name).map_err(|e| AppError::Other(e.to_string()))
}

pub fn set(name: &str, value: &str) -> Result<()> {
    entry(name)?.set_password(value).map_err(|e| AppError::Other(e.to_string()))
}

pub fn get(name: &str) -> Result<Option<String>> {
    match entry(name)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Other(e.to_string())),
    }
}

pub fn delete(name: &str) -> Result<()> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Other(e.to_string())),
    }
}

#[tauri::command]
pub fn has_secret(name: String) -> Result<bool> {
    Ok(get(&name)?.is_some())
}

#[tauri::command]
pub fn delete_secret(name: String) -> Result<()> {
    delete(&name)
}
