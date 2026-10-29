//! Desktop state and thin adapters to Core.
use crate::run::{RunControl, RunOutput};
use crate::workspace::{Preferences, Result, Settings, Workspace};
use codelearn_tool_core::{
    build::{
        BuildArtifact, BuildFailure, BuildRequest, BuildResult, CleanupReport, ProcessFailure,
        Source,
    },
    run::RunRequest,
    toolchain::{Candidate, DiscoveryRequest, Origin, Tool, Validation},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct DesktopState {
    desktop: Mutex<Desktop>,
    busy: AtomicBool,
    pub run_control: RunControl,
}

impl DesktopState {
    pub fn new(workspace: Workspace) -> Self {
        Self {
            desktop: Mutex::new(Desktop::new(workspace)),
            busy: AtomicBool::new(false),
            run_control: RunControl::default(),
        }
    }

    pub fn run(&self, stdin: String) -> Result<RunOutput> {
        self.operate(|state| self.run_control.execute(|| lock(state)?.prepare_run(stdin)))
    }

    pub fn operate<T>(&self, operation: impl FnOnce(&Mutex<Desktop>) -> Result<T>) -> Result<T> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "Another operation is in progress. Please wait for it to finish")?;
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = Reset(&self.busy);
        operation(&self.desktop)
    }
}

pub fn lock(state: &Mutex<Desktop>) -> Result<MutexGuard<'_, Desktop>> {
    state.lock().map_err(|_| "The application could not recover from an operation error. Restart it before continuing".into())
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ToolKind {
    Gcc,
    Gdb,
}

impl ToolKind {
    pub fn core(self) -> Tool {
        match self {
            Self::Gcc => Tool::Gcc,
            Self::Gdb => Tool::Gdb,
        }
    }
}

#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Targets {
    pub primary: Option<String>,
    pub additional: Vec<String>,
    pub includes: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct Document {
    pub relative_path: String,
    pub display_path: String,
    pub content: String,
}

struct Editor {
    document: Document,
    dirty: bool,
}

#[derive(Clone, Serialize)]
pub struct SelectedTool {
    path: PathBuf,
    version: Option<String>,
}

#[derive(Serialize)]
pub struct ToolView {
    id: usize,
    tool: ToolKind,
    path: String,
    origin: String,
    version: Option<String>,
    usable: bool,
    error: Option<String>,
    details: String,
}

#[derive(Clone, Serialize)]
pub struct CompileOutput {
    pub success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
    cleanup_errors: Vec<String>,
}

#[derive(Serialize)]
pub struct View {
    targets: Targets,
    candidates: Vec<ToolView>,
    gcc: Option<SelectedTool>,
    gdb: Option<SelectedTool>,
    build_valid: bool,
    output: Option<CompileOutput>,
    cleanup_errors: Vec<String>,
}

pub struct PreparedCompile {
    pub request: BuildRequest,
    sources: Vec<(String, Vec<u8>)>,
}

struct CurrentBuild {
    artifact: BuildArtifact,
    cleanup: CleanupReport,
    inputs: PreparedCompile,
}

pub struct Desktop {
    pub workspace: Workspace,
    editor: Option<Editor>,
    targets: Targets,
    candidates: Vec<Candidate>,
    gcc: Option<SelectedTool>,
    gdb: Option<SelectedTool>,
    current: Option<CurrentBuild>,
    output: Option<CompileOutput>,
    cleanup_errors: Vec<String>,
}

