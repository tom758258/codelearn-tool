use codelearn_tool_core::{
    build::{self, BuildArtifact, BuildRequest, Source},
    debug::{
        self, DebugCommandError, DebugEvent, DebugOutcome, DebugRequest, DebugSession, DebugState,
        OutputStream, StopReason,
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::Receiver,
    thread,
    time::{Duration, Instant},
};

const SOURCE: &str = include_str!("fixtures/debug.c");
struct Fixture {
    _root: tempfile::TempDir,
    workspace: PathBuf,
    artifact: BuildArtifact,
    source: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace space & 資料");
        let temp = root.path().join("temp space & 資料");
        fs::create_dir_all(workspace.join("input")).unwrap();
        fs::create_dir(&temp).unwrap();
        fs::write(workspace.join("input/data.bin"), [0, 255, 128, 42]).unwrap();
        let gcc = std::env::var_os("CODELEARN_TEST_GCC")
            .expect("select an absolute GCC with CODELEARN_TEST_GCC");
        let mut built = build::compile(&BuildRequest::new(
            workspace.clone(),
            temp,
            gcc.into(),
            Source::Text {
                code: SOURCE.into(),
                logical_directory: None,
                additional_files: vec![],
            },
        ));
        assert!(built.success, "{built:?}");
        Self {
            _root: root,
            workspace,
            source: built.sources[0].clone(),
            artifact: built.artifact.take().unwrap(),
        }
    }
    fn request(&self, mode: &str) -> DebugRequest {
        fs::write(self.workspace.join("mode"), mode).unwrap();
        let gdb = std::env::var_os("CODELEARN_TEST_GDB")
            .expect("select an absolute GDB with CODELEARN_TEST_GDB");
        let mut request =
            DebugRequest::new(self.artifact.clone(), gdb.into(), self.workspace.clone());
        request.input_root = Some(".".into());
        request.input_files = vec!["mode".into(), "input/data.bin".into()];
        request
    }
    fn start(&self, mode: &str) -> (DebugSession, Receiver<DebugEvent>) {
        let mut session = debug::start(self.request(mode)).unwrap();
        let events = session.take_events().unwrap();
        let (reason, frame) = stopped(&events);
        assert_eq!(reason, StopReason::MainEntry);
        assert_eq!(frame.function.as_deref(), Some("main"));
        assert_eq!(
            fs::canonicalize(frame.file.unwrap()).unwrap(),
            fs::canonicalize(&self.source).unwrap()
        );
        assert!(frame.line.is_some());
        (session, events)
    }
}
fn line(marker: &str) -> u32 {
    (SOURCE.lines().position(|s| s.contains(marker)).unwrap() + 1) as u32
}
fn event(events: &Receiver<DebugEvent>) -> DebugEvent {
    events
        .recv_timeout(Duration::from_secs(15))
        .expect("debug event timed out")
}
fn stopped(events: &Receiver<DebugEvent>) -> (StopReason, debug::StackFrame) {
    loop {
        match event(events) {
            DebugEvent::Stopped { reason, frame, .. } => {
                return (reason, frame.expect("source frame"));
            }
            DebugEvent::Running => {}
            other => panic!("expected stop, received {other:?}"),
        }
    }
}
fn until(mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "condition timed out"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn finished(events: &Receiver<DebugEvent>) {
    loop {
        if matches!(event(events), DebugEvent::Finished { .. }) {
            break;
        }
    }
}
fn clean(session: DebugSession, outcome: DebugOutcome) -> debug::DebugResult {
    let result = session.wait();
    assert_eq!(result.outcome, outcome, "{result:?}");
    assert!(result.cleanup_errors.is_empty(), "{result:?}");
    result
}
fn pid(directory: &Path, name: &str) -> u32 {
    until(|| fs::read_to_string(directory.join(name)).is_ok_and(|s| !s.is_empty()));
    fs::read_to_string(directory.join(name))
        .unwrap()
        .parse()
        .unwrap()
}
fn freeze_or_kill(pid: u32, freeze: bool) {
    #[cfg(unix)]
    {
        // SAFETY: fixture reports its own live parent's PID while its session is held.
        assert_eq!(
            unsafe {
                libc::kill(
                    pid as i32,
                    if freeze { libc::SIGSTOP } else { libc::SIGKILL },
                )
            },
            0
        );
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::{Diagnostics::ToolHelp::*, Threading::*};
        if freeze {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            assert_ne!(snapshot, INVALID_HANDLE_VALUE);
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut more = Thread32First(snapshot, &mut entry);
            while more != 0 {
                if entry.th32OwnerProcessID == pid {
                    let handle = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    assert!(!handle.is_null());
                    assert_ne!(SuspendThread(handle), u32::MAX);
                    CloseHandle(handle);
                }
                more = Thread32Next(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
        } else {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            assert!(!handle.is_null());
            assert_ne!(TerminateProcess(handle, 1), 0);
            CloseHandle(handle);
        }
    }
}
fn inactive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        !Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::*};
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return true;
        }
        let mut code = 0;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok != 0 && code != 259
    }
}

