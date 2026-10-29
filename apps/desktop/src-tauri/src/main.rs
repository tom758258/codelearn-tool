#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod compile;
mod workspace;

use codelearn_tool_core::{build, toolchain};
use compile::{Desktop, DesktopState, Document, Targets, ToolKind, View, lock};
use std::{path::PathBuf, sync::Mutex};
use tauri::Manager;
use workspace::{Area, Listing, Preferences, Result, Settings, Workspace};

async fn with_desktop<T: Send + 'static>(
    app: tauri::AppHandle,
    operation: impl FnOnce(&Mutex<Desktop>) -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(move || app.state::<DesktopState>().operate(operation))
        .await
        .map_err(|e| format!("The operation could not finish: {e}"))?
}

#[tauri::command]
async fn get_workspace_settings(app: tauri::AppHandle) -> Result<Settings> {
    with_desktop(app, |state| Ok(lock(state)?.workspace.settings())).await
}

#[tauri::command]
async fn apply_workspace_settings(
    app: tauri::AppHandle,
    preferences: Preferences,
    discard_changes: bool,
) -> Result<Settings> {
    with_desktop(app, move |state| {
        lock(state)?.apply(preferences, discard_changes)
    })
    .await
}

#[tauri::command]
async fn list_workspace_entries(
    app: tauri::AppHandle,
    area: Area,
    relative_path: String,
) -> Result<Listing> {
    with_desktop(app, move |state| {
        lock(state)?.workspace.list(area, &relative_path)
    })
    .await
}

#[tauri::command]
async fn import_to_workspace(app: tauri::AppHandle, source: PathBuf) -> Result<()> {
    with_desktop(app, move |state| lock(state)?.workspace.import(source)).await
}

#[tauri::command]
async fn copy_to_authored(app: tauri::AppHandle, relative_path: String) -> Result<()> {
    with_desktop(app, move |state| {
        let mut desktop = lock(state)?;
        desktop.invalidate();
        desktop.workspace.copy_to_authored(&relative_path)
    })
    .await
}

#[tauri::command]
async fn create_authored_file(
    app: tauri::AppHandle,
    parent_relative_path: String,
    filename: String,
) -> Result<()> {
    with_desktop(app, move |state| {
        let mut desktop = lock(state)?;
        desktop.invalidate();
        desktop
            .workspace
            .create_file(&parent_relative_path, &filename)
    })
    .await
}

#[tauri::command]
async fn read_authored_file(
    app: tauri::AppHandle,
    relative_path: String,
    discard_changes: bool,
) -> Result<Document> {
    with_desktop(app, move |state| {
        lock(state)?.open(relative_path, discard_changes)
    })
    .await
}

#[tauri::command]
async fn save_authored_file(
    app: tauri::AppHandle,
    relative_path: String,
    content: String,
) -> Result<Document> {
    with_desktop(app, move |state| lock(state)?.save(&relative_path, content)).await
}

#[tauri::command]
async fn set_editor_dirty(app: tauri::AppHandle, relative_path: String, dirty: bool) -> Result<()> {
    with_desktop(app, move |state| {
        lock(state)?.set_dirty(&relative_path, dirty)
    })
    .await
}

#[tauri::command]
async fn get_compile_state(app: tauri::AppHandle) -> Result<View> {
    with_desktop(app, |state| Ok(lock(state)?.view())).await
}

#[tauri::command]
async fn set_compile_targets(app: tauri::AppHandle, targets: Targets) -> Result<View> {
    with_desktop(app, move |state| {
        let mut desktop = lock(state)?;
        desktop.set_targets(targets)?;
        Ok(desktop.view())
    })
    .await
}

#[tauri::command]
async fn discover_toolchains(app: tauri::AppHandle) -> Result<View> {
    with_desktop(app, |state| {
        let request = lock(state)?.discovery_request()?;
        let candidates = toolchain::discover(&request);
        let mut desktop = lock(state)?;
        desktop.discovered(candidates);
        Ok(desktop.view())
    })
    .await
}

#[tauri::command]
async fn validate_toolchain(app: tauri::AppHandle, tool: ToolKind, path: PathBuf) -> Result<View> {
    with_desktop(app, move |state| {
        if !path.is_absolute() {
            return Err("Choose an absolute path to the tool executable".into());
        }
        let (_, temp) = lock(state)?.workspace.compile_roots()?;
        let validation =
            toolchain::validate(tool.core(), &path, &temp, build::DEFAULT_COMPILE_TIMEOUT);
        let mut desktop = lock(state)?;
        desktop.validated(tool, path, validation);
        Ok(desktop.view())
    })
    .await
}

#[tauri::command]
async fn select_toolchain(app: tauri::AppHandle, id: usize) -> Result<View> {
    with_desktop(app, move |state| {
        let mut desktop = lock(state)?;
        desktop.select_tool(id)?;
        Ok(desktop.view())
    })
    .await
}

#[tauri::command]
async fn compile_sources(app: tauri::AppHandle) -> Result<View> {
    with_desktop(app, |state| {
        let prepared = lock(state)?.prepare_compile()?;
        // The operation guard rejects conflicting commands, while the workspace
        // mutex is released for the entire compiler process.
        let result = build::compile(&prepared.request);
        let mut desktop = lock(state)?;
        desktop.finish_compile(prepared, result);
        Ok(desktop.view())
    })
    .await
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = app.path();
            app.manage(DesktopState::new(Workspace::load(
                paths.app_config_dir()?.join("workspace-settings.json"),
                paths.app_data_dir()?.join("LearningWorkspace"),
                paths.app_cache_dir()?.join("CodeLearnTemp"),
            )));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_workspace_settings,
            apply_workspace_settings,
            list_workspace_entries,
            import_to_workspace,
            copy_to_authored,
            create_authored_file,
            read_authored_file,
            save_authored_file,
            set_editor_dirty,
            get_compile_state,
            set_compile_targets,
            discover_toolchains,
            validate_toolchain,
            select_toolchain,
            compile_sources,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run CodeLearn Tool");
}
