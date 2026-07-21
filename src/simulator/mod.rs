//! The managed Simulacat Core process handle.
//!
//! [`Simulator::start`] is the single lifecycle authority (`rentaneko-design.md`
//! §§8-9). It owns the child process, its Unix process group, the stdin pipe,
//! the stdout/stderr capture tasks, the temporary configuration directory, the
//! base URI, and the seeded installation ID. Teardown is bounded and
//! deterministic; [`Drop`] is a best-effort last resort only.

mod config;
mod output;
mod process;
#[cfg(test)]
mod tests;

use std::{
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use output::{OutputCapture, ReadinessOutcome};
use tempfile::TempDir;
use tokio::{
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command},
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};

use crate::error::RentanekoError;

/// Relative path to the private Bun runner from the crate root.
const RUNNER_PATH: &str = "runner/simulacat_runner.ts";
/// Environment variable naming the runner configuration file.
const RUNNER_CONFIG_ENV: &str = "RENTANEKO_RUNNER_CONFIG";
/// Maximum time to wait for the runner to report readiness.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time to wait for graceful shutdown before forcing a group kill.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
/// Maximum time to wait for a capture task to drain after the child is reaped.
const TASK_JOIN_TIMEOUT: Duration = Duration::from_secs(2);

/// A managed, owned Simulacat Core process.
///
/// The handle owns every resource the runner needs and tears them all down
/// through [`Simulator::shutdown`] or, as a last resort, [`Drop`].
#[derive(Debug)]
pub struct Simulator {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout_task: Option<JoinHandle<()>>,
    stderr_task: Option<JoinHandle<()>>,
    stderr_capture: Arc<Mutex<OutputCapture>>,
    // Retained so the configuration file outlives the runner and is removed on
    // teardown; `None` for injected test commands that need no configuration.
    temp_dir: Option<TempDir>,
    base_uri: String,
    installation_id: u64,
    shutdown_timeout: Duration,
}

impl Simulator {
    /// Starts the managed runner and waits for its readiness event.
    ///
    /// # Errors
    ///
    /// Returns a [`RentanekoError`] if the configuration cannot be written, the
    /// runner cannot be spawned, its stdio pipes are unavailable, it exits or
    /// reports an error before readiness, or the startup timeout elapses.
    pub async fn start() -> Result<Self, RentanekoError> {
        let temp_dir = config::write_seed_config()?;
        let command = bun_command(&temp_dir);
        Self::start_with(command, Some(temp_dir), config::SEED_INSTALLATION_ID).await
    }

    /// Returns the loopback base URI the runner is listening on.
    #[must_use]
    pub fn base_uri(&self) -> &str { &self.base_uri }

    /// Returns the seeded installation ID.
    #[must_use]
    pub const fn installation_id(&self) -> u64 { self.installation_id }

    /// Gracefully stops the runner and reaps the owned process group.
    ///
    /// Closes stdin so the runner self-terminates on end-of-file, requests a
    /// graceful process-group stop, waits for a bounded interval, force-kills the
    /// whole owned group when graceful shutdown fails or times out, reaps the
    /// direct child, and joins the capture tasks.
    ///
    /// # Errors
    ///
    /// Returns [`RentanekoError::ShutdownFailed`] if the runner exits with a
    /// failure status or cannot be reaped after the forced-kill fallback.
    pub async fn shutdown(mut self) -> Result<(), RentanekoError> { self.teardown().await }

