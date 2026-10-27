//! GDB/MI3 sessions for one local, all-stop inferior. Supervision is not a sandbox.
use crate::{
    build::{self, BuildArtifact},
    process, runtime,
};
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
mod mi;
mod processes;
mod transport;
use mi::{Record, Value};
use processes::Processes;
use transport::Transport;

pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_RUNNING_TIMEOUT: Duration = Duration::from_secs(30);
pub const OUTPUT_LIMIT: usize = 25 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(5);

#[derive(Debug)]
pub struct DebugRequest {
    pub artifact: BuildArtifact,
    /// Explicitly selected existing absolute GDB path.
    pub gdb: PathBuf,
    pub workspace_root: PathBuf,
    pub stdin: Vec<u8>,
    pub input_root: Option<PathBuf>,
    pub input_files: Vec<PathBuf>,
    pub startup_timeout: Duration,
    pub command_timeout: Duration,
    pub running_timeout: Duration,
}
impl DebugRequest {
    pub fn new(artifact: BuildArtifact, gdb: PathBuf, workspace_root: PathBuf) -> Self {
        Self {
            artifact,
            gdb,
            workspace_root,
            stdin: Vec::new(),
            input_root: None,
            input_files: Vec::new(),
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            running_timeout: DEFAULT_RUNNING_TIMEOUT,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugState {
    Starting,
    Ready,
    Running,
    Stopped,
    Exited,
    Finished,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugOutcome {
    Exited,
    Stopped,
    Timeout,
    CommandTimeout,
    OutputLimit,
    DebuggerError,
    ProcessError,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    MainEntry,
    Breakpoint,
    Step,
    Pause,
    Signal,
    Other,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackFrame {
    pub level: u32,
    pub function: Option<String>,
    pub file: Option<PathBuf>,
    pub line: Option<u32>,
    pub address: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub value: Option<String>,
    pub is_argument: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Breakpoint {
    pub id: String,
    pub file: Option<PathBuf>,
    pub line: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugEvent {
    Running,
    Stopped {
        reason: StopReason,
        frame: Option<StackFrame>,
        signal: Option<String>,
        breakpoint: Option<String>,
    },
    Output {
        stream: OutputStream,
        bytes: Vec<u8>,
    },
    Exited {
        exit_code: Option<i32>,
        signal: Option<String>,
    },
    DebuggerError(String),
    Finished {
        outcome: DebugOutcome,
    },
}
#[derive(Debug)]
pub struct DebugResult {
    pub outcome: DebugOutcome,
    pub exit_code: Option<i32>,
    pub process_errors: Vec<String>,
    pub cleanup_errors: Vec<String>,
    pub working_directory: PathBuf,
    pub artifact: BuildArtifact,
}
#[derive(Debug)]
pub struct DebugStartError {
    pub error: io::Error,
    pub cleanup_errors: Vec<String>,
}
impl From<io::Error> for DebugStartError {
    fn from(error: io::Error) -> Self {
        Self {
            error,
            cleanup_errors: Vec::new(),
        }
    }
}
impl std::fmt::Display for DebugStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not start debug session: {}", self.error)
    }
}
impl std::error::Error for DebugStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugCommandError {
    InvalidState(DebugState),
    InvalidInput(String),
    Busy,
    Gdb(String),
    Timeout,
    Finished,
}
impl std::fmt::Display for DebugCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "debug command failed: {self:?}")
    }
}
impl std::error::Error for DebugCommandError {}
#[derive(Clone, Debug)]
pub struct DebugStopHandle(Arc<AtomicBool>);
impl DebugStopHandle {
    pub fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }
}

#[derive(Debug)]
pub struct DebugSession {
    stop: DebugStopHandle,
    state: Arc<Mutex<DebugState>>,
    commands: mpsc::SyncSender<Operation>,
    events: Option<mpsc::Receiver<DebugEvent>>,
    result: Option<mpsc::Receiver<DebugResult>>,
    command_timeout: Duration,
    artifact: BuildArtifact,
    working_directory: PathBuf,
}
impl DebugSession {
    pub fn state(&self) -> DebugState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }
    pub fn stop_handle(&self) -> DebugStopHandle {
        self.stop.clone()
    }
    pub fn stop(&self) {
        self.stop.stop();
    }
    /// Take the sole bounded receiver. Consume it while the session runs.
    pub fn take_events(&mut self) -> Option<mpsc::Receiver<DebugEvent>> {
        self.events.take()
    }
    fn command(&self, kind: Kind) -> Result<Reply, DebugCommandError> {
        if self.stop.0.load(Ordering::Acquire) || self.state() == DebugState::Finished {
            return Err(DebugCommandError::Finished);
        }
        let (response, rx) = mpsc::sync_channel(1);
        self.commands
            .try_send(Operation {
                kind,
                response,
                started: Instant::now(),
            })
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => DebugCommandError::Busy,
                mpsc::TrySendError::Disconnected(_) => DebugCommandError::Finished,
            })?;
        match rx.recv_timeout(self.command_timeout.saturating_add(Duration::from_secs(1))) {
            Ok(reply) => reply,
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(DebugCommandError::Finished),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.stop();
                Err(DebugCommandError::Timeout)
            }
        }
    }
    pub fn set_breakpoint(
        &self,
        file: impl Into<PathBuf>,
        line: u32,
    ) -> Result<Breakpoint, DebugCommandError> {
        match self.command(Kind::SetBreakpoint(file.into(), line))? {
            Reply::Breakpoint(value) => Ok(value),
            _ => unreachable!(),
        }
    }
    pub fn remove_breakpoint(&self, id: &str) -> Result<(), DebugCommandError> {
        self.command(Kind::RemoveBreakpoint(id.into())).map(|_| ())
    }
    /// Success means command acceptance; wait for Stopped to inspect the result.
    pub fn continue_execution(&self) -> Result<(), DebugCommandError> {
        self.command(Kind::Continue).map(|_| ())
    }
    pub fn step_into(&self) -> Result<(), DebugCommandError> {
        self.command(Kind::StepInto).map(|_| ())
    }
    pub fn step_over(&self) -> Result<(), DebugCommandError> {
        self.command(Kind::StepOver).map(|_| ())
    }
    pub fn step_out(&self) -> Result<(), DebugCommandError> {
        self.command(Kind::StepOut).map(|_| ())
    }
    pub fn pause(&self) -> Result<(), DebugCommandError> {
        self.command(Kind::Pause).map(|_| ())
    }
    pub fn call_stack(&self) -> Result<Vec<StackFrame>, DebugCommandError> {
        match self.command(Kind::Stack)? {
            Reply::Stack(value) => Ok(value),
            _ => unreachable!(),
        }
    }
    pub fn variables(&self, frame: u32) -> Result<Vec<Variable>, DebugCommandError> {
        match self.command(Kind::Variables(frame))? {
            Reply::Variables(value) => Ok(value),
            _ => unreachable!(),
        }
    }
    pub fn wait(mut self) -> DebugResult {
        self.result.take().unwrap().recv().unwrap_or_else(|_| {
            self.artifact.retain();
            self.artifact
                .cleanup_report()
                .record("debug supervisor stopped unexpectedly; artifact retained");
            DebugResult {
                outcome: DebugOutcome::ProcessError,
                exit_code: None,
                process_errors: vec!["debug supervisor stopped unexpectedly".into()],
                cleanup_errors: vec!["debug cleanup unconfirmed; artifact retained".into()],
                artifact: self.artifact.clone(),
                working_directory: self.working_directory.clone(),
            }
        })
    }
}
impl Drop for DebugSession {
    fn drop(&mut self) {
        if self.result.is_some() {
            self.stop();
        }
    }
}