impl Desktop {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            editor: None,
            targets: Targets::default(),
            candidates: Vec::new(),
            gcc: None,
            gdb: None,
            current: None,
            output: None,
            cleanup_errors: Vec::new(),
        }
    }

    pub fn invalidate(&mut self) {
        if let Some(current) = self.current.take() {
            drop(current.artifact);
            self.cleanup_errors.extend(current.cleanup.errors());
        }
    }

    fn allow_discard(&self, discard: bool) -> Result<()> {
        if !discard && self.editor.as_ref().is_some_and(|e| e.dirty) {
            return Err("Save or discard your edits before switching files or workspace".into());
        }
        Ok(())
    }

    pub fn open(&mut self, relative: String, discard: bool) -> Result<Document> {
        self.allow_discard(discard)?;
        let document = Document {
            content: self.workspace.read_authored(&relative)?,
            display_path: self.workspace.display_relative(&relative)?,
            relative_path: relative,
        };
        self.editor = Some(Editor {
            document: document.clone(),
            dirty: false,
        });
        Ok(document)
    }

    pub fn set_dirty(&mut self, relative: &str, dirty: bool) -> Result<()> {
        let editor = self.editor.as_mut().ok_or("No file is open")?;
        if editor.document.relative_path != relative {
            return Err("The open file has changed".into());
        }
        editor.dirty = dirty;
        if dirty {
            self.invalidate();
        }
        Ok(())
    }

    pub fn save(&mut self, relative: &str, content: String) -> Result<Document> {
        self.invalidate();
        let editor = self.editor.as_mut().ok_or("No file is open")?;
        if editor.document.relative_path != relative {
            return Err("The open file has changed".into());
        }
        self.workspace
            .save_authored(relative, &content, &editor.document.content)?;
        editor.document.content = content;
        editor.dirty = false;
        Ok(editor.document.clone())
    }

    pub fn apply(&mut self, preferences: Preferences, discard: bool) -> Result<Settings> {
        self.allow_discard(discard)?;
        let settings = self.workspace.apply(preferences)?;
        self.invalidate();
        self.editor = None;
        self.targets = Targets::default();
        self.output = None;
        Ok(settings)
    }

    fn resolve_targets(&self, targets: &Targets) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
        let mut sources = Vec::new();
        for relative in targets.primary.iter().chain(&targets.additional) {
            let path = self.workspace.authored_path(relative, false)?;
            if path.extension().is_none_or(|ext| ext != "c") {
                return Err("Choose only .c files in Authored as compilation sources".into());
            }
            push_unique(&mut sources, path)?;
        }
        let mut includes = Vec::new();
        for relative in &targets.includes {
            push_unique(&mut includes, self.workspace.authored_path(relative, true)?)?;
        }
        Ok((sources, includes))
    }

    pub fn set_targets(&mut self, targets: Targets) -> Result<()> {
        self.resolve_targets(&targets)?;
        if self.targets != targets {
            self.invalidate();
            self.targets = targets;
        }
        Ok(())
    }

    pub fn discovery_request(&self) -> Result<DiscoveryRequest> {
        let (_, temp) = self.workspace.compile_roots()?;
        let mut request = DiscoveryRequest::new(temp);
        if let Some(tool) = &self.gcc {
            request.gcc_paths.push(tool.path.clone());
        }
        if let Some(tool) = &self.gdb {
            request.gdb_paths.push(tool.path.clone());
        }
        Ok(request)
    }

    pub fn discovered(&mut self, candidates: Vec<Candidate>) {
        if self.gcc.as_ref().is_some_and(|selected| {
            !candidates
                .iter()
                .any(|c| c.tool == Tool::Gcc && c.path == selected.path && c.validation.usable)
        }) {
            self.gcc = None;
            self.invalidate();
        }
        if self.gdb.as_ref().is_some_and(|selected| {
            !candidates
                .iter()
                .any(|c| c.tool == Tool::Gdb && c.path == selected.path && c.validation.usable)
        }) {
            self.gdb = None;
        }
        self.candidates = candidates;
    }

    pub fn validated(&mut self, tool: ToolKind, path: PathBuf, validation: Validation) {
        // Validation displays a candidate; selection is always a separate user action.
        if !validation.usable {
            if tool == ToolKind::Gcc && self.gcc.as_ref().is_some_and(|s| s.path == path) {
                self.gcc = None;
                self.invalidate();
            }
            if tool == ToolKind::Gdb && self.gdb.as_ref().is_some_and(|s| s.path == path) {
                self.gdb = None;
            }
        }
        self.candidates
            .retain(|c| c.tool != tool.core() || c.path != path);
        self.candidates.push(Candidate {
            tool: tool.core(),
            path,
            origin: Origin::Explicit,
            validation,
        });
    }

    pub fn select_tool(&mut self, id: usize) -> Result<()> {
        let candidate = self
            .candidates
            .get(id)
            .ok_or("Scan or validate the tool again")?;
        if !candidate.validation.usable {
            return Err("This tool did not pass validation".into());
        }
        let selection = SelectedTool {
            path: candidate.path.clone(),
            version: candidate.validation.version.clone(),
        };
        match candidate.tool {
            Tool::Gcc => {
                self.invalidate();
                self.gcc = Some(selection);
            }
            Tool::Gdb => self.gdb = Some(selection),
        }
        Ok(())
    }

    pub fn prepare_compile(&mut self) -> Result<PreparedCompile> {
        self.invalidate();
        self.output = None;
        if self.editor.as_ref().is_some_and(|e| e.dirty) {
            return Err("Save your edits before compiling".into());
        }
        if let Some(editor) = &self.editor
            && self
                .workspace
                .read_authored(&editor.document.relative_path)?
                != editor.document.content
        {
            return Err(
                "The open file changed outside the editor. Reload it before compiling".into(),
            );
        }
        let gcc = self
            .gcc
            .as_ref()
            .ok_or("Select a validated GCC before compiling")?;
        if self.targets.primary.is_none() {
            return Err("Choose a primary .c file to compile".into());
        }
        let (files, includes) = self.resolve_targets(&self.targets)?;
        let (root, temp) = self.workspace.compile_roots()?;
        let mut request = BuildRequest::new(root, temp, gcc.path.clone(), Source::Files(files));
        request.include_directories = includes;
        let mut known: Vec<_> = self
            .targets
            .primary
            .iter()
            .chain(&self.targets.additional)
            .cloned()
            .collect();
        if let Some(editor) = &self.editor
            && !known.contains(&editor.document.relative_path)
        {
            known.push(editor.document.relative_path.clone());
        }
        let sources = known
            .into_iter()
            .map(|relative| {
                let bytes = fs::read(self.workspace.authored_path(&relative, false)?)
                    .map_err(|e| format!("Cannot read compilation source: {e}"))?;
                Ok((relative, bytes))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(PreparedCompile { request, sources })
    }

    fn inputs_match(&self, inputs: &PreparedCompile) -> Result<bool> {
        let (root, temp) = self.workspace.compile_roots()?;
        if root != inputs.request.workspace_root || temp != inputs.request.temp_root {
            return Ok(false);
        }
        self.resolve_targets(&self.targets)?;
        for (relative, expected) in &inputs.sources {
            let actual = fs::read(self.workspace.authored_path(relative, false)?)
                .map_err(|e| format!("Cannot recheck compilation source: {e}"))?;
            if &actual != expected {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn finish_compile(&mut self, inputs: PreparedCompile, mut result: BuildResult) {
        let mut error = result.failure.as_ref().map(build_error);
        let matches = match self.inputs_match(&inputs) {
            Ok(true) => true,
            Ok(false) => {
                error = Some(
                    "Sources changed during compilation. Compile again after reviewing your files"
                        .into(),
                );
                false
            }
            Err(cause) => {
                error = Some(cause);
                false
            }
        };
        let success = result.success
            && matches
            && result.artifact.is_some()
            && result.cleanup.errors().is_empty()
            && result.output.cleanup_errors.is_empty();
        if success {
            self.current = result.artifact.take().map(|artifact| CurrentBuild {
                artifact,
                cleanup: result.cleanup.clone(),
                inputs,
            });
        }
        drop(result.artifact.take());
        let mut cleanup_errors = result.output.cleanup_errors;
        cleanup_errors.extend(result.cleanup.errors());
        self.output = Some(CompileOutput {
            success,
            exit_code: result.output.exit_code,
            stdout: String::from_utf8_lossy(&result.output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&result.output.stderr).into_owned(),
            error,
            cleanup_errors,
        });
    }

    pub fn prepare_run(&mut self, stdin: String) -> Result<RunRequest> {
        if self.editor.as_ref().is_some_and(|editor| editor.dirty) {
            return Err("Save your edits and compile before running".into());
        }
        self.check_current();
        let current = self
            .current
            .as_ref()
            .ok_or("Compile the current saved sources successfully before running")?;
        let (authored, _) = self.workspace.compile_roots()?;
        let mut request = RunRequest::new(current.artifact.clone(), authored);
        request.stdin = stdin.into_bytes();
        Ok(request)
    }

    fn check_current(&mut self) {
        if self.current.as_ref().is_some_and(|current| {
            !current.artifact.executable().is_file()
                || !self.inputs_match(&current.inputs).unwrap_or(false)
        }) {
            self.invalidate();
        }
    }

    pub fn view(&mut self) -> View {
        self.check_current();
        View {
            targets: self.targets.clone(),
            candidates: self
                .candidates
                .iter()
                .enumerate()
                .map(|(id, c)| ToolView {
                    id,
                    tool: if c.tool == Tool::Gcc {
                        ToolKind::Gcc
                    } else {
                        ToolKind::Gdb
                    },
                    path: c.path.to_string_lossy().into_owned(),
                    origin: match c.origin {
                        Origin::Explicit => "Manual path".into(),
                        Origin::Path(i) => format!("PATH entry {}", i + 1),
                        Origin::CommonLocation => "Common location".into(),
                    },
                    version: c.validation.version.clone(),
                    usable: c.validation.usable,
                    error: c.validation.error.clone(),
                    details: validation_details(&c.validation),
                })
                .collect(),
            gcc: self.gcc.clone(),
            gdb: self.gdb.clone(),
            build_valid: self.current.is_some(),
            output: self.output.clone(),
            cleanup_errors: self.cleanup_errors.clone(),
        }
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) -> Result<()> {
    for previous in paths.iter() {
        if same_file::is_same_file(previous, &path)
            .map_err(|e| format!("Cannot compare selected files: {e}"))?
        {
            return Err("The same file or directory cannot be selected twice".into());
        }
    }
    paths.push(path);
    Ok(())
}

fn process_error(failure: &ProcessFailure) -> String {
    match failure {
        ProcessFailure::Start(cause) => format!("Cannot start the compiler: {cause}"),
        ProcessFailure::Timeout => "Compilation exceeded its time limit".into(),
        ProcessFailure::Wait(cause) => format!("Cannot wait for the compiler: {cause}"),
        ProcessFailure::Capture(cause) => format!("Cannot read compiler output: {cause}"),
    }
}

fn build_error(failure: &BuildFailure) -> String {
    match failure {
        BuildFailure::InvalidInput(cause) => {
            format!("Cannot compile the selected sources: {cause}")
        }
        BuildFailure::Io(cause) => format!("Cannot prepare compilation: {cause}"),
        BuildFailure::Process(cause) => process_error(cause),
        BuildFailure::Compile => "The compiler reported an error. See the output below".into(),
        BuildFailure::MissingArtifact => "The compiler did not produce an executable".into(),
        BuildFailure::Cleanup => "Compilation cleanup failed. See the details below".into(),
    }
}

fn validation_details(validation: &Validation) -> String {
    let mut details = Vec::new();
    for output in [&validation.version_output, &validation.compile_output]
        .into_iter()
        .flatten()
    {
        if let Some(failure) = &output.failure {
            details.push(process_error(failure));
        }
        if !output.stderr.is_empty() {
            details.push(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        details.extend(output.cleanup_errors.clone());
    }
    details.extend(validation.cleanup.errors());
    details.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use codelearn_tool_core::{build, toolchain};
    use std::sync::{Arc, mpsc};

    fn fixture() -> (tempfile::TempDir, DesktopState) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let workspace = Workspace::load(
            root.join("config/settings.json"),
            root.join("workspace"),
            root.join("temp"),
        );
        workspace.create_file("", "main.c").unwrap();
        workspace.create_file("", "math.h").unwrap();
        (directory, DesktopState::new(workspace))
    }

    #[test]
    fn protects_unsaved_edits_targets_and_exclusive_operations() {
        let (_directory, state) = fixture();
        state
            .operate(|state| {
                let mut desktop = lock(state)?;
                desktop.open("main.c".into(), false)?;
                assert!(desktop.prepare_run(String::new()).is_err());
                desktop.set_dirty("main.c", true)?;
                assert!(desktop.prepare_compile().is_err());
                assert!(desktop.prepare_run(String::new()).is_err());
                assert!(desktop.open("math.h".into(), false).is_err());
                assert!(desktop.apply(Preferences::default(), false).is_err());
                desktop.save("main.c", "int main(void) { return 0; }\n".into())?;
                desktop.open("math.h".into(), false)?;
                for targets in [
                    Targets {
                        primary: Some("math.h".into()),
                        ..Targets::default()
                    },
                    Targets {
                        primary: Some("../Imported/main.c".into()),
                        ..Targets::default()
                    },
                    Targets {
                        primary: Some("main.c".into()),
                        additional: vec!["main%2Ec".into()],
                        includes: vec![],
                    },
                    Targets {
                        includes: vec!["../Imported".into()],
                        ..Targets::default()
                    },
                ] {
                    assert!(desktop.set_targets(targets).is_err());
                }
                desktop.set_targets(Targets {
                    primary: Some("main.c".into()),
                    includes: vec!["".into()],
                    ..Targets::default()
                })?;
                assert!(desktop.prepare_compile().is_err()); // no validated GCC
                Ok(())
            })
            .unwrap();
        let state = Arc::new(state);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            worker_state.operate(|state| {
                drop(lock(state)?);
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Err::<(), _>("Synthetic operation failure".into())
            })
        });
        entered_rx.recv().unwrap();
        // The long operation releases the data mutex, but all command entry
        // points still reject conflicting work rather than waiting on it.
        assert!(state.desktop.try_lock().is_ok());
        assert!(
            state
                .operate(|state| lock(state)?.apply(Preferences::default(), true))
                .is_err()
        );
        assert!(
            state
                .operate(|state| lock(state)?.prepare_compile())
                .is_err()
        );
        assert!(state.run(String::new()).is_err());
        assert!(!state.run_control.stop());
        release_tx.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        assert!(
            state
                .operate(|state| Ok(lock(state)?.workspace.settings()))
                .is_ok()
        );
    }

    #[test]
    #[ignore = "Requires CODELEARN_TEST_GCC pointing to an existing GCC executable"]
    fn desktop_compile_tracks_saved_sources_and_releases_obsolete_results() {
        let gcc =
            PathBuf::from(std::env::var_os("CODELEARN_TEST_GCC").expect("Set CODELEARN_TEST_GCC"));
        let (_directory, state) = fixture();
        state.operate(|state| {
            let mut desktop = lock(state)?;
            let (root, temp) = desktop.workspace.compile_roots()?;
            fs::create_dir(root.join("include")).unwrap();
            fs::write(root.join("include/math.h"), "int answer(void);\n").unwrap();
            fs::write(root.join("math.c"), "int answer(void) { return 42; }\n").unwrap();
            let main = "#include <math.h>\nint main(void) { int unused; return answer() == 42 ? 0 : 1; }\n";
            fs::write(root.join("main.c"), main).unwrap();
            let validation = toolchain::validate(Tool::Gcc, &gcc, &temp, build::DEFAULT_COMPILE_TIMEOUT);
            assert!(validation.usable, "{:?}", validation.error);
            desktop.validated(ToolKind::Gcc, gcc.clone(), validation);
            assert!(desktop.gcc.is_none());
            desktop.select_tool(0)?;
            let missing = temp.join("missing-gdb");
            let validation = toolchain::validate(Tool::Gdb, &missing, &temp, build::DEFAULT_COMPILE_TIMEOUT);
            desktop.validated(ToolKind::Gdb, missing, validation);
            assert!(desktop.select_tool(1).is_err());
            if let Some(gdb) = std::env::var_os("CODELEARN_TEST_GDB") {
                let gdb = PathBuf::from(gdb);
                let validation = toolchain::validate(Tool::Gdb, &gdb, &temp, build::DEFAULT_COMPILE_TIMEOUT);
                assert!(validation.usable, "{:?}", validation.error);
                desktop.validated(ToolKind::Gdb, gdb, validation);
                desktop.select_tool(2)?;
            }
            desktop.set_targets(Targets { primary: Some("main.c".into()), additional: vec!["math.c".into()], includes: vec!["include".into()] })?;
            desktop.open("include/math.h".into(), false)?;
            let compile = |desktop: &mut Desktop| -> Result<()> {
                let prepared = desktop.prepare_compile()?;
                let result = build::compile(&prepared.request);
                desktop.finish_compile(prepared, result);
                Ok(())
            };
            compile(&mut desktop)?;
            assert!(desktop.view().build_valid, "{:?}", desktop.output.as_ref().and_then(|o| o.error.as_ref()));
            assert!(!desktop.output.as_ref().unwrap().stderr.is_empty()); // warnings still succeed
            let old_directory = desktop.current.as_ref().unwrap().artifact.directory().to_owned();
            desktop.set_dirty("include/math.h", true)?;
            assert!(!desktop.view().build_valid);
            assert!(desktop.prepare_run(String::new()).is_err());
            assert!(!old_directory.exists());
            assert!(desktop.prepare_compile().is_err());
            desktop.save("include/math.h", "int answer(void);\n// saved\n".into())?;
            assert!(!desktop.view().build_valid); // Save never compiles
            compile(&mut desktop)?;
            assert!(desktop.view().build_valid);
            fs::write(root.join("math.c"), "invalid C\n").unwrap();
            assert!(!desktop.view().build_valid); // external source change
            assert!(desktop.prepare_run(String::new()).is_err());
            compile(&mut desktop)?;
            assert!(!desktop.view().build_valid);
            assert!(!desktop.output.as_ref().unwrap().success);
            assert!(desktop.prepare_run(String::new()).is_err());
            fs::write(root.join("math.c"), "int answer(void) { return 42; }\n").unwrap();
            compile(&mut desktop)?;
            assert!(desktop.view().build_valid);
            let prepared = desktop.prepare_compile()?;
            let result = build::compile(&prepared.request);
            fs::write(root.join("math.c"), "int answer(void) { return 43; }\n").unwrap();
            desktop.finish_compile(prepared, result);
            assert!(!desktop.view().build_valid); // changed during the compile interval
            compile(&mut desktop)?;
            desktop.set_targets(Targets { primary: Some("math.c".into()), ..Targets::default() })?;
            assert!(!desktop.view().build_valid);
            assert!(desktop.prepare_run(String::new()).is_err());
            fs::write(root.join("math.c"), "int main(void) { return 0; }\n").unwrap();
            compile(&mut desktop)?; // single source
            assert!(desktop.view().build_valid);
            desktop.select_tool(0)?;
            assert!(!desktop.view().build_valid);
            assert!(desktop.prepare_run(String::new()).is_err());
            compile(&mut desktop)?;
            assert!(desktop.view().build_valid);
            desktop.apply(Preferences::default(), false)?;
            assert!(!desktop.view().build_valid);
            assert!(desktop.prepare_run(String::new()).is_err());
            assert!(desktop.cleanup_errors.is_empty());
            Ok(())
        }).unwrap();
    }

    #[test]
    #[ignore = "Requires CODELEARN_TEST_GCC pointing to an existing GCC executable"]
    fn desktop_run_preserves_input_results_stop_and_artifact_lifetime() {
        use codelearn_tool_core::run::{RunOutcome, RunResult};
        use std::time::{Duration, Instant};

        let gcc =
            PathBuf::from(std::env::var_os("CODELEARN_TEST_GCC").expect("Set CODELEARN_TEST_GCC"));
        let (_directory, state) = fixture();
        let state = Arc::new(state);
        assert!(state.run(String::new()).is_err());
        assert!(!state.run_control.status().active);
        let (root, artifact) = state
            .operate(|state| {
                let mut desktop = lock(state)?;
                let (root, temp) = desktop.workspace.compile_roots()?;
                fs::write(
                    root.join("main.c"),
                    r#"#include <stdio.h>
#include <string.h>
int main(void) {
    char input[128];
    if (!fgets(input, sizeof input, stdin)) return 0;
    if (strcmp(input, "wait\n") == 0) {
        FILE *ready = fopen("ready", "w");
        if (!ready) return 2;
        fclose(ready);
        for (;;) {}
    }
    fputs(input, stdout);
    fputs("example stderr\n", stderr);
    return 7;
}
"#,
                )
                .unwrap();
                let validation =
                    toolchain::validate(Tool::Gcc, &gcc, &temp, build::DEFAULT_COMPILE_TIMEOUT);
                assert!(validation.usable, "{:?}", validation.error);
                desktop.validated(ToolKind::Gcc, gcc, validation);
                desktop.select_tool(0)?;
                desktop.set_targets(Targets {
                    primary: Some("main.c".into()),
                    ..Targets::default()
                })?;
                let prepared = desktop.prepare_compile()?;
                let result = build::compile(&prepared.request);
                desktop.finish_compile(prepared, result);
                let request = desktop.prepare_run(String::new())?;
                assert_eq!(request.workspace_root, root);
                assert!(request.input_root.is_none() && request.input_files.is_empty());
                assert_eq!(
                    request.timeout,
                    codelearn_tool_core::run::DEFAULT_RUN_TIMEOUT
                );
                Ok((root, request.artifact))
            })
            .unwrap();

        for _ in 0..2 {
            let output = state.run("hello 世界\n".into()).unwrap();
            assert_eq!(output.outcome, Some("Exited"));
            assert_eq!(output.exit_code, Some(7));
            assert_eq!(output.stdout, "hello 世界\n");
            assert_eq!(output.stderr, "example stderr\n");
            assert!(output.start_error.is_none());
            assert!(output.process_errors.is_empty() && output.cleanup_errors.is_empty());
            assert!(PathBuf::from(output.working_directory.unwrap()).is_dir());
        }
        assert_eq!(state.run(String::new()).unwrap().exit_code, Some(0));

        // Stop while the blocking worker is preparing, before a handle exists.
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            worker_state.operate(|desktop| {
                worker_state.run_control.execute(|| {
                    let request = lock(desktop)?.prepare_run("wait\n".into())?;
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(request)
                })
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(state.desktop.try_lock().is_ok());
        assert!(state.run(String::new()).is_err());
        assert!(state.operate(|_| Ok(())).is_err());
        assert!(state.run_control.stop());
        assert!(state.run_control.stop());
        assert!(artifact.executable().is_file());
        release_tx.send(()).unwrap();
        let stopped = worker.join().unwrap().unwrap();
        assert_eq!(stopped.outcome, Some("Stopped"));
        assert!(stopped.cleanup_errors.is_empty());
        assert!(!state.run_control.stop());

        // Confirm program entry before exercising the running Stop path.
        for entry in fs::read_dir(artifact.directory()).unwrap().flatten() {
            let marker = entry.path().join("ready");
            if marker.exists() {
                fs::remove_file(marker).unwrap();
            }
        }
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || worker_state.run("wait\n".into()));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !fs::read_dir(artifact.directory())
            .unwrap()
            .flatten()
            .any(|e| e.path().join("ready").is_file())
        {
            assert!(Instant::now() < deadline, "program did not signal entry");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(state.run_control.stop());
        let stopped = worker.join().unwrap().unwrap();
        assert_eq!(stopped.outcome, Some("Stopped"));
        assert!(stopped.cleanup_errors.is_empty());
        assert_eq!(state.run(String::new()).unwrap().outcome, Some("Exited"));

        // Preserve termination cause independently of process and cleanup errors.
        for (outcome, expected) in [
            (RunOutcome::Timeout, "Timeout"),
            (RunOutcome::OutputLimit, "OutputLimit"),
            (RunOutcome::ProcessError, "ProcessError"),
        ] {
            let output = RunOutput::from(RunResult {
                outcome,
                exit_code: None,
                stdout: vec![0xff],
                stderr: b"stderr".to_vec(),
                process_errors: vec!["synthetic I/O failure".into()],
                cleanup_errors: vec!["synthetic cleanup failure".into()],
                working_directory: artifact.directory().join("run-synthetic"),
                artifact: artifact.clone(),
            });
            assert_eq!(output.outcome, Some(expected));
            assert_eq!(output.stdout, "�");
            assert_eq!(output.process_errors, ["synthetic I/O failure"]);
            assert_eq!(output.cleanup_errors, ["synthetic cleanup failure"]);
            assert!(output.start_error.is_none());
        }

        // A disappeared executable is rejected without launching or recompiling.
        fs::remove_file(artifact.executable()).unwrap();
        assert!(state.run(String::new()).is_err());
        assert!(!state.run_control.status().active);
        assert!(
            state
                .operate(|desktop| Ok(!lock(desktop)?.view().build_valid))
                .unwrap()
        );
        let directory = artifact.directory().to_owned();
        drop(artifact);
        assert!(!directory.exists());
        assert!(root.join("main.c").is_file());
    }
}