    /// Core startup path shared by [`Simulator::start`] and test injection.
    async fn start_with(
        mut command: Command,
        temp_dir: Option<TempDir>,
        installation_id: u64,
    ) -> Result<Self, RentanekoError> {
        configure_stdio(&mut command);
        process::set_process_group(&mut command);
        let mut child = spawn(command)?;
        let stdin = take_stdin(&mut child)?;
        let stdout = take_stdout(&mut child)?;
        let stderr = take_stderr(&mut child)?;

        let stderr_capture = Arc::new(Mutex::new(OutputCapture::default()));
        let stdout_capture = Arc::new(Mutex::new(OutputCapture::default()));
        let (readiness_tx, readiness_rx) = oneshot::channel();
        let stdout_task = tokio::spawn(output::pump_stdout(stdout, readiness_tx, stdout_capture));
        let stderr_task = tokio::spawn(output::pump_stderr(stderr, Arc::clone(&stderr_capture)));

        // The handle owns the child and tasks from here on, so cancelling this
        // future (dropping it during startup) runs `Drop` and cleans them up.
        let mut simulator = Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout_task: Some(stdout_task),
            stderr_task: Some(stderr_task),
            stderr_capture: Arc::clone(&stderr_capture),
            temp_dir,
            base_uri: String::new(),
            installation_id,
            shutdown_timeout: SHUTDOWN_TIMEOUT,
        };
        let port = await_readiness(readiness_rx, &stderr_capture).await?;
        simulator.base_uri = format!("http://127.0.0.1:{port}");
        Ok(simulator)
    }

    async fn teardown(&mut self) -> Result<(), RentanekoError> {
        let Some(mut child) = self.child.take() else {
            self.abort_tasks();
            return Ok(());
        };
        // Close stdin so the runner self-terminates on EOF, then ask the whole
        // owned group to stop gracefully.
        drop(self.stdin.take());
        process::terminate_process_group(child.id());
        let graceful = timeout(self.shutdown_timeout, child.wait()).await;
        let result = match graceful {
            Ok(Ok(status)) => process::exit_status_result(status),
            Ok(Err(error)) => process::force_kill_and_reap(&mut child, error).await,
            Err(_) => process::force_kill_and_reap(&mut child, "graceful shutdown timed out").await,
        };
        self.join_tasks().await;
        // The configuration file is no longer needed once the runner has stopped.
        drop(self.temp_dir.take());
        result.map_err(|error| self.shutdown_error_with_context(error))
    }

    /// Enriches a shutdown failure with the bounded captured stderr for
    /// diagnostics, leaving other errors untouched.
    fn shutdown_error_with_context(&self, error: RentanekoError) -> RentanekoError {
        match error {
            RentanekoError::ShutdownFailed { message } => RentanekoError::ShutdownFailed {
                message: format!(
                    "{message}; captured stderr: {}",
                    output::snapshot(&self.stderr_capture)
                ),
            },
            other => other,
        }
    }

    /// Overrides the graceful-shutdown timeout for deterministic teardown tests.
    #[cfg(test)]
    const fn set_shutdown_timeout(&mut self, shutdown_timeout: Duration) {
        self.shutdown_timeout = shutdown_timeout;
    }

    async fn join_tasks(&mut self) {
        join_task(self.stdout_task.take()).await;
        join_task(self.stderr_task.take()).await;
    }

    fn abort_tasks(&mut self) {
        if let Some(task) = self.stdout_task.take() {
            task.abort();
        }
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            // Best-effort last resort: close stdin, then guarantee the whole
            // owned group is terminated. This is not graceful shutdown and does
            // not reap asynchronously.
            drop(self.stdin.take());
            process::terminate_process_group(child.id());
            process::force_kill_process_group(child);
            drop(child.start_kill());
        }
        self.abort_tasks();
    }
}

fn bun_command(temp_dir: &TempDir) -> Command {
    let config_path = config::config_path(temp_dir);
    let mut command = Command::new("bun");
    command
        .arg("run")
        .arg("--conditions")
        .arg("development")
        .arg(RUNNER_PATH)
        .env(RUNNER_CONFIG_ENV, config_path);
    command
}

fn configure_stdio(command: &mut Command) {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
}

fn spawn(mut command: Command) -> Result<Child, RentanekoError> {
    command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            RentanekoError::BunUnavailable { source: error }
        } else {
            RentanekoError::RunnerSpawnFailed {
                message: format!("failed to spawn the runner: {error}"),
            }
        }
    })
}

fn take_stdin(child: &mut Child) -> Result<ChildStdin, RentanekoError> {
    child.stdin.take().ok_or_else(|| pipe_unavailable("stdin"))
}

fn take_stdout(child: &mut Child) -> Result<ChildStdout, RentanekoError> {
    child
        .stdout
        .take()
        .ok_or_else(|| pipe_unavailable("stdout"))
}

fn take_stderr(child: &mut Child) -> Result<ChildStderr, RentanekoError> {
    child
        .stderr
        .take()
        .ok_or_else(|| pipe_unavailable("stderr"))
}

fn pipe_unavailable(stream: &str) -> RentanekoError {
    RentanekoError::RunnerSpawnFailed {
        message: format!("runner {stream} was not piped"),
    }
}

async fn await_readiness(
    readiness_rx: oneshot::Receiver<ReadinessOutcome>,
    stderr_capture: &Arc<Mutex<OutputCapture>>,
) -> Result<u16, RentanekoError> {
    match timeout(STARTUP_TIMEOUT, readiness_rx).await {
        Ok(Ok(ReadinessOutcome::Port(port))) => Ok(port),
        Ok(Ok(ReadinessOutcome::ErrorEvent(message))) => {
            Err(RentanekoError::RunnerErrorEvent { message })
        }
        Ok(Ok(ReadinessOutcome::Malformed(line))) => {
            Err(RentanekoError::MalformedReadinessEvent { line })
        }
        Ok(Ok(ReadinessOutcome::Eof) | Err(_)) => Err(RentanekoError::RunnerExitedBeforeReady {
            stderr: output::snapshot(stderr_capture),
        }),
        Err(_) => Err(RentanekoError::ReadinessTimeout {
            stderr: output::snapshot(stderr_capture),
        }),
    }
}

async fn join_task(task: Option<JoinHandle<()>>) {
    let Some(handle) = task else {
        return;
    };
    // After the child is reaped both pipes are closed, so the pump tasks finish
    // on their own. Bound the wait and abort a wedged task so none can leak.
    let abort_handle = handle.abort_handle();
    if timeout(TASK_JOIN_TIMEOUT, handle).await.is_err() {
        abort_handle.abort();
    }
}
