use std::{
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

/// Keep this observer after dropping an artifact to inspect automatic cleanup errors.
#[derive(Clone, Debug, Default)]
pub struct CleanupReport(Arc<Mutex<Vec<String>>>);

impl CleanupReport {
    pub fn errors(&self) -> Vec<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn record(&self, error: impl ToString) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(error.to_string());
    }
}

#[derive(Debug)]
pub(crate) struct Directory {
    inner: Option<tempfile::TempDir>,
    retained: AtomicBool,
    pub report: CleanupReport,
}

impl Directory {
    pub fn new(root: &Path, report: CleanupReport) -> io::Result<Self> {
        Ok(Self {
            inner: Some(
                tempfile::Builder::new()
                    .prefix("codelearn-build-")
                    .tempdir_in(root)?,
            ),
            retained: AtomicBool::new(false),
            report,
        })
    }

    pub fn path(&self) -> &Path {
        self.inner.as_ref().unwrap().path()
    }

    pub fn retain(&self) {
        self.retained.store(true, Ordering::Relaxed);
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        if let Some(dir) = self.inner.take() {
            if *self.retained.get_mut() {
                // Preserve files only; TempDir and its owner are still released.
                let _ = dir.keep();
                return;
            }
            let path = dir.path().to_owned();
            if let Err(error) = dir.close() {
                self.report
                    .record(format!("cleanup {}: {error}", path.display()));
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};

    #[test]
    fn cleanup_failure_remains_observable_after_drop() {
        let root = tempfile::tempdir().unwrap();
        let report = CleanupReport::default();
        let directory = Directory::new(root.path(), report.clone()).unwrap();
        let lock = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(directory.path().join("locked"))
            .unwrap();
        drop(directory);
        assert!(!report.errors().is_empty());
        assert!(root.path().is_dir());
        drop(lock);
    }
}
