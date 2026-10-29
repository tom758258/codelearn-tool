//! Single desktop run control. Core owns execution and process cleanup.
use crate::workspace::Result;
use codelearn_tool_core::run::{
    self, RunOutcome, RunRequest, RunResult, RunStartError, RunStopHandle,
};
use serde::Serialize;
use std::sync::{Mutex, MutexGuard};

#[derive(Clone, Debug, Serialize)]
pub struct RunOutput {
    pub outcome: Option<&'static str>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub start_error: Option<String>,
    pub process_errors: Vec<String>,
    pub cleanup_errors: Vec<String>,
    pub working_directory: Option<String>,
}

impl From<RunResult> for RunOutput {
    fn from(result: RunResult) -> Self {
        Self {
            outcome: Some(match result.outcome {
                RunOutcome::Exited => "Exited",
                RunOutcome::Stopped => "Stopped",
                RunOutcome::Timeout => "Timeout",
                RunOutcome::OutputLimit => "OutputLimit",
                RunOutcome::ProcessError => "ProcessError",
            }),
            exit_code: result.exit_code,
            stdout: String::from_utf8_lossy(&result.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
            start_error: None,
            process_errors: result.process_errors,
            cleanup_errors: result.cleanup_errors,
            working_directory: Some(result.working_directory.to_string_lossy().into_owned()),
        }
    }
}

impl From<RunStartError> for RunOutput {
    fn from(error: RunStartError) -> Self {
        Self {
            outcome: None,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            start_error: Some(error.to_string()),
            process_errors: Vec::new(),
            cleanup_errors: error.cleanup_errors,
            working_directory: None,
        }
    }
}

#[derive(Clone, Default, Serialize)]
pub struct RunStatus {
    pub active: bool,
    pub output: Option<RunOutput>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Control {
    status: RunStatus,
    stop_requested: bool,
    handle: Option<RunStopHandle>,
}

#[derive(Default)]
pub struct RunControl(Mutex<Control>);

impl RunControl {
    fn lock(&self) -> MutexGuard<'_, Control> {
        // Never abandon a live session because its short control lock was poisoned.
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub fn status(&self) -> RunStatus {
        self.lock().status.clone()
    }

    /// Returns whether a run accepted the request; completion still requires wait().
    pub fn stop(&self) -> bool {
        let mut control = self.lock();
        if !control.status.active {
            return false;
        }
        control.stop_requested = true;
        if let Some(handle) = &control.handle {
            handle.stop();
        }
        true
    }

    fn install(&self, handle: RunStopHandle) {
        let mut control = self.lock();
        if control.stop_requested {
            handle.stop();
        }
        control.handle = Some(handle);
    }

    /// The caller holds Desktop's operation guard until this entire method returns.
    pub fn execute(&self, prepare: impl FnOnce() -> Result<RunRequest>) -> Result<RunOutput> {
        {
            let mut control = self.lock();
            control.status = RunStatus {
                active: true,
                ..RunStatus::default()
            };
            control.stop_requested = false;
            control.handle = None;
        }
        struct Reset<'a>(&'a RunControl);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                let mut control = self.0.lock();
                if let Some(handle) = control.handle.take() {
                    handle.stop();
                }
                if control.status.output.is_none() && control.status.error.is_none() {
                    control.status.output = Some(RunOutput {
                        outcome: Some("ProcessError"),
                        exit_code: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        start_error: None,
                        process_errors: vec!["The run worker failed unexpectedly".into()],
                        cleanup_errors: vec!["Process cleanup could not be confirmed".into()],
                        working_directory: None,
                    });
                }
                control.status.active = false;
            }
        }
        let _reset = Reset(self);
        let result = prepare().map(|request| match run::start(request) {
            Ok(session) => {
                self.install(session.stop_handle());
                // No Desktop or control mutex is held here. The blocking command
                // owns the session even if its frontend caller disappears.
                RunOutput::from(session.wait())
            }
            Err(error) => RunOutput::from(error),
        });
        let mut control = self.lock();
        match &result {
            Ok(output) => control.status.output = Some(output.clone()),
            Err(error) => control.status.error = Some(error.clone()),
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_failure_keeps_cleanup_details_separate_and_releases_control() {
        let output = RunOutput::from(RunStartError {
            error: std::io::Error::other("synthetic startup failure"),
            cleanup_errors: vec!["synthetic cleanup failure".into()],
        });
        let value = serde_json::to_value(&output).unwrap();
        assert!(value["outcome"].is_null() && value["exit_code"].is_null());
        assert!(
            value["start_error"]
                .as_str()
                .unwrap()
                .contains("synthetic startup failure")
        );
        assert_eq!(value["cleanup_errors"][0], "synthetic cleanup failure");
        assert_eq!(value["process_errors"], serde_json::json!([]));

        let control = RunControl::default();
        let result = control.execute(|| {
            assert!(control.stop());
            assert!(control.stop());
            Err("synthetic preparation failure".into())
        });
        assert!(result.is_err());
        assert!(!control.stop());
        let status = control.status();
        assert!(!status.active);
        assert_eq!(
            status.error.as_deref(),
            Some("synthetic preparation failure")
        );
        assert!(status.output.is_none());
    }
}
