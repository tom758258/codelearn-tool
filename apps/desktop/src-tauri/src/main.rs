#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod compile;
mod debug;
mod run;
mod workspace;

use codelearn_tool_core::{build, toolchain};
use compile::{Desktop, DesktopState, Document, Targets, ToolKind, View, lock};
use std::{path::PathBuf, sync::Mutex};
use tauri::Manager;
use workspace::{Course, Library, Preferences, Result, Settings, Unit, UnitKey, Workspace};

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
async fn get_library(app: tauri::AppHandle) -> Result<Library> {
    with_desktop(app, |state| lock(state)?.workspace.library()).await
}

#[tauri::command]
async fn import_learning(app: tauri::AppHandle, source: PathBuf, course: bool) -> Result<Library> {
    with_desktop(app, move |state| {
        lock(state)?.workspace.import_learning(source, course)
    })
    .await
}

#[tauri::command]
async fn get_course(app: tauri::AppHandle, key: String) -> Result<Course> {
    with_desktop(app, move |state| lock(state)?.workspace.course(&key)).await
}

#[tauri::command]
async fn get_course_content(
    app: tauri::AppHandle,
    key: String,
    chapter: String,
    block: String,
) -> Result<String> {
    with_desktop(app, move |state| {
        lock(state)?.workspace.content(&key, &chapter, &block)
    })
    .await
}

#[tauri::command]
async fn get_course_image(
    app: tauri::AppHandle,
    key: String,
    chapter: String,
    block: String,
    reference: String,
) -> Result<Vec<u8>> {
    // Reading images does not mutate the selected exercise or its process.
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<DesktopState>()
            .read_image(&key, &chapter, &block, &reference)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_unit(
    app: tauri::AppHandle,
    key: Option<UnitKey>,
    discard_changes: bool,
) -> Result<Option<Unit>> {
    with_desktop(app, move |state| {
        lock(state)?.activate(key, discard_changes)
    })
    .await
}

#[tauri::command]
async fn reset_exercise(app: tauri::AppHandle, discard_changes: bool) -> Result<Unit> {
    with_desktop(app, move |state| lock(state)?.reset(discard_changes)).await
}

#[tauri::command]
async fn restore_toolchains(app: tauri::AppHandle) -> Result<View> {
    with_desktop(app, |state| {
        let mut desktop = lock(state)?;
        desktop.restore_tools();
        Ok(desktop.view())
    })
    .await
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

#[tauri::command]
async fn run_program(app: tauri::AppHandle, stdin: String) -> Result<Result<run::RunOutput>> {
    // Expected validation failures are returned as data, distinct from a lost
    // command waiter. Only the latter needs recovery through get_run_state.
    tauri::async_runtime::spawn_blocking(move || app.state::<DesktopState>().run(stdin))
        .await
        .map_err(|error| format!("The run command could not finish: {error}"))
}

#[tauri::command]
fn stop_run(state: tauri::State<'_, DesktopState>) -> bool {
    state.run_control.stop()
}

#[tauri::command]
fn get_run_state(state: tauri::State<'_, DesktopState>) -> run::RunStatus {
    state.run_control.status()
}

#[tauri::command]
async fn debug_program(
    app: tauri::AppHandle,
    id: String,
    stdin: String,
    breakpoints: Vec<debug::BreakpointRequest>,
) -> Result<Result<debug::DebugOutput>> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<DesktopState>().debug(id, stdin, breakpoints)
    })
    .await
    .map_err(|e| format!("The debug command could not finish: {e}"))
}

#[tauri::command]
fn stop_debug(state: tauri::State<'_, DesktopState>, id: String) -> bool {
    state.debug_control.stop(Some(&id))
}

#[tauri::command]
fn get_debug_state(state: tauri::State<'_, DesktopState>) -> debug::DebugStatus {
    state.debug_control.status()
}

#[tauri::command]
async fn debug_action(app: tauri::AppHandle, id: String, action: debug::Action) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<DesktopState>()
            .debug_control
            .command(&id, action)
    })
    .await
    .map_err(|e| e.to_string())?
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
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && (window.state::<DesktopState>().run_control.stop()
                    | window.state::<DesktopState>().debug_control.stop(None))
            {
                // React waits for the result and handles cleanup/unsaved-edit
                // confirmation. A missing frontend waiter cannot terminate the
                // host while its blocking worker still owns a run session.
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_workspace_settings,
            get_library,
            import_learning,
            get_course,
            get_course_content,
            get_course_image,
            open_unit,
            reset_exercise,
            restore_toolchains,
            apply_workspace_settings,
            read_authored_file,
            save_authored_file,
            set_editor_dirty,
            get_compile_state,
            set_compile_targets,
            discover_toolchains,
            validate_toolchain,
            select_toolchain,
            compile_sources,
            run_program,
            stop_run,
            get_run_state,
            debug_program,
            stop_debug,
            get_debug_state,
            debug_action,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run CodeLearn Tool");
}
