use serde::Serialize;

/// All errors crossing the Tauri boundary. `code` lets the UI pick a
/// friendly message; `message` is a safe human-readable fallback.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("You are offline. Your files are saved locally and will sync later.")]
    Offline,
    #[error("GitHub access expired. Please reconnect GitHub.")]
    GithubAuthExpired,
    #[error("GitHub error: {0}")]
    Github(String),
    #[error("PugDock cannot write to this folder. Choose another folder.")]
    FolderPermission,
    #[error("This file changed in two places. Choose which version to keep.")]
    SyncConflict,
    #[error("Anthropic API key is invalid. Please check the key and try again.")]
    AnthropicKeyInvalid,
    #[error("Connect Anthropic to use AI features.")]
    AiNotConnected,
    #[error("No workspace is open.")]
    NoWorkspace,
    #[error("Path is outside the workspace.")]
    PathOutsideWorkspace,
    #[error("Git is not installed. PugDock needs Git to sync. Install it from https://git-scm.com and restart.")]
    GitMissing,
    #[error("{0}")]
    Other(String),
}

#[derive(Serialize)]
struct ErrorPayload {
    code: &'static str,
    message: String,
}

impl AppError {
    fn code(&self) -> &'static str {
        match self {
            AppError::Offline => "offline",
            AppError::GithubAuthExpired => "github_auth_expired",
            AppError::Github(_) => "github",
            AppError::FolderPermission => "folder_permission",
            AppError::SyncConflict => "sync_conflict",
            AppError::AnthropicKeyInvalid => "anthropic_key_invalid",
            AppError::AiNotConnected => "ai_not_connected",
            AppError::NoWorkspace => "no_workspace",
            AppError::PathOutsideWorkspace => "path_outside_workspace",
            AppError::GitMissing => "git_missing",
            AppError::Other(_) => "other",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        ErrorPayload { code: self.code(), message: self.to_string() }.serialize(s)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            AppError::FolderPermission
        } else {
            AppError::Other(e.to_string())
        }
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_connect() || e.is_timeout() {
            AppError::Offline
        } else {
            AppError::Other(e.to_string())
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Other(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
