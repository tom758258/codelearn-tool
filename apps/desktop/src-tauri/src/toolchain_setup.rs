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
    lock(state)?.discovered(candidates);
    let selected = tools.map(|tools| tools.iter().map(|tool| tool.core()).collect::<Vec<_>>());
    let assessment = setup::assess(
        Some(&root),
        selected.as_deref(),
        &available,
        &request.temp_root,
    )?;
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
