//! Serialization and existing tool-validation integration for the read-only setup check.
use crate::{
    compile::{Desktop, ToolKind, lock},
    workspace::Result,
};
use codelearn_tool_core::{toolchain, toolchain_setup as setup};
use serde::Serialize;
use std::{path::PathBuf, sync::Mutex};

#[derive(Serialize)]
pub struct Package {
    repository: String,
    name: String,
    version: String,
    installed_version: Option<String>,
}

#[derive(Serialize)]
pub struct View {
    root: PathBuf,
    gcc_version: Option<String>,
    gdb_version: Option<String>,
    selected: Vec<ToolKind>,
    preview: Vec<Package>,
    preview_error: Option<String>,
    installation_block: &'static str,
}

pub fn assess(
    state: &Mutex<Desktop>,
    root: Option<PathBuf>,
    tools: Option<Vec<ToolKind>>,
) -> Result<View> {
    let root = setup::locate(root.as_deref())?;
    // Reuse existing validation and candidate presentation. Never select or save a tool here.
    let mut request = lock(state)?.discovery_request()?;
    for (tool, path) in setup::tool_paths(&root) {
        match tool {
            toolchain::Tool::Gcc => request.gcc_paths.push(path),
            toolchain::Tool::Gdb => request.gdb_paths.push(path),
        }
    }
    let candidates = toolchain::discover(&request);
    let available: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.validation.usable)
        .map(|candidate| candidate.tool)
        .collect();
    let selected = tools.map(|tools| tools.iter().map(|tool| tool.core()).collect::<Vec<_>>());
    let assessment = setup::assess(
        Some(&root),
        selected.as_deref(),
        &available,
        &request.temp_root,
    );
    finish_assessment(state, candidates, assessment)
}

fn finish_assessment(
    state: &Mutex<Desktop>,
    candidates: Vec<toolchain::Candidate>,
    assessment: Result<setup::Assessment>,
) -> Result<View> {
    // Publish discovery only after pacman queries succeeded. A failed setup
    // check must not change the active selections or replace the candidate list.
    let assessment = assessment?;
    if assessment.preview_error.is_none() {
        lock(state)?.discovered(candidates);
    }
    Ok(View {
        root: assessment.root,
        gcc_version: assessment.gcc_version,
        gdb_version: assessment.gdb_version,
        selected: assessment.selected.into_iter().map(kind).collect(),
        preview: assessment
            .preview
            .into_iter()
            .map(|package| Package {
                repository: package.repository,
                name: package.name,
                version: package.version,
                installed_version: package.installed_version,
            })
            .collect(),
        preview_error: assessment.preview_error,
        installation_block: setup::INSTALLATION_BLOCK,
    })
}