#[derive(Debug)]
enum Kind {
    Start,
    SetBreakpoint(PathBuf, u32),
    RemoveBreakpoint(String),
    Continue,
    StepInto,
    StepOver,
    StepOut,
    Pause,
    Stack,
    Variables(u32),
}
impl Kind {
    fn resumes(&self) -> bool {
        matches!(
            self,
            Self::Start | Self::Continue | Self::StepInto | Self::StepOver | Self::StepOut
        )
    }
    fn wire(&self, stopped_thread: Option<&str>) -> Result<Vec<u8>, DebugCommandError> {
        let wire: String = match self {
            Self::Start => "-exec-run --start".into(),
            Self::Continue => "-exec-continue".into(),
            Self::StepInto => "-exec-step".into(),
            Self::StepOver => "-exec-next".into(),
            Self::StepOut => "-exec-finish".into(),
            Self::Pause => "-exec-interrupt".into(),
            Self::Stack => format!(
                "-stack-list-frames --thread {}",
                stopped_thread.ok_or_else(malformed)?
            ),
            Self::Variables(frame) => format!(
                "-stack-list-variables --thread {} --frame {frame} --all-values",
                stopped_thread.ok_or_else(malformed)?
            ),
            Self::SetBreakpoint(file, line) => {
                if *line == 0 || !file.is_absolute() || !file.is_file() {
                    return Err(DebugCommandError::InvalidInput(
                        "select an existing absolute source path and positive line".into(),
                    ));
                }
                let file = build::canonical_path(file)
                    .map_err(|e| DebugCommandError::InvalidInput(e.to_string()))?;
                let mut wire = b"-break-insert --source ".to_vec();
                wire.extend(
                    quote_path(&file)
                        .map_err(|e| DebugCommandError::InvalidInput(e.to_string()))?,
                );
                wire.extend(format!(" --line {line}").into_bytes());
                return Ok(wire);
            }
            Self::RemoveBreakpoint(id) => {
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
                    return Err(DebugCommandError::InvalidInput(
                        "invalid breakpoint id".into(),
                    ));
                }
                format!("-break-delete {id}")
            }
        };
        Ok(wire.into_bytes())
    }
}
#[derive(Debug)]
enum Reply {
    Accepted,
    Breakpoint(Breakpoint),
    Stack(Vec<StackFrame>),
    Variables(Vec<Variable>),
}
#[derive(Debug)]
struct Operation {
    kind: Kind,
    response: mpsc::SyncSender<Result<Reply, DebugCommandError>>,
    started: Instant,
}
struct Pending {
    token: u64,
    kind: Kind,
    response: Option<mpsc::SyncSender<Result<Reply, DebugCommandError>>>,
    started: Instant,
}

