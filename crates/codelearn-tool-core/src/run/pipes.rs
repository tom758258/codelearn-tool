//! Run-only pipe workers; compiler capture continues to use its existing files.
use super::{BuildArtifact, OUTPUT_LIMIT, POLL, RunOutcome, State, lock};
use std::{
    io::{self, Read, Write},
    process::Child,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) struct Workers {
    threads: Vec<(&'static str, JoinHandle<()>)>,
    cancelled: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
    artifact: BuildArtifact,
}
impl Workers {
    pub fn new(state: Arc<Mutex<State>>, artifact: BuildArtifact) -> Self {
        Self {
            threads: Vec::new(),
            cancelled: Arc::new(AtomicBool::new(false)),
            state,
            artifact,
        }
    }
    fn spawn(
        &mut self,
        name: &'static str,
        work: impl FnOnce(&AtomicBool, &Mutex<State>) -> io::Result<()> + Send + 'static,
    ) -> io::Result<()> {
        let cancelled = self.cancelled.clone();
        let state = self.state.clone();
        let artifact = self.artifact.clone();
        let thread = thread::Builder::new()
            .name(format!("codelearn-{name}"))
            .spawn(move || {
                let _artifact = artifact;
                if let Err(error) = work(&cancelled, &state)
                    && !cancelled.load(Ordering::Acquire)
                {
                    lock(&state).error(format!("{name}: {error}"));
                }
            })?;
        self.threads.push((name, thread));
        Ok(())
    }
    pub fn start(&mut self, child: &mut Child, input: Vec<u8>) -> io::Result<()> {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing stdin pipe"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing stdout pipe"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("missing stderr pipe"))?;
        #[cfg(unix)]
        {
            nonblocking(&stdin)?;
            nonblocking(&stdout)?;
            nonblocking(&stderr)?;
        }
        self.spawn("stdout", move |cancel, state| {
            read(stdout, false, cancel, state)
        })?;
        self.spawn("stderr", move |cancel, state| {
            read(stderr, true, cancel, state)
        })?;
        self.spawn("stdin", move |cancel, _| {
            let mut remaining = input.as_slice();
            while !remaining.is_empty() && !cancel.load(Ordering::Acquire) {
                match stdin.write(&remaining[..remaining.len().min(8192)]) {
                    Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                    Ok(n) => remaining = &remaining[n..],
                    Err(e) if e.kind() == io::ErrorKind::BrokenPipe => break,
                    // Windows can report ERROR_NO_DATA when the reader closes.
                    #[cfg(windows)]
                    Err(e) if e.raw_os_error() == Some(232) => break,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
                    Err(e) => return Err(e),
                }
            }
            // Dropping the only parent writer delivers EOF, including empty input.
            Ok(())
        })
    }
    pub fn finish(&mut self, cleanup: &mut Vec<String>) {
        let started = Instant::now();
        // Managed processes have been terminated first. Allow buffered output to
        // drain, but escaped processes must not hold wait() hostage with a pipe.
        while self.threads.iter().any(|(_, t)| !t.is_finished())
            && started.elapsed() < Duration::from_secs(2)
        {
            thread::sleep(POLL);
        }
        if self.threads.iter().any(|(_, t)| !t.is_finished()) {
            cleanup.push("stdio remained open after process cleanup; cancelling I/O".into());
            self.cancelled.store(true, Ordering::Release);
        }
        let cancelling = Instant::now();
        while self.threads.iter().any(|(_, t)| !t.is_finished())
            && cancelling.elapsed() < Duration::from_secs(5)
        {
            #[cfg(windows)]
            for (_, thread) in &self.threads {
                if !thread.is_finished() {
                    use std::os::windows::io::AsRawHandle;
                    // SAFETY: JoinHandle owns this live thread handle. Retrying
                    // closes the race between cancellation and entering WriteFile/
                    // ReadFile. The worker checks cancelled before its next call.
                    if unsafe {
                        windows_sys::Win32::System::IO::CancelSynchronousIo(thread.as_raw_handle())
                    } == 0
                    {
                        let error = io::Error::last_os_error();
                        // ERROR_NOT_FOUND just means no synchronous I/O was pending.
                        if error.raw_os_error() != Some(1168) {
                            let message = format!("I/O cancellation: {error}");
                            if !cleanup.contains(&message) {
                                cleanup.push(message);
                            }
                        }
                    }
                }
            }
            thread::sleep(POLL);
        }
        for (name, thread) in self.threads.drain(..) {
            if !thread.is_finished() {
                // Exceptional OS failure: the worker still owns its pipe and an
                // artifact reference. Do not block the caller or delete its files.
                cleanup.push(format!(
                    "{name} worker did not finish after I/O cancellation"
                ));
            } else if thread.join().is_err() {
                lock(&self.state).error(format!("{name} worker panicked"));
            }
        }
    }
}

fn read(
    mut pipe: impl Read,
    stderr: bool,
    cancelled: &AtomicBool,
    state: &Mutex<State>,
) -> io::Result<()> {
    let mut buffer = [0; 8192];
    while !cancelled.load(Ordering::Acquire) {
        match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                let mut state = lock(state);
                if state.closed {
                    break;
                }
                let keep = n.min(OUTPUT_LIMIT - state.stdout.len() - state.stderr.len());
                let bytes = if stderr {
                    &mut state.stderr
                } else {
                    &mut state.stdout
                };
                bytes.extend_from_slice(&buffer[..keep]);
                if keep < n {
                    state.exceeded = true;
                    state.request(RunOutcome::OutputLimit);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: pipe owns this descriptor; flags affect only the parent's pipe end.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
