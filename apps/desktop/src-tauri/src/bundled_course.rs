//! Embedded, offline course materials for the Desktop distribution.
use std::{
    fs,
    path::{Component, Path},
};

use crate::workspace::{Library, Result, Workspace};

include!(concat!(env!("OUT_DIR"), "/bundled_course_files.rs"));

pub fn ensure_builtin_course(workspace: &Workspace) -> Result<Library> {
    let library = workspace.library()?;
    let manifest = BUNDLED_COURSE_FILES
        .iter()
        .find(|(path, _)| *path == "course.json")
        .ok_or("The bundled course has no course.json")?;
    let course: serde_json::Value = serde_json::from_slice(manifest.1)
        .map_err(|e| format!("Invalid bundled course manifest: {e}"))?;
    let id = course["id"]
        .as_str()
        .ok_or("Bundled course ID is missing")?;
    let version = course["version"]
        .as_str()
        .ok_or("Bundled course version is missing")?;
    if library
        .courses
        .iter()
        .any(|entry| entry.id == id && entry.version == version)
    {
        return Ok(library);
    }

    // Reuse the same validation, safe-copy, and registration contract as Import course.
    // Only the bundled materials go into this temporary staging directory.
    let staging = tempfile::Builder::new()
        .prefix("codelearn-builtin-")
        .tempdir()
        .map_err(|e| format!("Cannot prepare bundled course: {e}"))?;
    for (name, bytes) in BUNDLED_COURSE_FILES {
        let relative = Path::new(name);
        if !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err("Invalid path in bundled course".into());
        }
        let destination = staging.path().join(relative);
        fs::create_dir_all(destination.parent().ok_or("Invalid bundled course path")?)
            .map_err(|e| format!("Cannot prepare bundled course directory: {e}"))?;
        fs::write(destination, bytes)
            .map_err(|e| format!("Cannot write bundled course staging file: {e}"))?;
    }
    workspace.import_learning(staging.path().to_path_buf(), true)
}
