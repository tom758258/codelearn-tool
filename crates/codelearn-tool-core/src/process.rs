use std::{
    fs::{self, File},
    io,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessFailure {
    Start(String),
    Timeout,
    Wait(String),
    Capture(String),
}

/// Raw bytes preserve diagnostics even when the compiler uses a non-UTF-8 locale.
#[derive(Debug, Default)]
pub struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub failure: Option<ProcessFailure>,
    pub cleanup_errors: Vec<String>,
}

pub(crate) fn execute(command: &mut Command, directory: &Path, timeout: Duration) -> ProcessOutput {
    let mut output = ProcessOutput::default();
    let started = Instant::now();
    let stdout = directory.join("stdout");
    let stderr = directory.join("stderr");
    let capture =
        || -> io::Result<(File, File)> { Ok((File::create(&stdout)?, File::create(&stderr)?)) };
    let (out, err) = match capture() {
        Ok(files) => files,
        Err(error) => {
            output.failure = Some(ProcessFailure::Capture(error.to_string()));
            return output;
        }
    };
    // Files avoid pipe deadlocks and keep all output without reader threads.
    command
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    match platform::spawn(command, &mut output.cleanup_errors) {
        Err(error) => output.failure = Some(ProcessFailure::Start(error.to_string())),
        Ok((mut child, group)) => {
            loop {
                if started.elapsed() >= timeout {
                    output.failure = Some(ProcessFailure::Timeout);
                    break;
                }
                match child.try_wait() {
                    Ok(Some(status)) => {
                        output.exit_code = status.code();
                        output.success = status.success();
                        break;
                    }
                    Ok(None) => {
                        thread::sleep(
                            Duration::from_millis(10)
                                .min(timeout.saturating_sub(started.elapsed())),
                        );
                    }
                    Err(error) => {
                        output.failure = Some(ProcessFailure::Wait(error.to_string()));
                        break;
                    }
                }
            }
            // Also stop stragglers after a normal compiler exit.
            if let Err(error) = group.terminate() {
                output.cleanup_errors.push(error.to_string());
            }
            if output.failure.is_some()
                && let Some(status) = stop_and_reap(&mut child, &mut output.cleanup_errors)
            {
                output.exit_code = status.code();
            }
        }
    }
    // Release Command's capture handles before reading or deleting the directory.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    for (path, bytes) in [(&stdout, &mut output.stdout), (&stderr, &mut output.stderr)] {
        match fs::read(path) {
            Ok(data) => *bytes = data,
            Err(error) => {
                output
                    .failure
                    .get_or_insert(ProcessFailure::Capture(error.to_string()));
            }
        }
    }
    output.success &= output.failure.is_none() && output.cleanup_errors.is_empty();
    output
}