fn kind(tool: toolchain::Tool) -> ToolKind {
    match tool {
        toolchain::Tool::Gcc => ToolKind::Gcc,
        toolchain::Tool::Gdb => ToolKind::Gdb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile::DesktopState, workspace::Workspace};

    fn candidate(tool: toolchain::Tool, path: PathBuf, usable: bool) -> toolchain::Candidate {
        toolchain::Candidate {
            tool,
            path,
            origin: toolchain::Origin::Explicit,
            validation: toolchain::Validation {
                usable,
                version: Some("Synthetic tool 1".into()),
                error: (!usable).then(|| "Synthetic validation failure".into()),
                version_output: None,
                compile_output: None,
                cleanup: Default::default(),
            },
        }
    }

    fn selected_fixture(dir: &std::path::Path) -> DesktopState {
        let workspace = Workspace::load(
            dir.join("settings.json"),
            dir.join("workspace"),
            dir.join("temp"),
        );
        let state = DesktopState::new(workspace);
        state
            .operate(|state| {
                let mut desktop = lock(state)?;
                std::fs::create_dir_all(dir.join("tools")).unwrap();
                for (index, tool) in [ToolKind::Gcc, ToolKind::Gdb].into_iter().enumerate() {
                    let path = dir.join("tools/../tools").join(if tool == ToolKind::Gcc {
                        "gcc.exe"
                    } else {
                        "gdb.exe"
                    });
                    std::fs::write(&path, "synthetic executable identity; never executed").unwrap();
                    desktop.validated(
                        tool,
                        path.clone(),
                        candidate(tool.core(), path, true).validation,
                    );
                    desktop.select_tool(index)?;
                }
                Ok(())
            })
            .unwrap();
        state
    }

    fn assessment(root: PathBuf) -> setup::Assessment {
        setup::Assessment {
            root,
            gcc_version: None,
            gdb_version: None,
            selected: vec![],
            preview: vec![],
            preview_error: None,
        }
    }

    #[test]
    fn late_query_failure_keeps_active_and_saved_choices_and_releases_guard() {
        let dir = tempfile::tempdir().unwrap();
        let state = selected_fixture(dir.path());
        let before = state
            .operate(|state| Ok(serde_json::to_value(lock(state)?.view()).unwrap()))
            .unwrap();
        let saved = std::fs::read(dir.path().join("settings.json")).unwrap();
        let rejected_candidates = vec![candidate(
            toolchain::Tool::Gcc,
            dir.path().join("missing"),
            false,
        )];
        assert!(
            state
                .operate(|state| finish_assessment(
                    state,
                    rejected_candidates,
                    Err("Synthetic pacman -Q failure".into())
                ))
                .is_err()
        );
        let after = state
            .operate(|state| Ok(serde_json::to_value(lock(state)?.view()).unwrap()))
            .unwrap();
        assert_eq!(after, before);
        let mut partial = assessment(dir.path().to_owned());
        partial.preview_error = Some("Synthetic pacman preview failure".into());
        state
            .operate(|state| finish_assessment(state, vec![], Ok(partial)))
            .unwrap();
        let after = state
            .operate(|state| Ok(serde_json::to_value(lock(state)?.view()).unwrap()))
            .unwrap();
        assert_eq!(after, before);
        assert_eq!(
            std::fs::read(dir.path().join("settings.json")).unwrap(),
            saved
        );
    }

    #[test]
    fn successful_setup_keeps_same_file_aliases_and_invalidates_failed_tools_only() {
        let dir = tempfile::tempdir().unwrap();
        let state = selected_fixture(dir.path());
        let before = state
            .operate(|state| Ok(serde_json::to_value(lock(state)?.view()).unwrap()))
            .unwrap();
        let saved = std::fs::read(dir.path().join("settings.json")).unwrap();
        let candidates = [toolchain::Tool::Gcc, toolchain::Tool::Gdb]
            .into_iter()
            .map(|tool| {
                let name = if tool == toolchain::Tool::Gcc {
                    "gcc.exe"
                } else {
                    "gdb.exe"
                };
                candidate(
                    tool,
                    dir.path().join("tools").join(name).canonicalize().unwrap(),
                    true,
                )
            })
            .collect();
        state
            .operate(|state| {
                finish_assessment(state, candidates, Ok(assessment(dir.path().to_owned())))
            })
            .unwrap();
        let after = state
            .operate(|state| Ok(serde_json::to_value(lock(state)?.view()).unwrap()))
            .unwrap();
        assert_eq!(after["gcc"], before["gcc"]);
        assert_eq!(after["gdb"], before["gdb"]);
        // A successful check with genuinely failed validations retains the old
        // invalidation behavior, while persisted preferences remain unchanged.
        let candidates = [toolchain::Tool::Gcc, toolchain::Tool::Gdb]
            .into_iter()
            .map(|tool| {
                let name = if tool == toolchain::Tool::Gcc {
                    "gcc.exe"
                } else {
                    "gdb.exe"
                };
                candidate(tool, dir.path().join("tools").join(name), false)
            })
            .collect();
        state
            .operate(|state| {
                finish_assessment(state, candidates, Ok(assessment(dir.path().to_owned())))
            })
            .unwrap();
        let after = state
            .operate(|state| Ok(serde_json::to_value(lock(state)?.view()).unwrap()))
            .unwrap();
        assert!(after["gcc"].is_null() && after["gdb"].is_null());
        assert_eq!(
            std::fs::read(dir.path().join("settings.json")).unwrap(),
            saved
        );
    }

    #[test]
    fn rejected_setup_preserves_saved_tool_choices_and_releases_operation() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("settings.json");
        let mut workspace = Workspace::load(
            config.clone(),
            dir.path().join("workspace"),
            dir.path().join("temp"),
        );
        let gcc = dir.path().join("existing-gcc.exe");
        let gdb = dir.path().join("existing-gdb.exe");
        workspace.save_tool(true, gcc.clone()).unwrap();
        workspace.save_tool(false, gdb.clone()).unwrap();
        let before = std::fs::read(&config).unwrap();
        let state = DesktopState::new(workspace);
        assert!(
            state
                .operate(|state| assess(state, Some(dir.path().join("missing-msys2")), None))
                .is_err()
        );
        state
            .operate(|state| {
                let desktop = lock(state)?;
                assert_eq!(desktop.workspace.gcc, Some(gcc));
                assert_eq!(desktop.workspace.gdb, Some(gdb));
                Ok(())
            })
            .unwrap();
        assert_eq!(std::fs::read(config).unwrap(), before);
    }
}
