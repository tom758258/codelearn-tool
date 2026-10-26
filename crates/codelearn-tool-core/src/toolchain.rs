//! Candidate discovery and independent GCC/GDB validation; no selection or storage.
use crate::{
    build::{CleanupReport, DEFAULT_COMPILE_TIMEOUT, FLAGS, absolute_file, canonical_path},
    process::{self, ProcessOutput},
    temporary::Directory,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Gcc,
    Gdb,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Explicit,
    Path(usize),
    CommonLocation,
}

#[derive(Debug)]
pub struct Validation {
    pub usable: bool,
    pub version: Option<String>,
    pub error: Option<String>,
    pub version_output: Option<ProcessOutput>,
    /// GCC only: a real compile/link, never execution of the probe executable.
    pub compile_output: Option<ProcessOutput>,
    pub cleanup: CleanupReport,
}

#[derive(Debug)]
pub struct Candidate {
    pub tool: Tool,
    pub path: PathBuf,
    pub origin: Origin,
    pub validation: Validation,
}

#[derive(Debug)]
pub struct DiscoveryRequest {
    pub gcc_paths: Vec<PathBuf>,
    pub gdb_paths: Vec<PathBuf>,
    /// Existing directory for controlled validation probes.
    pub temp_root: PathBuf,
    pub timeout: Duration,
}

impl DiscoveryRequest {
    pub fn new(temp_root: PathBuf) -> Self {
        Self {
            gcc_paths: Vec::new(),
            gdb_paths: Vec::new(),
            temp_root,
            timeout: DEFAULT_COMPILE_TIMEOUT,
        }
    }
}

pub fn validate(tool: Tool, path: &Path, temp_root: &Path, timeout: Duration) -> Validation {
    let mut result = Validation {
        usable: false,
        version: None,
        error: None,
        version_output: None,
        compile_output: None,
        cleanup: CleanupReport::default(),
    };
    let check = |result: &mut Validation| -> Result<(), String> {
        if timeout.is_zero() {
            return Err("validation timeout must be positive".into());
        }
        let path = absolute_file(path).map_err(|e| e.to_string())?;
        let temp_root = canonical_path(temp_root).map_err(|e| e.to_string())?;
        let dir = Directory::new(&temp_root, result.cleanup.clone()).map_err(|e| e.to_string())?;
        let mut command = Command::new(&path);
        command.current_dir(dir.path()).arg("--version");
        let version = process::execute(&mut command, dir.path(), timeout);
        let text = String::from_utf8_lossy(&version.stdout);
        let first = text.lines().next().unwrap_or_default().to_owned();
        let identified = match tool {
            Tool::Gcc => first.to_ascii_lowercase().contains("gcc"),
            Tool::Gdb => first.to_ascii_lowercase().contains("gnu gdb"),
        };
        let version_ok = version.success && identified;
        if !first.is_empty() {
            result.version = Some(first);
        }
        result.version_output = Some(version);
        if !version_ok {
            return Err(
                "version query failed or executable did not identify as the requested GNU tool"
                    .into(),
            );
        }
        if tool == Tool::Gcc {
            let source = dir.path().join("probe.c");
            let executable = dir
                .path()
                .join(if cfg!(windows) { "probe.exe" } else { "probe" });
            fs::write(
                &source,
                "#include <stdio.h>\nint main(void) { return puts(\"CodeLearn probe\") < 0; }\n",
            )
            .map_err(|e| e.to_string())?;
            let mut command = Command::new(&path);
            command
                .current_dir(dir.path())
                .args(FLAGS)
                .arg(&source)
                .arg("-o")
                .arg(executable.file_name().unwrap());
            let output = process::execute(&mut command, dir.path(), timeout);
            let success = output.success && executable.is_file();
            result.compile_output = Some(output);
            if !success {
                return Err("GCC compile/link validation failed".into());
            }
        }
        Ok(())
    };
    if let Err(error) = check(&mut result) {
        result.error = Some(error);
    }
    if !result.cleanup.errors().is_empty() {
        result
            .error
            .get_or_insert("validation directory cleanup failed".into());
    }
    result.usable = result.error.is_none();
    result
}

/// Suggested order: usable first, then explicit paths, PATH order, common paths.
/// Every entry retains its origin so the caller can explain and select a tool.
pub fn discover(request: &DiscoveryRequest) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = Vec::new();
    for tool in [Tool::Gcc, Tool::Gdb] {
        let explicit = match tool {
            Tool::Gcc => &request.gcc_paths,
            Tool::Gdb => &request.gdb_paths,
        };
        let name = match (tool, cfg!(windows)) {
            (Tool::Gcc, true) => "gcc.exe",
            (Tool::Gdb, true) => "gdb.exe",
            (Tool::Gcc, false) => "gcc",
            (Tool::Gdb, false) => "gdb",
        };
        let mut paths: Vec<_> = explicit
            .iter()
            .cloned()
            .map(|p| (p, Origin::Explicit))
            .collect();
        if let Some(path) = env::var_os("PATH") {
            paths.extend(
                env::split_paths(&path)
                    .enumerate()
                    .map(|(index, dir)| (dir.join(name), Origin::Path(index))),
            );
        }
        let common: &[&str] = if cfg!(windows) {
            &["C:/msys64/ucrt64/bin", "C:/msys64/mingw64/bin"]
        } else {
            &["/usr/bin", "/usr/local/bin", "/bin"]
        };
        paths.extend(
            common
                .iter()
                .map(|dir| (Path::new(dir).join(name), Origin::CommonLocation)),
        );
        for (path, origin) in paths {
            if origin != Origin::Explicit && !path.is_file() {
                continue;
            }
            // PATH may contain relative entries; resolve against this process's cwd.
            let path = if origin != Origin::Explicit {
                std::path::absolute(&path).unwrap_or(path)
            } else {
                path
            };
            let path = if path.is_absolute() {
                canonical_path(&path).unwrap_or(path)
            } else {
                path
            };
            if candidates.iter().any(|c| {
                c.tool == tool
                    && (c.path == path
                        || (c.path.is_absolute()
                            && path.is_absolute()
                            && same_file::is_same_file(&c.path, &path).unwrap_or(false)))
            }) {
                continue;
            }
            let validation = validate(tool, &path, &request.temp_root, request.timeout);
            candidates.push(Candidate {
                tool,
                path,
                origin,
                validation,
            });
        }
    }
    candidates.sort_by_key(|c| {
        (
            !c.validation.usable,
            match c.origin {
                Origin::Explicit => (0, 0),
                Origin::Path(index) => (1, index),
                Origin::CommonLocation => (2, 0),
            },
        )
    });
    candidates
}