fn stop_and_reap(child: &mut Child, cleanup: &mut Vec<String>) -> Option<ExitStatus> {
    if let Err(error) = child.kill()
        && !matches!(child.try_wait(), Ok(Some(_)))
    {
        cleanup.push(error.to_string());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                cleanup.push("compiler did not exit after termination".into());
                return None;
            }
            Err(error) => {
                cleanup.push(error.to_string());
                return None;
            }
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::{
        io,
        os::unix::process::CommandExt,
        process::{Child, Command},
    };

    pub struct Group(i32);

    pub fn spawn(command: &mut Command, _: &mut Vec<String>) -> io::Result<(Child, Group)> {
        let child = command.process_group(0).spawn()?;
        let group = Group(child.id() as i32);
        Ok((child, group))
    }

    impl Group {
        pub fn terminate(&self) -> io::Result<()> {
            // SAFETY: a negative child PID addresses only its new process group.
            if unsafe { libc::kill(-self.0, libc::SIGKILL) } == -1 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error);
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_fixture() {
        let Some(mode) = std::env::var_os("CODELEARN_PROCESS_FIXTURE") else {
            return;
        };
        let heartbeat = std::env::var_os("CODELEARN_HEARTBEAT").unwrap();
        if mode == "child" {
            for count in 0..3000 {
                fs::write(&heartbeat, count.to_string()).unwrap();
                thread::sleep(Duration::from_millis(10));
            }
        } else {
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "process::tests::compiler_fixture", "--nocapture"])
                .env("CODELEARN_PROCESS_FIXTURE", "child")
                .spawn()
                .unwrap();
            // Deliberately leave the descendant to the compiler supervisor.
            drop(child);
            println!("{}", "x".repeat(100_000));
            eprintln!("diagnostic marker");
            let deadline = Instant::now() + Duration::from_secs(5);
            while !Path::new(&heartbeat).is_file() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if mode == "timeout" {
                thread::sleep(Duration::from_secs(30));
            }
        }
    }

    #[test]
    fn timeout_and_normal_exit_stop_descendants_and_preserve_output() {
        let root = tempfile::tempdir().unwrap();
        for mode in ["timeout", "normal"] {
            let dir = tempfile::tempdir_in(root.path()).unwrap();
            let heartbeat = dir.path().join("heartbeat");
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "process::tests::compiler_fixture", "--nocapture"])
                .env("CODELEARN_PROCESS_FIXTURE", mode)
                .env("CODELEARN_HEARTBEAT", &heartbeat);
            let output = execute(&mut command, dir.path(), Duration::from_secs(2));
            if mode == "timeout" {
                assert_eq!(output.failure, Some(ProcessFailure::Timeout));
            } else {
                assert!(output.success, "{output:?}");
            }
            assert!(output.cleanup_errors.is_empty(), "{output:?}");
            assert!(output.stdout.len() > 100_000);
            assert!(String::from_utf8_lossy(&output.stderr).contains("diagnostic marker"));
            let before = fs::read(&heartbeat).expect("descendant started");
            thread::sleep(Duration::from_millis(150));
            assert_eq!(
                before,
                fs::read(&heartbeat).unwrap(),
                "descendant survived cleanup"
            );
        }
        let output = execute(
            &mut Command::new(root.path().join("missing-executable")),
            root.path(),
            Duration::from_secs(1),
        );
        assert!(matches!(output.failure, Some(ProcessFailure::Start(_))));
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        io,
        mem::{size_of, zeroed},
        os::windows::{
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
            process::CommandExt,
        },
        process::{Child, Command},
        ptr, thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
    };

    pub struct Group(OwnedHandle);

    fn checked(ok: i32) -> io::Result<()> {
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the API returned a new owned kernel handle.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    pub fn spawn(command: &mut Command, cleanup: &mut Vec<String>) -> io::Result<(Child, Group)> {
        // SAFETY: all API buffers have the documented size and live through each call.
        let group = unsafe {
            let job = owned(CreateJobObjectW(ptr::null(), ptr::null()))?;
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            checked(SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ))?;
            Group(job)
        };
        // The compiler cannot create descendants before assignment to the job.
        let mut child = command
            .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW)
            .spawn()?;
        let setup = || -> io::Result<()> {
            // SAFETY: handles remain owned; the child has not resumed yet.
            unsafe {
                checked(AssignProcessToJobObject(
                    group.0.as_raw_handle(),
                    child.as_raw_handle(),
                ))?;
                // Stable std does not expose the primary thread handle. A suspended
                // new process has one thread; locate it without using nightly APIs.
                let snapshot = owned(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0))?;
                let mut entry: THREADENTRY32 = zeroed();
                entry.dwSize = size_of::<THREADENTRY32>() as u32;
                checked(Thread32First(snapshot.as_raw_handle(), &mut entry))?;
                loop {
                    if entry.th32OwnerProcessID == child.id() {
                        let primary =
                            owned(OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID))?;
                        if ResumeThread(primary.as_raw_handle()) == u32::MAX {
                            return Err(io::Error::last_os_error());
                        }
                        return Ok(());
                    }
                    if Thread32Next(snapshot.as_raw_handle(), &mut entry) == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            "suspended compiler thread not found",
                        ));
                    }
                }
            }
        };
        if let Err(error) = setup() {
            if let Err(error) = group.terminate() {
                cleanup.push(error.to_string());
            }
            super::stop_and_reap(&mut child, cleanup);
            return Err(error);
        }
        Ok((child, group))
    }

    impl Group {
        pub fn terminate(&self) -> io::Result<()> {
            // SAFETY: the job handle is valid and the accounting buffer is sized.
            unsafe {
                checked(TerminateJobObject(self.0.as_raw_handle(), 1))?;
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = zeroed();
                    checked(QueryInformationJobObject(
                        self.0.as_raw_handle(),
                        JobObjectBasicAccountingInformation,
                        &mut info as *mut _ as _,
                        size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                        ptr::null_mut(),
                    ))?;
                    if info.ActiveProcesses == 0 {
                        return Ok(());
                    }
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "compiler job did not become empty",
                        ));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
}
