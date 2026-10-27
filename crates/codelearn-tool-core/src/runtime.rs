//! Selected runtime files shared by Run and Debug; no public execution contract.
use crate::build::{self, BuildArtifact};
use std::{
    fs::{self, OpenOptions},
    io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug)]
pub(crate) struct PreparationError {
    pub error: io::Error,
    pub cleanup_errors: Vec<String>,
}
impl From<io::Error> for PreparationError {
    fn from(error: io::Error) -> Self {
        Self {
            error,
            cleanup_errors: Vec::new(),
        }
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
pub(crate) fn prepare(
    artifact: &BuildArtifact,
    workspace_root: &Path,
    input_root: &Option<PathBuf>,
    input_files: &[PathBuf],
    prefix: &str,
) -> Result<tempfile::TempDir, PreparationError> {
    let workspace = build::directory(workspace_root)?;
    let input_root = match input_root {
        Some(root) => {
            let root = build::directory(&workspace.join(root))?;
            if !build::contains(&workspace, &root)? {
                return Err(invalid("input root must be inside the workspace").into());
            }
            root
        }
        None if input_files.is_empty() => workspace,
        None => return Err(invalid("select an input root for runtime input files").into()),
    };
    let mut sources = Vec::new();
    for relative in input_files {
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(invalid(
                "runtime input paths must contain only relative normal components",
            )
            .into());
        }
        // Reject Windows stream names, device names and normalization aliases
        // before opening destinations, even on case-sensitive directories.
        #[cfg(windows)]
        for component in relative.components() {
            let name = component.as_os_str().to_string_lossy();
            let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
            if name.contains(':')
                || name.ends_with(['.', ' '])
                || matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                )
                || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.chars().count() == 4
                    && stem
                        .chars()
                        .last()
                        .is_some_and(|c| "123456789¹²³".contains(c)))
            {
                return Err(invalid("runtime input contains a Windows path alias").into());
            }
        }
        let source = build::canonical_path(&input_root.join(relative))?;
        if !source.is_file() || !build::contains(&input_root, &source)? {
            return Err(invalid("runtime input must be a file inside the input root").into());
        }
        sources.push(source);
    }
    build::absolute_file(artifact.executable())?;
    let directory = tempfile::Builder::new()
        .prefix(&format!("{prefix}-"))
        .tempdir_in(artifact.directory())?;
    let copy = || -> io::Result<()> {
        for (source, relative) in sources.iter().zip(input_files) {
            let destination = directory.path().join(relative);
            fs::create_dir_all(destination.parent().unwrap())?;
            // Exclusive creation lets the destination filesystem reject aliases,
            // duplicate names, and file/directory collisions without overwriting.
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)?;
            let mut input = fs::File::open(source)?;
            io::copy(&mut input, &mut output)?;
        }
        Ok(())
    };
    if let Err(error) = copy() {
        let mut error = PreparationError::from(error);
        if let Err(cleanup) = directory.close() {
            error
                .cleanup_errors
                .push(format!("{prefix} directory cleanup: {cleanup}"));
        }
        return Err(error);
    }
    Ok(directory)
}