#[test]
#[ignore = "requires explicitly selected real GCC and GDB"]
fn real_debug_operations_frames_and_pause() {
    let f = Fixture::new();
    let (session, events) = f.start("flow");
    let breakpoint = session
        .set_breakpoint(&f.source, line("inner-entry"))
        .unwrap();
    assert_eq!(
        fs::canonicalize(breakpoint.file.as_ref().unwrap()).unwrap(),
        fs::canonicalize(&f.source).unwrap()
    );
    assert_eq!(breakpoint.line, Some(line("inner-entry")));
    assert!(session.set_breakpoint(&f.source, 0).is_err());
    assert!(session.set_breakpoint(&f.source, 99999).is_err());
    session.continue_execution().unwrap();
    let (reason, frame) = stopped(&events);
    assert_eq!(reason, StopReason::Breakpoint);
    assert_eq!(frame.function.as_deref(), Some("inner"));
    assert_eq!(
        fs::canonicalize(frame.file.unwrap()).unwrap(),
        fs::canonicalize(&f.source).unwrap()
    );
    assert_eq!(frame.line, Some(line("inner-entry")));
    let stack = session.call_stack().unwrap();
    assert_eq!(stack[0].function.as_deref(), Some("inner"));
    assert_eq!(stack[1].function.as_deref(), Some("outer"));
    let variables = session.variables(0).unwrap();
    assert!(
        variables
            .iter()
            .any(|v| v.name == "value" && v.value.as_deref() == Some("7") && v.is_argument)
    );
    assert!(
        session
            .variables(1)
            .unwrap()
            .iter()
            .any(|v| v.name == "seed" && v.value.as_deref() == Some("7"))
    );
    assert!(session.variables(9999).is_err());
    session.remove_breakpoint(&breakpoint.id).unwrap();
    session.step_into().unwrap();
    assert_eq!(stopped(&events).0, StopReason::Step);
    assert!(
        session
            .variables(0)
            .unwrap()
            .iter()
            .any(|v| v.name == "adjusted" && v.value.as_deref() == Some("10"))
    );
    session.step_over().unwrap();
    assert_eq!(stopped(&events).0, StopReason::Step);
    session.step_out().unwrap();
    assert_eq!(stopped(&events).0, StopReason::Step);
    session.continue_execution().unwrap();
    until(|| session.working_directory().join("running").exists());
    assert!(matches!(
        session.call_stack(),
        Err(DebugCommandError::InvalidState(DebugState::Running))
    ));
    assert!(matches!(
        session.set_breakpoint(&f.source, line("inner-entry")),
        Err(DebugCommandError::InvalidState(DebugState::Running))
    ));
    assert_eq!(
        session.remove_breakpoint(&breakpoint.id),
        Err(DebugCommandError::InvalidState(DebugState::Running))
    );
    session.pause().unwrap();
    assert_eq!(stopped(&events).0, StopReason::Pause);
    session.continue_execution().unwrap();
    session.stop_handle().stop();
    finished(&events);
    let result = clean(session, DebugOutcome::Stopped);
    assert!(result.process_errors.is_empty(), "{result:?}");
}

