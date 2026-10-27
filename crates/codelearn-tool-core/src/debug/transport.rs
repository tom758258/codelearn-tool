//! One writer and bounded readers. No worker waits for the event consumer.
use super::mi::{self, MAX_RECORD, Record};
use crate::build::BuildArtifact;
use std::{
    io::{self, Read, Write},
    process::Child,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(5);
pub(super) struct Transport {
    pub records: mpsc::Receiver<Record>,
    writes: Option<mpsc::SyncSender<Vec<u8>>>,
    fault: Arc<Mutex<Option<String>>>,
    cancelled: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
}
impl Transport {
    pub fn new() -> Self {
        let (_, records) = mpsc::sync_channel(1);
        Self {
            records,
            writes: None,
            fault: Arc::default(),
            cancelled: Arc::default(),
            workers: Vec::new(),
        }
    }
    fn spawn(
        &mut self,
        name: &str,
        artifact: &BuildArtifact,
        work: impl FnOnce(&AtomicBool) -> io::Result<()> + Send + 'static,
    ) -> io::Result<()> {
        let cancelled = self.cancelled.clone();
        let fault = self.fault.clone();
        let artifact = artifact.clone();
        self.workers.push(
            thread::Builder::new()
                .name(format!("codelearn-mi-{name}"))
                .spawn(move || {
                    let _artifact = artifact;
                    if let Err(error) = work(&cancelled)
                        && !cancelled.load(Ordering::Acquire)
                    {
                        fault
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .get_or_insert(error.to_string());
                    }
                })?,
        );
        Ok(())
    }
    pub fn start(&mut self, child: &mut Child, artifact: &BuildArtifact) -> io::Result<()> {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing GDB stdin"))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing GDB stdout"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("missing GDB stderr"))?;
        #[cfg(unix)]
        {
            nonblocking(&stdin)?;
            nonblocking(&stdout)?;
            nonblocking(&stderr)?;
        }
        let (records, rx) = mpsc::sync_channel(64);
        self.records = rx;
        self.spawn("read", artifact, move |cancel| {
            let mut buffer = [0; 8192];
            let mut framer = mi::Framer::default();
            while !cancel.load(Ordering::Acquire) {
                match stdout.read(&mut buffer) {
                    Ok(0) => return Err(framer.eof()),
                    Ok(n) => framer.push(&buffer[..n], |record| {
                        records
                            .try_send(record)
                            .map_err(|_| io::Error::other("GDB MI queue overflow or disconnected"))
                    })?,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        })?;
        // Diagnostics never become student output. Drain in fixed chunks.
        self.spawn("diagnostics", artifact, move |cancel| {
            let mut buffer = [0; 4096];
            while !cancel.load(Ordering::Acquire) {
                match stderr.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        })?;
        let (writes, rx) = mpsc::sync_channel::<Vec<u8>>(1);
        self.writes = Some(writes);
        self.spawn("write", artifact, move |cancel| {
            while !cancel.load(Ordering::Acquire) {
                let bytes = match rx.recv_timeout(POLL) {
                    Ok(bytes) => bytes,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let mut remaining = bytes.as_slice();
                while !remaining.is_empty() && !cancel.load(Ordering::Acquire) {
                    match stdin.write(remaining) {
                        Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                        Ok(n) => remaining = &remaining[n..],
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(POLL),
                        Err(e) => return Err(e),
                    }
                }
            }
            Ok(())
        })
    }
    pub fn send(&self, token: u64, command: impl AsRef<[u8]>) -> io::Result<()> {
        let command = command.as_ref();
        if command.len() > MAX_RECORD - 32 || command.iter().any(|b| b"\n\r\0".contains(b)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "GDB command exceeds limit or contains a line break",
            ));
        }
        let mut bytes = token.to_string().into_bytes();
        bytes.extend_from_slice(command);
        bytes.push(b'\n');
        self.writes
            .as_ref()
            .ok_or_else(|| io::Error::other("GDB writer unavailable"))?
            .try_send(bytes)
            .map_err(|_| io::Error::other("GDB writer unavailable"))
    }
    pub fn fault(&self) -> Option<String> {
        self.fault.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn finish(&mut self, errors: &mut Vec<String>) -> bool {
        self.cancelled.store(true, Ordering::Release);
        self.writes.take();
        let began = Instant::now();
        while self.workers.iter().any(|t| !t.is_finished())
            && began.elapsed() < Duration::from_secs(5)
        {
            #[cfg(windows)]
            for worker in &self.workers {
                use std::os::windows::io::AsRawHandle;
                if !worker.is_finished() {
                    // SAFETY: the JoinHandle owns this live worker handle.
                    unsafe {
                        windows_sys::Win32::System::IO::CancelSynchronousIo(worker.as_raw_handle());
                    }
                }
            }
            thread::sleep(POLL);
        }
        let mut finished = true;
        for worker in self.workers.drain(..) {
            if !worker.is_finished() {
                errors.push("GDB I/O worker did not finish after cancellation".into());
                finished = false;
            } else if worker.join().is_err() {
                errors.push("GDB I/O worker panicked".into());
                finished = false;
            }
        }
        finished
    }
}
#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    // SAFETY: fcntl operates on the live owned pipe descriptor.
    unsafe {
        let fd = pipe.as_raw_fd();
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
