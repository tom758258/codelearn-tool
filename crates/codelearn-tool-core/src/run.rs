//! Supervised execution of a build artifact. Process control is not a sandbox.
use std::{
    io,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex, MutexGuard, mpsc},
    thread,
    time::{Duration, Instant},
};

use crate::{build::BuildArtifact, process};
mod pipes;

pub const DEFAULT_RUN_TIMEOUT: Duration = Duration::from_secs(30);
pub const OUTPUT_LIMIT: usize = 25 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub struct RunRequest {
    pub artifact: BuildArtifact,
    pub workspace_root: PathBuf,
    pub stdin: Vec<u8>,
    pub timeout: Duration,
    /// Relative to the workspace, or an absolute directory inside it.
    /// Required only when input_files is nonempty.
    pub input_root: Option<PathBuf>,
    /// Explicit files relative to input_root; no directory scanning.
    pub input_files: Vec<PathBuf>,
}

impl RunRequest {
    pub fn new(artifact: BuildArtifact, workspace_root: PathBuf) -> Self {
        Self {
            artifact,
            workspace_root,
            stdin: Vec::new(),
            timeout: DEFAULT_RUN_TIMEOUT,
            input_root: None,
            input_files: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Exited,
    Stopped,
    Timeout,
    OutputLimit,
    ProcessError,
}

#[derive(Debug)]
pub struct RunResult {
    pub outcome: RunOutcome,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Wait or necessary I/O failures, separate from the termination cause.
    pub process_errors: Vec<String>,
    pub cleanup_errors: Vec<String>,
    pub working_directory: PathBuf,
    /// Retains all run directories until the last artifact reference is dropped.
    /// Unconfirmed process cleanup retains them on disk even after that drop.
    pub artifact: BuildArtifact,
}

#[derive(Debug)]
pub struct RunStartError {
    pub error: io::Error,
    pub cleanup_errors: Vec<String>,
}

impl std::fmt::Display for RunStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not start run: {}", self.error)
    }
}
impl std::error::Error for RunStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
impl From<io::Error> for RunStartError {
    fn from(error: io::Error) -> Self {
        Self {
            error,
            cleanup_errors: Vec::new(),
        }
    }
}

#[derive(Default, Debug)]
struct State {
    requested: Option<RunOutcome>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exceeded: bool,
    errors: Vec<String>,
    closed: bool,
}
impl State {
    fn request(&mut self, reason: RunOutcome) {
        if !self.closed {
            self.requested.get_or_insert(reason);
        }
    }
    fn error(&mut self, error: String) {
        if !self.closed {
            self.errors.push(error);
            self.request(RunOutcome::ProcessError);
        }
    }
}
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Debug)]
pub struct RunStopHandle(Arc<Mutex<State>>);
impl RunStopHandle {
    /// Request cancellation without waiting for termination or cleanup.
    pub fn stop(&self) {
        lock(&self.0).request(RunOutcome::Stopped);
    }
}

#[derive(Debug)]
pub struct RunSession {
    stop: RunStopHandle,
    result: Option<mpsc::Receiver<RunResult>>,
    artifact: BuildArtifact,
    working_directory: PathBuf,
}
impl RunSession {
    pub fn stop_handle(&self) -> RunStopHandle {
        self.stop.clone()
    }
    pub fn stop(&self) {
        self.stop.stop();
    }
    pub fn wait(mut self) -> RunResult {
        // Disarm Drop before waiting: consuming a session must not cancel it.
        self.result
            .take()
            .unwrap()
            .recv()
            .unwrap_or_else(|_| RunResult {
                outcome: RunOutcome::ProcessError,
                exit_code: None,
                stdout: Vec::new(),
                stderr: Vec::new(),
                process_errors: vec!["run supervisor terminated unexpectedly".into()],
                cleanup_errors: vec!["supervisor cleanup could not be confirmed".into()],
                artifact: self.artifact.clone(),
                working_directory: self.working_directory.clone(),
            })
    }
}
impl Drop for RunSession {
    fn drop(&mut self) {
        if self.result.is_some() {
            self.stop();
        }
    }
}