type Ready = mpsc::SyncSender<Result<(BuildArtifact, PathBuf), DebugStartError>>;
pub fn start(request: DebugRequest) -> Result<DebugSession, DebugStartError> {
    let stop = DebugStopHandle(Arc::default());
    let state = Arc::new(Mutex::new(DebugState::Starting));
    let (commands, command_rx) = mpsc::sync_channel(1);
    let (events, event_rx) = mpsc::sync_channel(128);
    let (ready, ready_rx) = mpsc::sync_channel(1);
    let (result, result_rx) = mpsc::sync_channel(1);
    let timeout = request.command_timeout;
    let control = Control {
        stop: stop.clone(),
        state: state.clone(),
        commands: command_rx,
        events,
    };
    thread::Builder::new()
        .name("codelearn-debug".into())
        .spawn(move || supervise(request, control, ready, result))?;
    let (artifact, working_directory) = ready_rx
        .recv()
        .map_err(|_| io::Error::other("debug supervisor stopped during startup"))??;
    Ok(DebugSession {
        stop,
        state,
        commands,
        events: Some(event_rx),
        result: Some(result_rx),
        command_timeout: timeout,
        artifact,
        working_directory,
    })
}
struct Control {
    stop: DebugStopHandle,
    state: Arc<Mutex<DebugState>>,
    commands: mpsc::Receiver<Operation>,
    events: mpsc::SyncSender<DebugEvent>,
}
struct Capture {
    stdout: File,
    stderr: File,
    delivered: usize,
}
impl Capture {
    fn read(&mut self, engine: &mut Engine, drain: bool) {
        let sizes = self.stdout.metadata().and_then(|out| {
            self.stderr
                .metadata()
                .map(|err| out.len().saturating_add(err.len()))
        });
        match sizes {
            Ok(size) if size > OUTPUT_LIMIT as u64 => {
                if matches!(engine.outcome, None | Some(DebugOutcome::Exited)) {
                    engine.outcome = Some(DebugOutcome::OutputLimit);
                }
            }
            Err(e) => engine.process_error(format!("student output metadata: {e}")),
            _ => {}
        }
        for (stream, file) in [
            (OutputStream::Stdout, &mut self.stdout),
            (OutputStream::Stderr, &mut self.stderr),
        ] {
            loop {
                let remaining = OUTPUT_LIMIT.saturating_sub(self.delivered);
                if remaining == 0 {
                    break;
                }
                let mut buffer = vec![0; remaining.min(8192)];
                match file.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        buffer.truncate(n);
                        self.delivered += n;
                        engine.emit(DebugEvent::Output {
                            stream,
                            bytes: buffer,
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        engine.process_error(format!("student output read: {e}"));
                        break;
                    }
                }
                if !drain || engine.event_failed {
                    break;
                }
            }
        }
    }
}
struct Engine {
    control: Control,
    state: DebugState,
    transport: Transport,
    next_token: u64,
    pending: Option<Pending>,
    operation_wait: Option<(bool, Instant)>, // true: resume, false: interrupt
    running_since: Option<Instant>,
    main_expected: bool,
    pause_requested: bool,
    stopped_thread: Option<String>,
    outcome: Option<DebugOutcome>,
    exit_code: Option<i32>,
    errors: Vec<String>,
    event_failed: bool,
}
impl Engine {
    fn new(control: Control) -> Self {
        Self {
            control,
            state: DebugState::Starting,
            transport: Transport::new(),
            next_token: 1,
            pending: None,
            operation_wait: None,
            running_since: None,
            main_expected: true,
            pause_requested: false,
            stopped_thread: None,
            outcome: None,
            exit_code: None,
            errors: Vec::new(),
            event_failed: false,
        }
    }
    fn state(&mut self, state: DebugState) {
        self.state = state;
        *self.control.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
    }
    fn process_error(&mut self, error: String) {
        if self.errors.len() < 16 {
            self.errors.push(error);
        }
        if matches!(self.outcome, None | Some(DebugOutcome::Exited)) {
            self.outcome = Some(DebugOutcome::ProcessError);
        }
    }
    fn emit(&mut self, event: DebugEvent) {
        if !self.event_failed && self.control.events.try_send(event).is_err() {
            self.event_failed = true;
            self.process_error(
                "debug event receiver disconnected or queue full; event delivery stopped".into(),
            );
        }
    }
    fn finish_event(&mut self) {
        let mut event = DebugEvent::Finished {
            outcome: self.outcome.unwrap_or(DebugOutcome::ProcessError),
        };
        let began = Instant::now();
        // Earlier backpressure must not suppress the terminal notification once
        // the consumer catches up. Retry only for this finite delivery window.
        loop {
            match self.control.events.try_send(event) {
                Ok(()) => return,
                Err(mpsc::TrySendError::Full(returned))
                    if began.elapsed() < Duration::from_millis(100) =>
                {
                    event = returned;
                    thread::sleep(POLL);
                }
                Err(_) => {
                    self.process_error("could not deliver debug Finished event".into());
                    return;
                }
            }
        }
    }
    fn debugger_error(&mut self, error: String, outcome: DebugOutcome) {
        self.outcome.get_or_insert(outcome);
        if self.errors.len() < 16 {
            self.errors.push(error.clone());
        }
        self.emit(DebugEvent::DebuggerError(error));
    }
    fn token(&mut self) -> u64 {
        let token = self.next_token;
        self.next_token += 1;
        token
    }
    fn initialize(
        &mut self,
        command: impl AsRef<[u8]>,
        began: Instant,
        timeout: Duration,
    ) -> io::Result<Value> {
        if began.elapsed() >= timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "GDB startup timeout",
            ));
        }
        let token = self.token();
        self.transport.send(token, command)?;
        loop {
            if began.elapsed() >= timeout {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "GDB startup timeout",
                ));
            }
            match self.transport.records.recv_timeout(POLL) {
                Ok(Record::Result {
                    token: Some(received),
                    class,
                    data,
                }) if received == token => {
                    return if class == "done" {
                        Ok(data)
                    } else {
                        Err(io::Error::other(
                            text(&data, "msg").unwrap_or("GDB initialization failed"),
                        ))
                    };
                }
                Ok(Record::Result { .. }) => {
                    return Err(io::Error::other("mismatched GDB startup response"));
                }
                Ok(Record::Prompt | Record::Stream { .. } | Record::Async { .. }) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other("GDB disconnected during startup"));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if let Some(error) = self.transport.fault() {
                return Err(io::Error::other(error));
            }
        }
    }
    fn dispatch(
        &mut self,
        kind: Kind,
        response: Option<mpsc::SyncSender<Result<Reply, DebugCommandError>>>,
        started: Instant,
    ) {
        let wire = match kind.wire(self.stopped_thread.as_deref()) {
            Ok(wire) => wire,
            Err(error) => {
                if let Some(response) = response {
                    let _ = response.send(Err(error));
                }
                return;
            }
        };
        let token = self.token();
        if let Err(error) = self.transport.send(token, &wire) {
            if let Some(response) = response {
                let _ = response.send(Err(DebugCommandError::Gdb(error.to_string())));
            }
            self.debugger_error(error.to_string(), DebugOutcome::DebuggerError);
            return;
        }
        if kind.resumes() || matches!(kind, Kind::Pause) {
            self.operation_wait = Some((kind.resumes(), started));
        }
        if matches!(kind, Kind::Pause) {
            self.pause_requested = true;
        }
        self.pending = Some(Pending {
            token,
            kind,
            response,
            started,
        });
    }
    fn commands(&mut self, timeout: Duration) {
        if let Ok(op) = self.control.commands.try_recv() {
            let error = if self.outcome.is_some() {
                Some(DebugCommandError::Finished)
            } else if op.started.elapsed() >= timeout {
                Some(DebugCommandError::Timeout)
            } else if self.pending.is_some() || self.operation_wait.is_some() {
                Some(DebugCommandError::Busy)
            } else if self.state
                != if matches!(op.kind, Kind::Pause) {
                    DebugState::Running
                } else {
                    DebugState::Stopped
                }
            {
                Some(DebugCommandError::InvalidState(self.state))
            } else {
                None
            };
            if let Some(error) = error {
                let _ = op.response.send(Err(error));
            } else {
                self.dispatch(op.kind, Some(op.response), op.started);
            }
        }
    }
    fn record(&mut self, record: Record) {
        match record {
            Record::Result { token, class, data } => {
                let Some(pending) = self.pending.take() else {
                    self.debugger_error(
                        "unsolicited GDB result".into(),
                        DebugOutcome::DebuggerError,
                    );
                    return;
                };
                if token != Some(pending.token) {
                    if let Some(response) = pending.response {
                        let _ = response
                            .send(Err(DebugCommandError::Gdb("mismatched GDB token".into())));
                    }
                    self.debugger_error("mismatched GDB token".into(), DebugOutcome::DebuggerError);
                    return;
                }
                let reply = if class == "error" {
                    Err(DebugCommandError::Gdb(
                        text(&data, "msg").unwrap_or("GDB command rejected").into(),
                    ))
                } else {
                    parse_reply(&pending.kind, &class, &data)
                };
                if let Err(error) = &reply {
                    self.operation_wait = None;
                    self.pause_requested = false;
                    if matches!(pending.kind, Kind::Start) {
                        self.debugger_error(error.to_string(), DebugOutcome::DebuggerError);
                    }
                }
                if let Some(response) = pending.response {
                    let _ = response.send(reply);
                }
            }
            Record::Async {
                kind: b'*',
                class,
                data: _,
            } if class == "running" => {
                if self.outcome.is_none() {
                    if self.state != DebugState::Running {
                        self.running_since = Some(Instant::now());
                        self.state(DebugState::Running);
                        self.emit(DebugEvent::Running);
                    }
                    if matches!(self.operation_wait, Some((true, _))) {
                        self.operation_wait = None;
                    }
                }
            }
            Record::Async {
                kind: b'*',
                class,
                data,
            } if class == "stopped" => self.stopped(data),
            Record::Async {
                kind: b'=',
                class,
                data,
            } if class == "thread-group-exited" => {
                if let Some(code) = text(&data, "exit-code") {
                    self.exit_code = exit_code(code);
                }
            }
            Record::Stream { kind: b'@', bytes } if !bytes.is_empty() => self.debugger_error(
                "unexpected target output on MI channel; student I/O is not isolated".into(),
                DebugOutcome::DebuggerError,
            ),
            _ => {}
        }
    }
    fn stopped(&mut self, data: Value) {
        let reason = text(&data, "reason").unwrap_or("unknown");
        self.running_since = None;
        self.operation_wait = None;
        if matches!(reason, "exited" | "exited-normally" | "exited-signalled") {
            if reason == "exited-normally" {
                self.exit_code = Some(0);
            } else if let Some(code) = text(&data, "exit-code") {
                self.exit_code = exit_code(code);
            }
            self.state(DebugState::Exited);
            self.emit(DebugEvent::Exited {
                exit_code: self.exit_code,
                signal: text(&data, "signal-name").map(str::to_owned),
            });
            self.outcome.get_or_insert(DebugOutcome::Exited);
        } else if self.outcome.is_none() {
            let frame = data.field("frame").and_then(|v| frame(v, Some(0)).ok());
            let signal = text(&data, "signal-name").map(str::to_owned);
            let stopped = match reason {
                "breakpoint-hit"
                    if self.main_expected
                        && text(&data, "disp") == Some("del")
                        && frame.as_ref().and_then(|f| f.function.as_deref()) == Some("main") =>
                {
                    StopReason::MainEntry
                }
                "breakpoint-hit" => StopReason::Breakpoint,
                "end-stepping-range" | "function-finished" | "location-reached" => StopReason::Step,
                "signal-received"
                    if self.pause_requested
                        && (signal.as_deref() == Some("SIGINT")
                            || (cfg!(windows) && signal.as_deref() == Some("SIGTRAP"))) =>
                {
                    StopReason::Pause
                }
                "signal-received" => StopReason::Signal,
                _ => StopReason::Other,
            };
            self.stopped_thread = text(&data, "thread-id")
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .map(str::to_owned);
            self.state(DebugState::Stopped);
            self.emit(DebugEvent::Stopped {
                reason: stopped,
                frame,
                signal,
                breakpoint: text(&data, "bkptno").map(str::to_owned),
            });
        }
        self.pause_requested = false;
        self.main_expected = false;
    }
    fn expire(&mut self, command_timeout: Duration, running_timeout: Duration) {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.started.elapsed() >= command_timeout)
            || self
                .operation_wait
                .is_some_and(|(_, began)| began.elapsed() >= command_timeout)
        {
            if let Some(pending) = self.pending.take()
                && let Some(response) = pending.response
            {
                let _ = response.send(Err(DebugCommandError::Timeout));
            }
            self.debugger_error(
                "GDB command or asynchronous transition timed out".into(),
                DebugOutcome::CommandTimeout,
            );
        }
        if self
            .running_since
            .is_some_and(|began| began.elapsed() >= running_timeout)
        {
            self.outcome.get_or_insert(DebugOutcome::Timeout);
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.field(key)?.text()
}
fn path(value: &Value) -> Option<PathBuf> {
    let bytes = value.bytes()?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(Path::new(std::ffi::OsStr::from_bytes(bytes)).to_owned())
    }
    #[cfg(windows)]
    {
        Some(PathBuf::from(std::str::from_utf8(bytes).ok()?))
    }
}
fn quote_path(path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(mi::quote(path.as_os_str().as_bytes()))
    }
    #[cfg(windows)]
    {
        Ok(mi::quote(
            path.to_str()
                .ok_or_else(|| invalid("path is not valid Unicode"))?
                .replace('\\', "/")
                .as_bytes(),
        ))
    }
}
fn exit_code(value: &str) -> Option<i32> {
    if let Some(value) = value.strip_prefix("0x") {
        u32::from_str_radix(value, 16).ok().map(|code| code as i32)
    } else if value.starts_with('0') && value.len() > 1 {
        u32::from_str_radix(value, 8).ok().map(|code| code as i32)
    } else {
        value.parse().ok()
    }
}
fn malformed() -> DebugCommandError {
    DebugCommandError::Gdb("missing or invalid GDB response fields".into())
}
fn frame(value: &Value, fallback: Option<u32>) -> Result<StackFrame, DebugCommandError> {
    Ok(StackFrame {
        level: text(value, "level")
            .and_then(|n| n.parse().ok())
            .or(fallback)
            .ok_or_else(malformed)?,
        function: text(value, "func").map(str::to_owned),
        file: value
            .field("fullname")
            .or_else(|| value.field("file"))
            .and_then(path),
        line: text(value, "line").and_then(|n| n.parse().ok()),
        address: text(value, "addr").map(str::to_owned),
    })
}
fn parse_reply(kind: &Kind, class: &str, data: &Value) -> Result<Reply, DebugCommandError> {
    if kind.resumes() {
        return if class == "running" {
            Ok(Reply::Accepted)
        } else {
            Err(malformed())
        };
    }
    if class != "done" {
        return Err(malformed());
    }
    Ok(match kind {
        Kind::SetBreakpoint(..) => {
            let b = data.field("bkpt").ok_or_else(malformed)?;
            Reply::Breakpoint(Breakpoint {
                id: text(b, "number").ok_or_else(malformed)?.into(),
                file: b
                    .field("fullname")
                    .or_else(|| b.field("file"))
                    .and_then(path),
                line: text(b, "line").and_then(|n| n.parse().ok()),
            })
        }
        Kind::Stack => Reply::Stack(
            data.field("stack")
                .and_then(Value::list)
                .ok_or_else(malformed)?
                .iter()
                .map(|(_, v)| frame(v, None))
                .collect::<Result<_, _>>()?,
        ),
        Kind::Variables(_) => Reply::Variables(
            data.field("variables")
                .and_then(Value::list)
                .ok_or_else(malformed)?
                .iter()
                .map(|(_, v)| {
                    Ok(Variable {
                        name: text(v, "name").ok_or_else(malformed)?.into(),
                        value: text(v, "value").map(str::to_owned),
                        is_argument: text(v, "arg") == Some("1"),
                    })
                })
                .collect::<Result<_, DebugCommandError>>()?,
        ),
        _ => Reply::Accepted,
    })
}

