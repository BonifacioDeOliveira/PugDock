mod error;
mod git_sync;
mod integrations;
mod search;
mod secrets;
mod theme;
mod update;
mod workspace;

use error::Result;
use tauri_plugin_opener::OpenerExt;

/// Show a workspace file in the OS file manager.
#[tauri::command]
fn reveal_in_file_manager(app: tauri::AppHandle, path: String) -> Result<()> {
    let root = workspace::workspace_root(&app)?;
    let abs = workspace::resolve(&root, &path)?;
    app.opener()
        .reveal_item_in_dir(abs)
        .map_err(|e| error::AppError::Other(e.to_string()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            reveal_in_file_manager,
            // workspace
            workspace::get_app_config,
            workspace::set_app_config,
            workspace::inspect_folder,
            workspace::create_workspace,
            workspace::list_tree,
            workspace::read_file,
            workspace::read_file_base64,
            workspace::write_file,
            workspace::create_folder,
            workspace::rename_path,
            workspace::delete_path,
            workspace::duplicate_file,
            workspace::import_file,
            // secrets
            secrets::has_secret,
            secrets::delete_secret,
            // git / sync
            git_sync::git_init_workspace,
            git_sync::git_checkpoint,
            git_sync::git_push,
            git_sync::git_pull,
            git_sync::git_resolve_conflict,
            git_sync::git_conflict_versions,
            git_sync::git_status,
            git_sync::git_history,
            git_sync::git_file_at,
            // search
            search::rebuild_index,
            search::index_file,
            search::remove_from_index,
            search::search_workspace,
            search::search_context,
            search::folder_contents,
            // github
            integrations::github::github_auth_mode,
            integrations::github::github_oauth_start,
            integrations::github::github_device_start,
            integrations::github::github_device_poll,
            integrations::github::github_user,
            integrations::github::github_orgs,
            integrations::github::github_repo_exists,
            integrations::github::github_create_repo,
            // anthropic
            integrations::anthropic::anthropic_connect,
            integrations::anthropic::anthropic_auth_status,
            integrations::anthropic::anthropic_oauth_login,
            integrations::anthropic::anthropic_models,
            integrations::anthropic::anthropic_run,
            // themes
            theme::import_vsix_theme,
            theme::list_imported_themes,
            theme::get_imported_theme,
            theme::delete_imported_theme,
            // updates
            update::check_updates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
