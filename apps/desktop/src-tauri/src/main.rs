#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod workspace;

use codelearn_tool_core as _;
use std::{path::PathBuf, sync::Mutex};
use tauri::Manager;
use workspace::{Area, Listing, Preferences, Result, Settings, Workspace, WorkspaceState};

async fn with_workspace<T: Send + 'static>(
    app: tauri::AppHandle,
    operation: impl FnOnce(&mut Workspace) -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<WorkspaceState>();
        let mut workspace = state
            .0
            .try_lock()
            .map_err(|_| "Workspace is busy; try again after the current operation")?;
        operation(&mut workspace)
    })
    .await
    .map_err(|e| format!("Workspace operation failed: {e}"))?
}

#[tauri::command]
async fn get_workspace_settings(app: tauri::AppHandle) -> Result<Settings> {
    with_workspace(app, |workspace| Ok(workspace.settings())).await
}

#[tauri::command]
async fn apply_workspace_settings(
    app: tauri::AppHandle,
    preferences: Preferences,
) -> Result<Settings> {
    with_workspace(app, move |workspace| workspace.apply(preferences)).await
}

#[tauri::command]
async fn list_workspace_entries(
    app: tauri::AppHandle,
    area: Area,
    relative_path: String,
) -> Result<Listing> {
    with_workspace(app, move |workspace| workspace.list(area, &relative_path)).await
}

#[tauri::command]
async fn import_to_workspace(app: tauri::AppHandle, source: PathBuf) -> Result<()> {
    with_workspace(app, move |workspace| workspace.import(source)).await
}

#[tauri::command]
async fn copy_to_authored(app: tauri::AppHandle, relative_path: String) -> Result<()> {
    with_workspace(app, move |workspace| {
        workspace.copy_to_authored(&relative_path)
    })
    .await
}

#[tauri::command]
async fn create_authored_file(
    app: tauri::AppHandle,
    parent_relative_path: String,
    filename: String,
) -> Result<()> {
    with_workspace(app, move |workspace| {
        workspace.create_file(&parent_relative_path, &filename)
    })
    .await
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = app.path();
            app.manage(WorkspaceState(Mutex::new(Workspace::load(
                paths.app_config_dir()?.join("workspace-settings.json"),
                paths.app_data_dir()?.join("LearningWorkspace"),
                paths.app_cache_dir()?.join("CodeLearnTemp"),
            ))));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_workspace_settings,
            apply_workspace_settings,
            list_workspace_entries,
            import_to_workspace,
            copy_to_authored,
            create_authored_file,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run CodeLearn Tool");
}