fn retain(artifact: &BuildArtifact, errors: &mut Vec<String>) {
    artifact.retain();
    errors.push(
        "debug process/I/O cleanup could not be confirmed; artifact retained for manual cleanup"
            .into(),
    );
    for error in errors {
        artifact.cleanup_report().record(error);
    }
}
fn cleanup(
    child: &mut Child,
    processes: &Processes,
    transport: &mut Transport,
    errors: &mut Vec<String>,
) -> bool {
    let mut terminated = false;
    for _ in 0..2 {
        match processes.terminate() {
            Ok(()) => {
                terminated = true;
                break;
            }
            Err(e) => errors.push(format!("debug process cleanup: {e}")),
        }
    }
    let reaped = process::stop_and_reap(child, errors).is_some();
    let io_finished = transport.finish(errors);
    terminated && reaped && io_finished
}
fn close(directory: tempfile::TempDir, errors: &mut Vec<String>) {
    if let Err(error) = directory.close() {
        errors.push(format!("debug directory cleanup: {error}"));
    }
}

#[cfg(windows)]
fn gdb_directory(path: &Path) -> PathBuf {
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt, OsStringExt},
    };
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // GDB expands relative executables using a code-page cwd before launching.
    // Use the same directory's existing short name; no alias is created.
    // SAFETY: wide is NUL-terminated; a zero-sized output queries the size.
    let length = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) };
    if length == 0 {
        return path.to_owned();
    }
    let mut short = vec![0; length as usize];
    // SAFETY: short has the capacity passed to GetShortPathNameW.
    let written = unsafe { GetShortPathNameW(wide.as_ptr(), short.as_mut_ptr(), length) };
    if written == 0 || written >= length {
        return path.to_owned();
    }
    OsString::from_wide(&short[..written as usize]).into()
}

