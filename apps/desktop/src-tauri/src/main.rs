#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use codelearn_tool_core as _;

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("failed to run CodeLearn Tool");
}
