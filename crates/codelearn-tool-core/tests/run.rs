use codelearn_tool_core::{
    build::{self, BuildArtifact, BuildRequest, CleanupReport, Source},
    run::{self, RunOutcome, RunRequest, RunResult},
};
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

struct Fixture {
    root: tempfile::TempDir,
    workspace: PathBuf,
    temp: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace space & 資料");
        let temp = root.path().join("temp space & 資料");
        fs::create_dir_all(workspace.join("lesson/input")).unwrap();
        fs::create_dir(&temp).unwrap();
        fs::write(workspace.join("lesson/input/data.bin"), [0, 0xff, 0x80, 42]).unwrap();
        fs::write(workspace.join("lesson/unselected"), b"do not copy").unwrap();
        Self {
            root,
            workspace,
            temp,
        }
    }
    fn build(&self) -> (BuildArtifact, CleanupReport) {
        let gcc = std::env::var_os("CODELEARN_TEST_GCC")
            .expect("set CODELEARN_TEST_GCC to an existing absolute GCC path");
        let mut result = build::compile(&BuildRequest::new(
            self.workspace.clone(),
            self.temp.clone(),
            gcc.into(),
            Source::Text {
                code: include_str!("fixtures/run.c").into(),
                logical_directory: None,
                additional_files: vec![],
            },
        ));
        assert!(result.success, "{result:?}");
        (result.artifact.take().unwrap(), result.cleanup)
    }
    fn request(&self, artifact: &BuildArtifact, mode: &str) -> RunRequest {
        fs::write(self.workspace.join("lesson/mode"), mode).unwrap();
        let mut request = RunRequest::new(artifact.clone(), self.workspace.clone());
        request.input_root = Some("lesson".into());
        request.input_files = vec!["mode".into(), "input/data.bin".into()];
        request
    }
}
fn clean(result: &RunResult, outcome: RunOutcome) {
    assert_eq!(result.outcome, outcome, "{:?}", result.process_errors);
    assert!(
        result.process_errors.is_empty(),
        "{:?}",
        result.process_errors
    );
    assert!(
        result.cleanup_errors.is_empty(),
        "{:?}",
        result.cleanup_errors
    );
}
fn until(mut condition: impl FnMut() -> bool) {
    let started = Instant::now();
    while !condition() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "condition did not become true"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn run_directory(artifact: &BuildArtifact, existing: &[PathBuf]) -> PathBuf {
    fs::read_dir(artifact.directory())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.is_dir()
                && !existing.contains(p)
                && p.file_name().unwrap().to_string_lossy().starts_with("run-")
        })
        .unwrap()
}
fn descendant_stopped(directory: &Path) {
    let before = fs::read(directory.join("heartbeat")).expect("descendant started");
    thread::sleep(Duration::from_millis(100));
    assert_eq!(
        before,
        fs::read(directory.join("heartbeat")).unwrap(),
        "descendant survived cleanup"
    );
}
fn link_directory(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:CODELEARN_LINK -Target $env:CODELEARN_TARGET | Out-Null"])
            .env("CODELEARN_LINK", link).env("CODELEARN_TARGET", target).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
#[ignore = "requires explicitly selected real GCC via CODELEARN_TEST_GCC"]
fn real_run_bytes_inputs_paths_and_artifact_lifetime() {
    let f = Fixture::new();
    let (artifact, cleanup) = f.build();
    let build_dir = artifact.directory().to_owned();
    let input: Vec<u8> = (0..256).cycle().take(256 * 1024).map(|n| n as u8).collect();
    let mut first = f.request(&artifact, "basic");
    assert_eq!(first.timeout, Duration::from_secs(30));
    first.timeout = Duration::MAX;
    first.stdin = input.clone();
    let first = run::start(first).unwrap().wait();
    clean(&first, RunOutcome::Exited);
    assert_eq!(first.exit_code, Some(0));
    let mut expected = vec![0xff; 128 * 1024];
    expected.extend_from_slice(&input);
    expected.extend_from_slice(&[0, 0xff, 0x80, 42]);
    assert_eq!(first.stdout, expected);
    assert_eq!(first.stderr, vec![0xff; 128 * 1024]);
    assert_eq!(
        fs::read(first.working_directory.join("input/data.bin")).unwrap(),
        b"modified copy"
    );
    assert_eq!(
        fs::read(f.workspace.join("lesson/input/data.bin")).unwrap(),
        [0, 0xff, 0x80, 42]
    );
    let second = run::start(f.request(&artifact, "exit7")).unwrap().wait();
    clean(&second, RunOutcome::Exited);
    assert_eq!(second.exit_code, Some(7));
    assert!(second.stdout.ends_with(&[0, 0xff, 0x80, 42]));
    assert_ne!(first.working_directory, second.working_directory);
    let mut closed = f.request(&artifact, "close");
    closed.stdin = vec![0; 4 * 1024 * 1024];
    let closed = run::start(closed).unwrap().wait();
    clean(&closed, RunOutcome::Exited);
    assert_eq!(closed.exit_code, Some(7));
    drop(closed);

    let run_count = || {
        fs::read_dir(&build_dir)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().path().is_dir())
            .count()
    };
    let directory_count = run_count();
    let reject = |request| {
        let error = run::start(request).unwrap_err();
        assert!(error.cleanup_errors.is_empty(), "{error:?}");
        assert_eq!(run_count(), directory_count);
        assert!(first.working_directory.is_dir());
    };
    for files in [
        vec!["missing".into()],
        vec!["../unselected".into()],
        vec![f.workspace.join("lesson/mode")],
        vec!["input".into()],
        vec!["mode".into(), "mode".into()],
    ] {
        let mut request = f.request(&artifact, "basic");
        request.input_files = files;
        reject(request);
    }
    let mut invalid = f.request(&artifact, "basic");
    invalid.timeout = Duration::ZERO;
    reject(invalid);
    let mut invalid = f.request(&artifact, "basic");
    invalid.workspace_root = f.root.path().join("missing");
    reject(invalid);
    let mut invalid = f.request(&artifact, "basic");
    invalid.input_root = None;
    reject(invalid);
    let mut invalid = f.request(&artifact, "basic");
    invalid.input_root = Some(f.root.path().into());
    reject(invalid);
    link_directory(f.root.path(), &f.workspace.join("lesson/escape"));
    fs::write(f.root.path().join("outside.bin"), b"outside").unwrap();
    let mut invalid = f.request(&artifact, "basic");
    invalid.input_files = vec!["escape/outside.bin".into()];
    reject(invalid);
    // Even a source inside Workspace must remain inside the selected input root.
    link_directory(&f.workspace, &f.workspace.join("lesson/alias"));
    fs::write(f.workspace.join("other.bin"), b"outside input root").unwrap();
    let mut invalid = f.request(&artifact, "basic");
    invalid.input_files = vec!["alias/other.bin".into()];
    reject(invalid);
    #[cfg(windows)]
    {
        for files in [
            vec!["mode".into(), "MODE".into()],
            vec!["mode:stream".into()],
            vec!["NUL".into()],
        ] {
            let mut request = f.request(&artifact, "basic");
            request.input_files = files;
            reject(request);
        }
        fs::remove_dir(f.workspace.join("lesson/escape")).unwrap();
        fs::remove_dir(f.workspace.join("lesson/alias")).unwrap();
    }
    // Existing but non-executable artifact exercises spawn failure after copying.
    let executable = fs::read(artifact.executable()).unwrap();
    fs::write(artifact.executable(), b"invalid executable").unwrap();
    reject(f.request(&artifact, "basic"));
    fs::write(artifact.executable(), executable).unwrap();
    fs::remove_file(artifact.executable()).unwrap();
    reject(f.request(&artifact, "basic"));
    drop(artifact);
    assert!(build_dir.exists());
    drop(first);
    assert!(second.working_directory.is_dir());
    drop(second);
    until(|| !build_dir.exists());
    assert!(cleanup.errors().is_empty());
    assert!(f.workspace.is_dir() && f.temp.is_dir());
}

