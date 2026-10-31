//! One desktop debugger. Core owns GDB, execution, and process cleanup.
use crate::workspace::{Result, UnitKey};
use codelearn_tool_core::debug::{
    self, DebugEvent, DebugSession, DebugState, DebugStopHandle, OutputStream,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard, mpsc},
    time::Duration,
};

#[derive(Clone, Deserialize, Serialize)]
pub struct BreakpointRequest {
    pub file: String,
    pub line: u32,
}

pub struct PreparedDebug {
    pub request: debug::DebugRequest,
    pub unit: UnitKey,
    pub files: Vec<(PathBuf, String, String)>,
    pub breakpoints: Vec<BreakpointRequest>,
}

#[derive(Clone, Serialize)]
pub struct Frame {
    level: u32,
    function: Option<String>,
    file: Option<String>,
    display_file: Option<String>,
    line: Option<u32>,
}

#[derive(Clone, Serialize)]
pub struct Variable {
    name: String,
    value: Option<String>,
    is_argument: bool,
}

#[derive(Clone, Serialize)]
pub struct Breakpoint {
    id: String,
    file: String,
    line: u32,
}

#[derive(Clone, Default, Serialize)]
pub struct DebugOutput {
    pub outcome: String,
    pub exit_code: Option<i32>,
    pub process_errors: Vec<String>,
    pub cleanup_errors: Vec<String>,
}

#[derive(Clone, Default, Serialize)]
pub struct DebugStatus {
    pub id: String,
    pub unit: Option<UnitKey>,
    pub active: bool,
    pub stopping: bool,
    pub state: String,
    pub frames: Vec<Frame>,
    pub variables: Vec<Variable>,
    pub breakpoints: Vec<Breakpoint>,
    pub stdout: String,
    pub stderr: String,
    pub events: Vec<String>,
    pub output: Option<DebugOutput>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Continue,
    StepInto,
    StepOver,
    StepOut,
    Pause,
    AddBreakpoint { file: String, line: u32 },
    RemoveBreakpoint { id: String },
    Variables { frame: u32 },
}

type Pending = (Action, mpsc::SyncSender<Result<()>>);

#[derive(Default)]
struct Control {
    view: DebugStatus,
    handle: Option<DebugStopHandle>,
    commands: Option<mpsc::SyncSender<Pending>>,
    stops: u64,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Default)]
pub struct DebugControl(Mutex<Control>);

