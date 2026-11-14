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
        if !relative.components().all(|part| matches!(part, Component::Normal(_))) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::UnitKey;

    #[test]
    fn bundled_course_is_idempotent_and_preserves_student_work() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config");
        fs::create_dir_all(&config).unwrap();
        let workspace = Workspace::load(
            config.join("settings.json"),
            directory.path().join("workspace"),
            directory.path().join("cache"),
        );
        assert!(workspace.settings().error.is_none());

        let first = ensure_builtin_course(&workspace).unwrap();
        assert_eq!(first.courses.len(), 1);
        let entry = &first.courses[0];
        assert_eq!(entry.id, "c-foundations");
        assert_eq!(entry.version, "1.0");
        assert_eq!(workspace.course(&entry.key).unwrap().chapters.len(), 17);

        let exercise = workspace
            .unit(UnitKey::Course {
                course: entry.key.clone(),
                chapter: "c01-first-program".into(),
                block: "exercise".into(),
            })
            .unwrap();
        let path = exercise.targets.primary.as_deref().unwrap();
        let before = workspace.read_authored(path).unwrap();
        workspace
            .save_authored(path, "// student edit\n", &before)
            .unwrap();
        let second = ensure_builtin_course(&workspace).unwrap();
        assert_eq!(second.courses.len(), 1);
        assert_eq!(second.courses[0].key, entry.key);
        assert_eq!(workspace.read_authored(path).unwrap(), "// student edit\n");
    }
}