fn supervise(
    mut request: DebugRequest,
    control: Control,
    ready: Ready,
    result_sender: mpsc::SyncSender<DebugResult>,
) {
    let began = Instant::now();
    let mut engine = Engine::new(control);
    let prepare =
        || -> Result<(tempfile::TempDir, tempfile::TempDir, Capture, PathBuf), DebugStartError> {
            if [
                request.startup_timeout,
                request.command_timeout,
                request.running_timeout,
            ]
            .iter()
            .any(Duration::is_zero)
            {
                return Err(invalid("debug timeouts must be positive").into());
            }
            let gdb = build::absolute_file(&request.gdb)?;
            let working = runtime::prepare(
                &request.artifact,
                &request.workspace_root,
                &request.input_root,
                &request.input_files,
                "debug",
            )
            .map_err(|e| DebugStartError {
                error: e.error,
                cleanup_errors: e.cleanup_errors,
            })?;
            let io_dir = match tempfile::Builder::new()
                .prefix(".debug-io-")
                .tempdir_in(request.artifact.directory())
            {
                Ok(directory) => directory,
                Err(error) => {
                    let mut cleanup_errors = Vec::new();
                    close(working, &mut cleanup_errors);
                    return Err(DebugStartError {
                        error,
                        cleanup_errors,
                    });
                }
            };
            let setup = || -> io::Result<Capture> {
                fs::write(io_dir.path().join("stdin"), &request.stdin)?;
                for name in ["stdout", "stderr"] {
                    File::create(io_dir.path().join(name))?;
                }
                Ok(Capture {
                    stdout: File::open(io_dir.path().join("stdout"))?,
                    stderr: File::open(io_dir.path().join("stderr"))?,
                    delivered: 0,
                })
            };
            match setup() {
                Ok(capture) => Ok((working, io_dir, capture, gdb)),
                Err(error) => {
                    let mut cleanup_errors = Vec::new();
                    close(io_dir, &mut cleanup_errors);
                    close(working, &mut cleanup_errors);
                    Err(DebugStartError {
                        error,
                        cleanup_errors,
                    })
                }
            }
        };
    let (working, io_dir, mut capture, gdb) = match prepare() {
        Ok(value) => value,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    request.stdin.clear();
    request.stdin.shrink_to_fit();
    let mut command = Command::new(gdb);
    command
        .args([
            "--interpreter=mi3",
            "--quiet",
            "-nx",
            "-iex",
            "set auto-load off",
        ])
        .current_dir(working.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.current_dir(gdb_directory(working.path()));
    #[cfg(target_os = "linux")]
    command.env("SHELL", "/bin/sh");
    let mut cleanup_errors = Vec::new();
    let (mut child, processes) = match Processes::spawn(&mut command, &mut cleanup_errors) {
        Ok(value) => value,
        Err(error) => {
            drop(command);
            drop(capture);
            close(io_dir, &mut cleanup_errors);
            close(working, &mut cleanup_errors);
            let _ = ready.send(Err(DebugStartError {
                error,
                cleanup_errors,
            }));
            return;
        }
    };
    drop(command);
    let mut initialize = || -> io::Result<()> {
        engine.transport.start(&mut child, &request.artifact)?;
        for setting in [
            "-gdb-set pagination off",
            "-gdb-set confirm off",
            "-gdb-set non-stop off",
            "-gdb-set mi-async on",
        ] {
            engine.initialize(setting, began, request.startup_timeout)?;
        }
        #[cfg(target_os = "linux")]
        engine.initialize(
            "-gdb-set disable-randomization off",
            began,
            request.startup_timeout,
        )?;
        let features = engine.initialize("-list-features", began, request.startup_timeout)?;
        if !features
            .field("features")
            .and_then(Value::list)
            .is_some_and(|list| {
                list.iter()
                    .any(|(_, v)| v.text() == Some("exec-run-start-option"))
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "GDB does not support exec-run --start",
            ));
        }
        let auto_load = engine.initialize(
            "-gdb-show auto-load gdb-scripts",
            began,
            request.startup_timeout,
        )?;
        if text(&auto_load, "value") != Some("off") {
            return Err(io::Error::other("GDB auto-loading was not disabled"));
        }
        let mut load = b"-file-exec-and-symbols ".to_vec();
        #[cfg(windows)]
        // GDB opens executable paths using the system code page, not MI UTF-8.
        let executable: &Path =
            &Path::new("..").join(request.artifact.executable().file_name().unwrap());
        #[cfg(not(windows))]
        let executable = request.artifact.executable();
        load.extend(quote_path(executable)?);
        engine.initialize(load, began, request.startup_timeout)?;
        let name = io_dir
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| invalid("invalid private I/O directory"))?;
        // Only Core-generated ASCII names reach shell/native redirection parsing.
        engine.initialize(
            format!("-exec-arguments < ../{name}/stdin > ../{name}/stdout 2> ../{name}/stderr"),
            began,
            request.startup_timeout,
        )?;
        Ok(())
    };
    if let Err(error) = initialize() {
        let cleaned = cleanup(
            &mut child,
            &processes,
            &mut engine.transport,
            &mut cleanup_errors,
        );
        drop(capture);
        if cleaned {
            close(io_dir, &mut cleanup_errors);
            close(working, &mut cleanup_errors);
        } else {
            retain(&request.artifact, &mut cleanup_errors);
            let _ = io_dir.keep();
            let _ = working.keep();
        }
        let _ = ready.send(Err(DebugStartError {
            error,
            cleanup_errors,
        }));
        return;
    }
    engine.state(DebugState::Ready);
    engine.dispatch(Kind::Start, None, Instant::now());
    let working_directory = working.keep();
    if ready
        .send(Ok((request.artifact.clone(), working_directory.clone())))
        .is_err()
    {
        engine.control.stop.stop();
    }
    while engine.outcome.is_none() {
        capture.read(&mut engine, false);
        for _ in 0..64 {
            match engine.transport.records.try_recv() {
                Ok(record) => engine.record(record),
                Err(_) => break,
            }
        }
        if engine.outcome.is_some() {
            break;
        }
        if let Some(error) = engine.transport.fault() {
            engine.debugger_error(error, DebugOutcome::DebuggerError);
            break;
        }
        match processes.debugger_exited(&mut child) {
            Ok(true) => {
                engine.debugger_error(
                    "GDB exited before the session finished".into(),
                    DebugOutcome::DebuggerError,
                );
                break;
            }
            Err(error) => {
                engine.process_error(format!("GDB wait: {error}"));
                break;
            }
            Ok(false) => {}
        }
        if engine.control.stop.0.load(Ordering::Acquire) {
            engine.outcome.get_or_insert(DebugOutcome::Stopped);
            break;
        }
        engine.expire(request.command_timeout, request.running_timeout);
        engine.commands(request.command_timeout);
        thread::sleep(POLL);
    }
    if let Some(pending) = engine.pending.take()
        && let Some(response) = pending.response
    {
        let _ = response.send(Err(DebugCommandError::Finished));
    }
    while let Ok(op) = engine.control.commands.try_recv() {
        let _ = op.response.send(Err(DebugCommandError::Finished));
    }
    // Best effort orderly exit, followed by finite OS control even if GDB stops
    // responding. No command acceptance is interpreted as completed cleanup.
    if engine.transport.fault().is_none() {
        let token = engine.token();
        let _ = engine.transport.send(token, "-gdb-exit");
        let grace = Instant::now();
        while grace.elapsed() < Duration::from_millis(200) {
            if matches!(processes.debugger_exited(&mut child), Ok(true)) {
                break;
            }
            capture.read(&mut engine, false);
            thread::sleep(POLL);
        }
    }
    let cleaned = cleanup(
        &mut child,
        &processes,
        &mut engine.transport,
        &mut cleanup_errors,
    );
    capture.read(&mut engine, true);
    drop(capture);
    if cleaned {
        close(io_dir, &mut cleanup_errors);
    } else {
        retain(&request.artifact, &mut cleanup_errors);
        let _ = io_dir.keep();
    }
    for error in &cleanup_errors {
        request.artifact.cleanup_report().record(error);
    }
    engine.state(DebugState::Finished);
    engine.finish_event();
    let result = DebugResult {
        outcome: engine.outcome.unwrap_or(DebugOutcome::ProcessError),
        exit_code: engine.exit_code,
        process_errors: engine.errors,
        cleanup_errors,
        working_directory,
        artifact: request.artifact,
    };
    let _ = result_sender.send(result);
}