#[test]
#[ignore = "requires explicitly selected real GCC and GDB"]
fn real_debug_bytes_limits_and_timeouts() {
    let f = Fixture::new();
    let mut request = f.request("io");
    let input: Vec<u8> = (0..=255).cycle().take(256 * 1024).collect();
    request.stdin = input.clone();
    let mut session = debug::start(request).unwrap();
    let events = session.take_events().unwrap();
    stopped(&events);
    session.continue_execution().unwrap();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    while stdout.len() < input.len() + 4 || stderr.len() < 4 {
        match event(&events) {
            DebugEvent::Running => {}
            DebugEvent::Output {
                stream: OutputStream::Stdout,
                bytes,
            } => stdout.extend(bytes),
            DebugEvent::Output {
                stream: OutputStream::Stderr,
                bytes,
            } => stderr.extend(bytes),
            other => panic!("unexpected I/O event {other:?}"),
        }
    }
    assert_eq!(session.state(), DebugState::Running); // flushed output precedes program exit
    let mut expected = input;
    expected.extend([0, 255, 128, 42]);
    assert_eq!(stdout, expected);
    assert_eq!(stderr, [255, 0, 128, 42]);
    fs::write(session.working_directory().join("finish"), b"done").unwrap();
    finished(&events);
    let result = clean(session, DebugOutcome::Exited);
    assert_eq!(result.exit_code, Some(7));
    assert!(result.process_errors.is_empty(), "{result:?}");

    let (session, events) = f.start("overflow");
    session.continue_execution().unwrap();
    let mut bytes = 0;
    loop {
        match event(&events) {
            DebugEvent::Output { bytes: chunk, .. } => bytes += chunk.len(),
            DebugEvent::Finished { .. } => break,
            _ => {}
        }
    }
    assert!(bytes > 0 && bytes <= debug::OUTPUT_LIMIT);
    clean(session, DebugOutcome::OutputLimit);

    let mut request = f.request("flow");
    request.running_timeout = Duration::from_millis(250);
    let mut session = debug::start(request).unwrap();
    let events = session.take_events().unwrap();
    stopped(&events);
    thread::sleep(Duration::from_millis(350));
    assert_eq!(session.state(), DebugState::Stopped);
    session.continue_execution().unwrap();
    until(|| session.working_directory().join("running").exists());
    session.pause().unwrap();
    stopped(&events);
    thread::sleep(Duration::from_millis(350));
    session.continue_execution().unwrap();
    finished(&events);
    clean(session, DebugOutcome::Timeout);

    let mut request = f.request("flow");
    request.command_timeout = Duration::from_millis(200);
    let mut session = debug::start(request).unwrap();
    let events = session.take_events().unwrap();
    stopped(&events);
    freeze_or_kill(pid(session.working_directory(), "debugger.pid"), true);
    assert_eq!(session.variables(0), Err(DebugCommandError::Timeout));
    finished(&events);
    clean(session, DebugOutcome::CommandTimeout);
}

