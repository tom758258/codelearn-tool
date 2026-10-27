//! Debug-only control: GDB creates an inferior process group of its own on Linux.
//! An exclusive Linux session covers those groups without changing the host's
//! subreaper status. Windows retains the existing Job Object boundary.
#[cfg(windows)]
pub(super) struct Processes(crate::process::Group);

#[cfg(windows)]
impl Processes {
    pub fn spawn(
        command: &mut std::process::Command,
        errors: &mut Vec<String>,
    ) -> std::io::Result<(std::process::Child, Self)> {
        crate::process::spawn(command, errors).map(|(child, group)| (child, Self(group)))
    }
    pub fn debugger_exited(&self, child: &mut std::process::Child) -> std::io::Result<bool> {
        Ok(child.try_wait()?.is_some())
    }
    pub fn terminate(&self) -> std::io::Result<()> {
        self.0.terminate()
    }
}

#[cfg(target_os = "linux")]
pub(super) use linux::Processes;

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        fs::{self, File},
        io,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::process::CommandExt,
        },
        process::{Child, Command},
        thread,
        time::{Duration, Instant},
    };

    pub(crate) struct Processes {
        sid: i32,
        debugger: File,
    }
    fn pidfd(pid: i32) -> io::Result<File> {
        // SAFETY: pidfd_open returns a new descriptor, or -1 without ownership.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: this descriptor was just opened and has one owner.
        Ok(unsafe { File::from_raw_fd(fd as i32) })
    }
    fn signal(fd: &File) -> io::Result<()> {
        // SAFETY: a pidfd addresses the opened process identity, not a reused PID.
        if unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        } < 0
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        Ok(())
    }
    // state, session id, start time. The command name may contain ')' or spaces.
    fn identity(pid: i32) -> io::Result<(u8, i32, u64)> {
        let stat = fs::read(format!("/proc/{pid}/stat"))?;
        let end = stat
            .iter()
            .rposition(|&b| b == b')')
            .ok_or_else(|| io::Error::other("invalid process identity"))?;
        let suffix = std::str::from_utf8(&stat[end + 1..])
            .map_err(|_| io::Error::other("invalid process identity"))?;
        let fields: Vec<_> = suffix.split_whitespace().collect();
        let parse = || {
            Some((
                *fields.first()?.as_bytes().first()?,
                fields.get(3)?.parse().ok()?,
                fields.get(19)?.parse().ok()?,
            ))
        };
        parse().ok_or_else(|| io::Error::other("invalid process identity"))
    }
    impl Processes {
        pub fn spawn(command: &mut Command, errors: &mut Vec<String>) -> io::Result<(Child, Self)> {
            // SAFETY: setsid is async-signal-safe; no Rust allocation or locking
            // happens in the child between fork and exec.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
            let mut child = command.spawn()?;
            let sid = child.id() as i32;
            match pidfd(sid) {
                Ok(debugger) => Ok((child, Self { sid, debugger })),
                Err(error) => {
                    crate::process::stop_and_reap(&mut child, errors);
                    Err(error)
                }
            }
        }
        pub fn debugger_exited(&self, _: &mut Child) -> io::Result<bool> {
            let mut fd = libc::pollfd {
                fd: self.debugger.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: poll receives one live, correctly sized descriptor.
            let n = unsafe { libc::poll(&mut fd, 1, 0) };
            if n < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(n != 0)
        }
        fn terminate_members(&self, include_dead: bool) -> io::Result<usize> {
            let mut active = 0;
            for entry in fs::read_dir("/proc")? {
                let entry = entry?;
                let Some(pid) = entry
                    .file_name()
                    .to_str()
                    .and_then(|s| s.parse::<i32>().ok())
                else {
                    continue;
                };
                if pid == self.sid {
                    continue;
                }
                let before = match identity(pid) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                if before.1 != self.sid {
                    continue;
                }
                if matches!(before.0, b'Z' | b'X') {
                    active += usize::from(include_dead);
                    continue;
                }
                let handle = match pidfd(pid) {
                    Ok(handle) => handle,
                    Err(e) if e.raw_os_error() == Some(libc::ESRCH) => continue,
                    Err(e) => return Err(e),
                };
                // Revalidate membership after opening. Signal only through the
                // handle, so exit/PID reuse cannot target a replacement process.
                if identity(pid).is_ok_and(|after| {
                    after.1 == before.1 && after.2 == before.2 && !matches!(after.0, b'Z' | b'X')
                }) {
                    signal(&handle)?;
                    active += 1;
                }
            }
            Ok(active)
        }
        pub fn terminate(&self) -> io::Result<()> {
            let began = Instant::now();
            // Keep GDB alive briefly to reap its own inferior where possible.
            while self.terminate_members(false)? != 0 && began.elapsed() < Duration::from_secs(1) {
                thread::sleep(Duration::from_millis(10));
            }
            signal(&self.debugger)?;
            loop {
                if self.terminate_members(true)? == 0 {
                    return Ok(());
                }
                if began.elapsed() >= Duration::from_secs(3) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "debug session processes remain (including exited processes awaiting host reaping)",
                    ));
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