/// Prepare selected inputs and launch the process, then return without waiting
/// for its exit. No shell, arguments, or interactive stdin are exposed.
pub fn start(request: RunRequest) -> Result<RunSession, RunStartError> {
    let state = Arc::new(Mutex::new(State::default()));
    let stop = RunStopHandle(state.clone());
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    // Create the supervisor before acquiring OS process resources. Failure to
    // create this thread cannot strand a child or a partially prepared directory.
    thread::Builder::new()
        .name("codelearn-run".into())
        .spawn(move || {
            supervise(request, state, ready_tx, result_tx);
        })?;
    let (artifact, working_directory) = ready_rx
        .recv()
        .map_err(|_| io::Error::other("run supervisor terminated during startup"))??;
    Ok(RunSession {
        stop,
        result: Some(result_rx),
        artifact,
        working_directory,
    })
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn prepare(request: &RunRequest) -> Result<tempfile::TempDir, RunStartError> {
    if request.timeout.is_zero() {
        return Err(invalid("run timeout must be positive").into());
    }
    crate::runtime::prepare(
        &request.artifact,
        &request.workspace_root,
        &request.input_root,
        &request.input_files,
        "run",
    )
    .map_err(|e| RunStartError {
        error: e.error,
        cleanup_errors: e.cleanup_errors,
    })
}

fn close_directory(directory: tempfile::TempDir, errors: &mut Vec<String>) {
    if let Err(error) = directory.close() {
        errors.push(format!("run directory cleanup: {error}"));
    }
}

fn finish_process(
    child: &mut Child,
    mut terminate_group: impl FnMut() -> io::Result<()>,
    status: &mut Option<ExitStatus>,
    pipes: &mut pipes::Workers,
    errors: &mut Vec<String>,
) -> bool {
    let mut group_cleaned = false;
    // The initial cleanup and at most one retry use the existing finite waits.
    // Pipe cleanup happens once, before retrying any unresolved process cleanup.
    for attempt in 0..2 {
        if !group_cleaned {
            match terminate_group() {
                Ok(()) => group_cleaned = true,
                Err(error) => errors.push(format!("process group cleanup: {error}")),
            }
        }
        if status.is_none() {
            *status = process::stop_and_reap(child, errors);
        }
        if attempt == 0 {
            pipes.finish(errors);
        }
        if group_cleaned && status.is_some() {
            return true;
        }
    }
    false
}

fn retain_artifact(artifact: &BuildArtifact, errors: &mut Vec<String>) {
    artifact.retain();
    errors.push(format!(
        "process cleanup could not be confirmed after two attempts; retained artifact directory: {}; \
         confirm all processes using this artifact have terminated before manually removing this directory; \
         no further automatic process cleanup or directory removal will be attempted",
        artifact.directory().display()
    ));
    // Keep failures observable even when a session was dropped without wait().
    for error in errors {
        artifact.cleanup_report().record(error);
    }
}

type Ready = mpsc::SyncSender<Result<(BuildArtifact, PathBuf), RunStartError>>;
fn supervise(
    mut request: RunRequest,
    state: Arc<Mutex<State>>,
    ready: Ready,
    result_sender: mpsc::SyncSender<RunResult>,
) {
    let directory = match prepare(&request) {
        Ok(directory) => directory,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut command = Command::new(request.artifact.executable());
    command
        .current_dir(directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut cleanup_errors = Vec::new();
    let (mut child, group) = match process::spawn(&mut command, &mut cleanup_errors) {
        Ok(process) => process,
        Err(error) => {
            drop(command);
            close_directory(directory, &mut cleanup_errors);
            let _ = ready.send(Err(RunStartError {
                error,
                cleanup_errors,
            }));
            return;
        }
    };
    drop(command);
    let started = Instant::now();
    let mut pipes = pipes::Workers::new(state.clone(), request.artifact.clone());
    let setup = pipes.start(&mut child, std::mem::take(&mut request.stdin));
    if let Err(error) = setup {
        let mut status = None;
        if finish_process(
            &mut child,
            || group.terminate(),
            &mut status,
            &mut pipes,
            &mut cleanup_errors,
        ) {
            // This also removes the partial directory when only the retry
            // succeeds, before the startup error is returned to the caller.
            close_directory(directory, &mut cleanup_errors);
        } else {
            retain_artifact(&request.artifact, &mut cleanup_errors);
            let _ = directory.keep();
        }
        drop(pipes);
        #[cfg(windows)]
        drop(group);
        drop(child);
        drop(request);
        let _ = ready.send(Err(RunStartError {
            error,
            cleanup_errors,
        }));
        return;
    }
    let working_directory = directory.keep();
    if ready
        .send(Ok((request.artifact.clone(), working_directory.clone())))
        .is_err()
    {
        lock(&state).request(RunOutcome::Stopped);
    }
    let mut status = None;
    let mut outcome = loop {
        // A late stop request must not turn an already exited child into Stopped.
        match child.try_wait() {
            Ok(Some(exit)) => {
                status = Some(exit);
                break RunOutcome::Exited;
            }
            Err(error) => {
                lock(&state).error(format!("wait: {error}"));
                break RunOutcome::ProcessError;
            }
            Ok(None) => {}
        }
        let mut state = lock(&state);
        // Compare durations instead of adding to Instant: Duration::MAX is valid.
        if started.elapsed() >= request.timeout {
            state.request(RunOutcome::Timeout);
        }
        if let Some(reason) = state.requested.clone() {
            break reason;
        }
        drop(state);
        thread::sleep(POLL.min(request.timeout.saturating_sub(started.elapsed())));
    };
    if !finish_process(
        &mut child,
        || group.terminate(),
        &mut status,
        &mut pipes,
        &mut cleanup_errors,
    ) {
        retain_artifact(&request.artifact, &mut cleanup_errors);
    }
    let mut captured = lock(&state);
    // A final pipe buffer may be collected after natural exit. Report overflow
    // and I/O failure, but never apply late cancellation to that natural exit.
    if outcome == RunOutcome::Exited {
        if captured.exceeded {
            outcome = RunOutcome::OutputLimit;
        } else if !captured.errors.is_empty() {
            outcome = RunOutcome::ProcessError;
        }
    }
    captured.closed = true;
    let result = RunResult {
        outcome,
        exit_code: status.and_then(|s| s.code()),
        stdout: std::mem::take(&mut captured.stdout),
        stderr: std::mem::take(&mut captured.stderr),
        process_errors: std::mem::take(&mut captured.errors),
        cleanup_errors,
        working_directory,
        artifact: request.artifact.clone(),
    };
    drop(captured);
    drop(pipes);
    #[cfg(windows)]
    drop(group);
    drop(child);
    drop(request);
    let _ = result_sender.send(result);
}
