//! C11 compile/link only. Paths are checked boundaries, not an OS sandbox.
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use crate::{process, temporary::Directory};
pub use crate::{
    process::{ProcessFailure, ProcessOutput},
    temporary::CleanupReport,
};

pub const DEFAULT_COMPILE_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const FLAGS: [&str; 6] = [
    "-std=c11",
    "-Wall",
    "-Wextra",
    "-g",
    "-O0",
    "-fdiagnostics-color=never",
];

#[derive(Debug)]
pub enum Source {
    Files(Vec<PathBuf>),
    Text {
        code: String,
        /// Relative to the workspace, or an absolute directory inside it.
        logical_directory: Option<PathBuf>,
        additional_files: Vec<PathBuf>,
    },
}

#[derive(Debug)]
pub struct BuildRequest {
    /// Both roots must already exist. Core only creates its own build subdirectory.
    pub workspace_root: PathBuf,
    pub temp_root: PathBuf,
    /// Explicit absolute path; discovery never selects it for the caller.
    pub gcc: PathBuf,
    pub source: Source,
    pub include_directories: Vec<PathBuf>,
    pub timeout: Duration,
}

impl BuildRequest {
    pub fn new(workspace_root: PathBuf, temp_root: PathBuf, gcc: PathBuf, source: Source) -> Self {
        Self {
            workspace_root,
            temp_root,
            gcc,
            source,
            include_directories: Vec::new(),
            timeout: DEFAULT_COMPILE_TIMEOUT,
        }
    }
}

#[derive(Debug)]
pub enum BuildFailure {
    InvalidInput(String),
    Io(String),
    Process(ProcessFailure),
    Compile,
    MissingArtifact,
    Cleanup,
}

#[derive(Debug)]
struct ArtifactData {
    directory: Directory,
    executable: PathBuf,
}

/// Clones retain the executable and any text snapshot until the final drop.
/// An unconfirmed run cleanup can retain the directory on disk after release.
#[derive(Clone, Debug)]
pub struct BuildArtifact(Arc<ArtifactData>);

impl BuildArtifact {
    pub fn executable(&self) -> &Path {
        &self.0.executable
    }
    pub fn directory(&self) -> &Path {
        self.0.directory.path()
    }
    pub(crate) fn cleanup_report(&self) -> &CleanupReport {
        &self.0.directory.report
    }
    pub(crate) fn retain(&self) {
        self.0.directory.retain();
    }
}

#[derive(Debug)]
pub struct BuildResult {
    pub success: bool,
    pub failure: Option<BuildFailure>,
    pub output: ProcessOutput,
    /// Actual compiler inputs, including the snapshot path for text input.
    /// These identify raw diagnostic file/line locations without lossy parsing.
    pub sources: Vec<PathBuf>,
    pub artifact: Option<BuildArtifact>,
    pub cleanup: CleanupReport,
}

pub(crate) fn absolute_file(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() || !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected an existing absolute executable path",
        ));
    }
    #[cfg(windows)]
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "select an .exe tool, not a shell script",
        ));
    }
    canonical_path(path)
}

pub(crate) fn canonical_path(path: &Path) -> io::Result<PathBuf> {
    let resolved = path.canonicalize()?;
    // GCC/MinGW locates cc1 relative to argv[0] and does not understand Rust's
    // Windows verbatim prefixes. Resolve first, then use a regular absolute path.
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = resolved.components();
        let mut regular = match components.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", drive as char)),
                Prefix::VerbatimUNC(server, share) => {
                    let mut path = PathBuf::from(r"\\");
                    path.push(server);
                    path.push(share);
                    path
                }
                _ => return Ok(resolved),
            },
            _ => return Ok(resolved),
        };
        // Skip RootDir: pushing it would discard a UNC share.
        regular.extend(components.filter(|part| !matches!(part, Component::RootDir)));
        Ok(regular)
    }
    #[cfg(not(windows))]
    Ok(resolved)
}

pub(crate) fn directory(path: &Path) -> io::Result<PathBuf> {
    let path = canonical_path(path)?;
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected an existing directory",
        ));
    }
    Ok(path)
}

// Compare actual directory identities as well as canonical components (Windows
// casing, junctions and symlinks must not bypass ancestry checks).
pub(crate) fn contains(root: &Path, path: &Path) -> io::Result<bool> {
    for parent in path.ancestors() {
        if same_file::is_same_file(root, parent)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn within(root: &Path, path: &Path, is_directory: bool) -> io::Result<PathBuf> {
    let selected = if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    };
    let resolved = canonical_path(&selected)?;
    if !is_directory && resolved.extension().is_none_or(|ext| ext != "c") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source must be a .c file",
        ));
    }
    if !contains(root, &resolved)?
        || (if is_directory {
            !resolved.is_dir()
        } else {
            !resolved.is_file()
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "selected path must exist inside the workspace",
        ));
    }
    Ok(resolved)
}