impl DebugControl {
    fn lock(&self) -> MutexGuard<'_, Control> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> DebugStatus {
        let control = self.lock();
        let mut view = control.view.clone();
        view.stdout = String::from_utf8_lossy(&control.stdout).into_owned();
        view.stderr = String::from_utf8_lossy(&control.stderr).into_owned();
        view
    }

    pub fn stop(&self, id: Option<&str>) -> bool {
        let mut control = self.lock();
        if !control.view.active || id.is_some_and(|id| control.view.id != id) {
            return false;
        }
        control.view.stopping = true;
        control.view.state = "Stopping".into();
        control.view.frames.clear();
        control.view.variables.clear();
        if let Some(handle) = &control.handle {
            handle.stop();
        }
        true
    }

    pub fn command(&self, id: &str, action: Action) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let control = self.lock();
            if !control.view.active || control.view.stopping || control.view.id != id {
                return Err("This debug session has ended or is stopping".into());
            }
            control
                .commands
                .as_ref()
                .ok_or("The debugger is still starting")?
                .try_send((action, tx))
                .map_err(|_| "The debugger is busy; wait for the previous command")?;
        }
        rx.recv_timeout(Duration::from_secs(35)).map_err(|_| {
            "The debugger did not confirm the command; use Stop before continuing".to_string()
        })?
    }

    // Desktop's operation guard remains held until wait and event draining finish.
    pub fn execute(
        &self,
        id: String,
        prepare: impl FnOnce() -> Result<PreparedDebug>,
    ) -> Result<DebugOutput> {
        *self.lock() = Control {
            view: DebugStatus {
                id,
                active: true,
                state: "Starting".into(),
                ..DebugStatus::default()
            },
            ..Control::default()
        };
        struct Reset<'a>(&'a DebugControl);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                let mut control = self.0.lock();
                if let Some(handle) = control.handle.take() {
                    handle.stop();
                }
                control.commands = None;
                if control.view.output.is_none() && control.view.error.is_none() {
                    control.view.output = Some(DebugOutput {
                        outcome: "Debugger error".into(),
                        cleanup_errors: vec!["Debugger cleanup could not be confirmed".into()],
                        ..DebugOutput::default()
                    });
                }
                control.view.active = false;
                control.view.state = "Finished".into();
                control.view.frames.clear();
                control.view.variables.clear();
            }
        }
        let _reset = Reset(self);
        let result = self.execute_inner(prepare);
        let mut control = self.lock();
        match &result {
            Ok(output) => control.view.output = Some(output.clone()),
            Err(error) => control.view.error = Some(error.clone()),
        }
        result
    }

    fn execute_inner(
        &self,
        prepare: impl FnOnce() -> Result<PreparedDebug>,
    ) -> Result<DebugOutput> {
        let prepared = prepare()?;
        self.lock().view.unit = Some(prepared.unit);
        if self.lock().view.stopping {
            return Err("Debugging was cancelled before startup".into());
        }
        let mut session = match debug::start(prepared.request) {
            Ok(session) => session,
            Err(error) => {
                return Ok(DebugOutput {
                    outcome: "Could not start debugger".into(),
                    process_errors: vec![error.to_string()],
                    cleanup_errors: error.cleanup_errors,
                    ..DebugOutput::default()
                });
            }
        };
        let events = session
            .take_events()
            .ok_or("Cannot receive debugger events")?;
        let (tx, commands) = mpsc::sync_channel(1);
        {
            let mut control = self.lock();
            let handle = session.stop_handle();
            if control.view.stopping {
                handle.stop();
            }
            control.handle = Some(handle);
            control.commands = Some(tx);
        }
        let result = std::thread::scope(|scope| {
            // Always consume Core's bounded queue, including while commands block.
            let consumer = scope.spawn(|| {
                for event in events {
                    self.event(event, &prepared.files);
                }
            });
            while matches!(
                session.state(),
                DebugState::Starting | DebugState::Ready | DebugState::Running
            ) {
                std::thread::sleep(Duration::from_millis(5));
            }
            for point in prepared.breakpoints {
                if session.state() != DebugState::Stopped || self.lock().view.stopping {
                    break;
                }
                if let Err(error) = self.action(
                    &session,
                    Action::AddBreakpoint {
                        file: point.file,
                        line: point.line,
                    },
                    &prepared.files,
                ) {
                    self.note(error);
                    session.stop();
                    break;
                }
            }
            let mut inspected = 0;
            while session.state() != DebugState::Finished {
                let (stops, stopping) = {
                    let c = self.lock();
                    (c.stops, c.view.stopping)
                };
                if !stopping && stops != inspected && session.state() == DebugState::Stopped {
                    inspected = stops;
                    if let Err(error) = self.inspect(&session, 0, &prepared.files, stops) {
                        self.note(error);
                    }
                }
                match commands.recv_timeout(Duration::from_millis(10)) {
                    Ok((action, reply)) => {
                        let result = if self.lock().view.stopping {
                            Err("The debugger is stopping".into())
                        } else {
                            self.action(&session, action, &prepared.files)
                        };
                        let _ = reply.send(result);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                    Err(mpsc::RecvTimeoutError::Disconnected) => session.stop(),
                }
            }
            let result = session.wait();
            if consumer.join().is_err() {
                self.note("The debugger event reader stopped unexpectedly".into());
            }
            result
        });
        Ok(DebugOutput {
            outcome: match result.outcome {
                debug::DebugOutcome::Exited => "Exited",
                debug::DebugOutcome::Stopped => "Stopped",
                debug::DebugOutcome::Timeout => "Time limit reached",
                debug::DebugOutcome::CommandTimeout => "Debugger command timed out",
                debug::DebugOutcome::OutputLimit => "Output limit reached",
                debug::DebugOutcome::DebuggerError => "Debugger error",
                debug::DebugOutcome::ProcessError => "Process error",
            }
            .into(),
            exit_code: result.exit_code,
            process_errors: result.process_errors,
            cleanup_errors: result.cleanup_errors,
        })
    }

    fn note(&self, text: String) {
        let mut control = self.lock();
        if control.view.events.len() == 100 {
            control.view.events.remove(0);
        }
        control.view.events.push(text);
    }

    fn event(&self, event: DebugEvent, files: &[(PathBuf, String, String)]) {
        let mut control = self.lock();
        let message = match event {
            DebugEvent::Output { stream, bytes } => {
                match stream {
                    OutputStream::Stdout => control.stdout.extend(bytes),
                    OutputStream::Stderr => control.stderr.extend(bytes),
                }
                return;
            }
            DebugEvent::Running => {
                if !control.view.stopping {
                    control.view.state = "Running".into();
                }
                control.view.frames.clear();
                control.view.variables.clear();
                "Program is running".into()
            }
            DebugEvent::Stopped {
                reason,
                frame,
                signal,
                ..
            } => {
                if !control.view.stopping {
                    control.view.state = "Paused".into();
                    control.view.frames = frame.iter().map(|f| frame_view(f, files)).collect();
                    control.view.variables.clear();
                    control.stops += 1;
                }
                let reason = match reason {
                    debug::StopReason::MainEntry => "program entry",
                    debug::StopReason::Breakpoint => "breakpoint",
                    debug::StopReason::Step => "step",
                    debug::StopReason::Pause => "pause request",
                    debug::StopReason::Signal => "signal",
                    debug::StopReason::Other => "debugger stop",
                };
                format!(
                    "Paused at {reason}{}",
                    signal.map(|s| format!(": {s}")).unwrap_or_default()
                )
            }
            DebugEvent::Exited { exit_code, signal } => {
                control.view.state = "Finishing".into();
                control.view.frames.clear();
                control.view.variables.clear();
                format!(
                    "Program exited{}{}",
                    exit_code
                        .map(|code| format!(" with code {code}"))
                        .unwrap_or_default(),
                    signal.map(|s| format!(": {s}")).unwrap_or_default()
                )
            }
            DebugEvent::DebuggerError(error) => error,
            DebugEvent::Finished { .. } => {
                control.view.state = "Finishing".into();
                control.view.frames.clear();
                control.view.variables.clear();
                "Waiting for debugger cleanup".into()
            }
        };
        drop(control);
        self.note(message);
    }

    fn inspect(
        &self,
        session: &DebugSession,
        frame: u32,
        files: &[(PathBuf, String, String)],
        stop: u64,
    ) -> Result<()> {
        let frames = session.call_stack().map_err(command_error)?;
        let variables = session.variables(frame).map_err(command_error)?;
        let mut control = self.lock();
        if !control.view.stopping && control.stops == stop && session.state() == DebugState::Stopped
        {
            control.view.frames = frames.iter().map(|f| frame_view(f, files)).collect();
            control.view.variables = variables
                .into_iter()
                .map(|v| Variable {
                    name: v.name,
                    value: v.value,
                    is_argument: v.is_argument,
                })
                .collect();
        }
        Ok(())
    }

    fn action(
        &self,
        session: &DebugSession,
        action: Action,
        files: &[(PathBuf, String, String)],
    ) -> Result<()> {
        match action {
            Action::AddBreakpoint { file, line } => {
                let (path, _, _) = files
                    .iter()
                    .find(|(_, key, _)| key == &file)
                    .ok_or("Breakpoints must belong to this exercise or project")?;
                let point = session.set_breakpoint(path, line).map_err(command_error)?;
                self.lock().view.breakpoints.push(Breakpoint {
                    id: point.id,
                    file,
                    line: point.line.unwrap_or(line),
                });
            }
            Action::RemoveBreakpoint { id } => {
                if !self.lock().view.breakpoints.iter().any(|b| b.id == id) {
                    return Err("This breakpoint is no longer available".into());
                }
                session.remove_breakpoint(&id).map_err(command_error)?;
                self.lock().view.breakpoints.retain(|b| b.id != id);
            }
            Action::Variables { frame } => {
                let stop = self.lock().stops;
                self.inspect(session, frame, files, stop)?;
            }
            action => {
                match action {
                    Action::Continue => session.continue_execution(),
                    Action::StepInto => session.step_into(),
                    Action::StepOver => session.step_over(),
                    Action::StepOut => session.step_out(),
                    Action::Pause => session.pause(),
                    _ => unreachable!(),
                }
                .map_err(command_error)?;
                let mut control = self.lock();
                if session.state() == DebugState::Running && !control.view.stopping {
                    control.view.state = "Running".into();
                    control.view.frames.clear();
                    control.view.variables.clear();
                }
            }
        }
        Ok(())
    }
}

fn frame_view(frame: &debug::StackFrame, files: &[(PathBuf, String, String)]) -> Frame {
    let matched = frame.file.as_ref().and_then(|path| {
        files
            .iter()
            .find(|(known, _, _)| same_file::is_same_file(path, known).unwrap_or(false))
    });
    Frame {
        level: frame.level,
        function: frame.function.clone(),
        file: matched.map(|(_, key, _)| key.clone()),
        display_file: matched.map(|(_, _, name)| name.clone()).or_else(|| {
            frame
                .file
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|p| p.to_string_lossy().into_owned())
        }),
        line: frame.line,
    }
}

fn command_error(error: debug::DebugCommandError) -> String {
    match error {
        debug::DebugCommandError::InvalidState(_) => {
            "Wait for the program to pause before using this command".into()
        }
        debug::DebugCommandError::InvalidInput(reason) | debug::DebugCommandError::Gdb(reason) => {
            reason
        }
        debug::DebugCommandError::Busy => "The debugger is busy".into(),
        debug::DebugCommandError::Timeout => "The debugger command timed out; stopping".into(),
        debug::DebugCommandError::Finished => "The debugger has finished".into(),
    }
}