#[test]
#[ignore = "requires explicitly selected real GCC via CODELEARN_TEST_GCC"]
fn real_run_stop_timeout_drop_and_descendant_cleanup() {
    let f = Fixture::new();
    let (artifact, cleanup) = f.build();
    let mut request = f.request(&artifact, "wait");
    request.stdin = vec![0; 4 * 1024 * 1024];
    let started = Instant::now();
    let session = run::start(request).unwrap();
    let directory = run_directory(&artifact, &[]);
    until(|| directory.join("ready").exists());
    let stop = session.stop_handle();
    thread::spawn(move || stop.stop()).join().unwrap();
    let stopped = session.wait();
    clean(&stopped, RunOutcome::Stopped);
    assert!(started.elapsed() < Duration::from_secs(10));
    descendant_stopped(&stopped.working_directory);
    drop(stopped);

    let mut request = f.request(&artifact, "wait");
    request.timeout = Duration::from_secs(1);
    request.stdin = vec![0; 4 * 1024 * 1024];
    let timed = run::start(request).unwrap().wait();
    clean(&timed, RunOutcome::Timeout);
    descendant_stopped(&timed.working_directory);
    drop(timed);

    let session = run::start(f.request(&artifact, "descendant")).unwrap();
    let stop = session.stop_handle();
    // Waiting consumes the session without cancelling it; later stop requests
    // cannot replace the completed outcome.
    let natural = session.wait();
    stop.stop();
    clean(&natural, RunOutcome::Exited);
    assert_eq!(natural.exit_code, Some(0));
    descendant_stopped(&natural.working_directory);
    drop(natural);

    #[cfg(unix)]
    {
        // An intentionally escaped, self-expiring descendant keeps the pipes
        // open. Collection must return an explicit cleanup error, then this test
        // stops the escaped fixture itself; group supervision is not a sandbox.
        let escaped = run::start(f.request(&artifact, "escape")).unwrap().wait();
        let pid: i32 = fs::read_to_string(escaped.working_directory.join("child-pid"))
            .unwrap()
            .parse()
            .unwrap();
        // SAFETY: PID belongs to the deliberately escaped synthetic fixture.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
        assert_eq!(escaped.outcome, RunOutcome::Exited);
        assert!(
            escaped
                .cleanup_errors
                .iter()
                .any(|e| e.contains("stdio remained open"))
        );
        drop(escaped);
    }
    let mut request = f.request(&artifact, "wait");
    request.stdin = vec![0; 4 * 1024 * 1024];
    let existing = fs::read_dir(artifact.directory())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    let session = run::start(request).unwrap();
    let directory = run_directory(&artifact, &existing);
    until(|| directory.join("ready").exists());
    let build_dir = artifact.directory().to_owned();
    drop(artifact);
    let started = Instant::now();
    drop(session);
    assert!(started.elapsed() < Duration::from_secs(1));
    until(|| !build_dir.exists());
    assert!(cleanup.errors().is_empty());
}

#[test]
#[ignore = "requires explicitly selected real GCC via CODELEARN_TEST_GCC"]
fn real_run_shared_output_limit_and_exact_boundary() {
    let f = Fixture::new();
    let (artifact, _) = f.build();
    let exact = run::start(f.request(&artifact, "exact")).unwrap().wait();
    clean(&exact, RunOutcome::Exited);
    assert_eq!(exact.exit_code, Some(0));
    assert_eq!(exact.stdout.len(), 13 * 1024 * 1024);
    assert_eq!(exact.stderr.len(), 12 * 1024 * 1024);
    descendant_stopped(&exact.working_directory);
    drop(exact);
    let mut request = f.request(&artifact, "overflow");
    request.stdin = vec![0; 4 * 1024 * 1024];
    let overflow = run::start(request).unwrap().wait();
    clean(&overflow, RunOutcome::OutputLimit);
    assert!(!overflow.stdout.is_empty() && !overflow.stderr.is_empty());
    assert_eq!(
        overflow.stdout.len() + overflow.stderr.len(),
        run::OUTPUT_LIMIT
    );
    descendant_stopped(&overflow.working_directory);
}