pub fn compile(request: &BuildRequest) -> BuildResult {
    let mut result = BuildResult {
        success: false,
        failure: None,
        output: ProcessOutput::default(),
        sources: Vec::new(),
        artifact: None,
        cleanup: CleanupReport::default(),
    };
    let prepared = || -> io::Result<_> {
        let workspace = directory(&request.workspace_root)?;
        let temp = directory(&request.temp_root)?;
        if contains(&workspace, &temp)? || contains(&temp, &workspace)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workspace and temporary roots must not overlap",
            ));
        }
        if request.timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "compile timeout must be positive",
            ));
        }
        let gcc = absolute_file(&request.gcc)?;
        let files = match &request.source {
            Source::Files(files) => files,
            Source::Text {
                additional_files, ..
            } => additional_files,
        };
        if matches!(&request.source, Source::Files(files) if files.is_empty()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "select at least one .c source",
            ));
        }
        let files = files
            .iter()
            .map(|p| within(&workspace, p, false))
            .collect::<io::Result<Vec<_>>>()?;
        let includes = request
            .include_directories
            .iter()
            .map(|p| within(&workspace, p, true))
            .collect::<io::Result<Vec<_>>>()?;
        let logical = match &request.source {
            Source::Text {
                logical_directory, ..
            } => Some(within(
                &workspace,
                logical_directory.as_deref().unwrap_or(Path::new(".")),
                true,
            )?),
            _ => None,
        };
        Ok((temp, gcc, files, includes, logical))
    };
    let (temp, gcc, mut files, includes, logical) = match prepared() {
        Ok(value) => value,
        Err(error) => {
            result.failure = Some(BuildFailure::InvalidInput(error.to_string()));
            return result;
        }
    };
    let dir = match Directory::new(&temp, result.cleanup.clone()) {
        Ok(dir) => dir,
        Err(error) => {
            result.failure = Some(BuildFailure::Io(error.to_string()));
            return result;
        }
    };
    if let Source::Text { code, .. } = &request.source {
        let source = dir.path().join("source.c");
        if let Err(error) = fs::write(&source, code) {
            result.failure = Some(BuildFailure::Io(error.to_string()));
            return result;
        }
        files.insert(0, source);
    }
    let executable = dir.path().join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    let mut command = Command::new(gcc);
    command.current_dir(dir.path()).args(FLAGS);
    if let Some(logical) = logical {
        command.arg("-iquote").arg(logical);
    }
    for include in includes {
        command.arg("-I").arg(include);
    }
    // MinGW's linker may use the active code page for output path arguments.
    // A fixed relative name also works when the build directory contains Unicode.
    command
        .args(&files)
        .arg("-o")
        .arg(executable.file_name().unwrap());
    result.sources = files;
    result.output = process::execute(&mut command, dir.path(), request.timeout);
    result.failure = if let Some(failure) = &result.output.failure {
        Some(BuildFailure::Process(failure.clone()))
    } else if !result.output.cleanup_errors.is_empty() {
        Some(BuildFailure::Cleanup)
    } else if !result.output.success {
        Some(BuildFailure::Compile)
    } else if !executable.is_file() {
        Some(BuildFailure::MissingArtifact)
    } else {
        None
    };
    if result.failure.is_none() {
        result.success = true;
        result.artifact = Some(BuildArtifact(Arc::new(ArtifactData {
            directory: dir,
            executable,
        })));
    }
    // Failure paths drop only this build's directory before the result is observed.
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_artifact_files_outlive_all_references() {
        let root = tempfile::tempdir().unwrap();
        let directory = Directory::new(root.path(), CleanupReport::default()).unwrap();
        let path = directory.path().to_owned();
        let executable = path.join("program");
        fs::write(&executable, b"synthetic artifact").unwrap();
        let run = path.join("run-example");
        fs::create_dir(&run).unwrap();
        let artifact = BuildArtifact(Arc::new(ArtifactData {
            directory,
            executable: executable.clone(),
        }));
        let last = artifact.clone();
        let allocation = Arc::downgrade(&artifact.0);
        artifact.retain();
        drop(artifact);
        assert_eq!(last.directory(), path);
        drop(last);
        assert!(allocation.upgrade().is_none(), "artifact owner was leaked");
        assert_eq!(fs::read(executable).unwrap(), b"synthetic artifact");
        assert!(run.is_dir());
        // The test owns these synthetic files and has no running process.
        fs::remove_dir_all(path).unwrap();
        assert!(root.path().is_dir());
    }
}