#[test]
#[ignore = "requires explicitly selected real GCC and GDB"]
fn real_debug_start_failures_process_cleanup_and_artifact_lifetime() {
    let f = Fixture::new();
    // The constructor cannot finish until this caller releases it: start must
    // return a usable session before the actual main-entry stop.
    let mut session = debug::start(f.request("startup")).unwrap();
    let events = session.take_events().unwrap();
    until(|| session.working_directory().join("startup-wait").exists());
    assert!(!session.working_directory().join("passed-startup").exists());
    fs::write(session.working_directory().join("allow-main"), b"continue").unwrap();
    assert_eq!(stopped(&events).0, StopReason::MainEntry);
    session.stop();
    finished(&events);
    clean(session, DebugOutcome::Stopped);

    let mut invalid = f.request("flow");
    invalid.input_files.push("../escape".into());
    assert!(debug::start(invalid).is_err());
    let mut invalid = f.request("flow");
    invalid.running_timeout = Duration::ZERO;
    assert!(debug::start(invalid).is_err());
    let mut invalid = f.request("flow");
    invalid.startup_timeout = Duration::from_nanos(1);
    assert_eq!(
        debug::start(invalid).unwrap_err().error.kind(),
        std::io::ErrorKind::TimedOut
    );
    let mut invalid = f.request("flow");
    invalid.gdb = f.workspace.join("missing");
    assert!(debug::start(invalid).is_err());
    let mut invalid = f.request("flow");
    invalid.gdb = std::env::var_os("CODELEARN_TEST_GCC").unwrap().into();
    assert!(debug::start(invalid).is_err());

    let mut session = debug::start(f.request("premain")).unwrap();
    let events = session.take_events().unwrap();
    loop {
        let next = event(&events);
        assert!(!matches!(
            next,
            DebugEvent::Stopped {
                reason: StopReason::MainEntry,
                ..
            }
        ));
        if matches!(next, DebugEvent::Finished { .. }) {
            break;
        }
    }
    assert_eq!(clean(session, DebugOutcome::Exited).exit_code, Some(19));

    for action in ["stop", "gdb-kill", "drop"] {
        let (session, events) = f.start("descendant");
        let dir = session.working_directory().to_owned();
        session.continue_execution().unwrap();
        let child = pid(&dir, "child.pid");
        let inferior = pid(&dir, "inferior.pid");
        until(|| dir.join("heartbeat").exists());
        match action {
            "stop" => {
                session.stop();
                finished(&events);
                clean(session, DebugOutcome::Stopped);
            }
            "gdb-kill" => {
                freeze_or_kill(pid(&dir, "debugger.pid"), false);
                finished(&events);
                clean(session, DebugOutcome::DebuggerError);
            }
            _ => {
                let began = Instant::now();
                drop(session);
                assert!(began.elapsed() < Duration::from_secs(1));
                drop(events);
            }
        }
        until(|| inactive(child) && inactive(inferior));
        let before = fs::read(dir.join("heartbeat")).unwrap();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(before, fs::read(dir.join("heartbeat")).unwrap());
    }
    let exit_fixture = Fixture::new();
    let (session, events) = exit_fixture.start("descendant-exit");
    let dir = session.working_directory().to_owned();
    session
        .set_breakpoint(&exit_fixture.source, line("descendant-return"))
        .unwrap();
    session.continue_execution().unwrap();
    assert_eq!(stopped(&events).0, StopReason::Breakpoint);
    let child = pid(&dir, "child.pid");
    let inferior = pid(&dir, "inferior.pid");
    assert!(!inactive(child) && !inactive(inferior));
    assert!(dir.join("heartbeat").exists());
    // Main returns normally while the child continues its heartbeat loop.
    session.continue_execution().unwrap();
    let mut exited = false;
    loop {
        match event(&events) {
            DebugEvent::Exited { exit_code, signal } => {
                assert_eq!(exit_code, Some(0));
                assert!(signal.is_none());
                exited = true;
            }
            DebugEvent::Finished { outcome } => {
                assert_eq!(outcome, DebugOutcome::Exited);
                break;
            }
            DebugEvent::Running => {}
            other => panic!("unexpected natural-exit event {other:?}"),
        }
    }
    assert!(exited);
    let result = session.wait();
    assert_eq!(result.outcome, DebugOutcome::Exited);
    assert_eq!(result.exit_code, Some(0));
    assert!(result.process_errors.is_empty(), "{result:?}");
    assert!(inactive(inferior));
    let retained = !result.cleanup_errors.is_empty();
    #[cfg(windows)]
    assert!(!retained, "{result:?}");
    #[cfg(target_os = "linux")]
    if retained {
        match fs::read_to_string(format!("/proc/{child}/stat")) {
            Ok(stat) => {
                let state = stat.rsplit_once(')').unwrap().1.trim_start().as_bytes()[0];
                assert!(matches!(state, b'Z' | b'X'), "descendant still running");
            }
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
        }
        assert!(
            result
                .cleanup_errors
                .iter()
                .any(|e| e.contains("including exited processes awaiting host reaping"))
        );
        assert!(
            result
                .cleanup_errors
                .iter()
                .any(|e| e.contains("artifact retained"))
        );
        eprintln!(
            "Natural-exit child awaits host reaping; cleanup error and artifact retention verified"
        );
    }
    if !retained {
        assert!(inactive(child));
    }
    let before = fs::read(dir.join("heartbeat")).unwrap();
    thread::sleep(Duration::from_millis(100));
    assert_eq!(before, fs::read(dir.join("heartbeat")).unwrap());
    let artifact_dir = result.artifact.directory().to_owned();
    drop(exit_fixture.artifact);
    assert!(artifact_dir.exists());
    drop(result);
    if retained {
        assert!(artifact_dir.exists());
    } else {
        until(|| !artifact_dir.exists());
    }

    let (session, events) = f.start("overflow");
    session.continue_execution().unwrap();
    // No event reads: both backpressure and output limiting must terminate.
    thread::sleep(Duration::from_millis(300));
    let result = session.wait();
    assert!(matches!(
        result.outcome,
        DebugOutcome::OutputLimit | DebugOutcome::ProcessError
    ));
    assert!(result.cleanup_errors.is_empty(), "{result:?}");
    drop(events);
    let artifact_dir = f.artifact.directory().to_owned();
    let cleanup = result.artifact.clone();
    let Fixture {
        _root, artifact, ..
    } = f;
    drop(artifact);
    assert!(artifact_dir.exists());
    drop(result);
    assert!(artifact_dir.exists());
    drop(cleanup);
    until(|| !artifact_dir.exists());
}
