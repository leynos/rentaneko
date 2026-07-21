//! Throwaway Rust harness for the 1.1.1 compatibility checkpoint.

use std::{
    error::Error,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    process::{Child, Command},
    task::JoinHandle,
    time::timeout,
};

const RUNNER_PATH: &str = "tests/checkpoint_support/checkpoint_runner.ts";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CAPTURED_STDERR_BYTES: usize = 16 * 1024;
const STDERR_TRUNCATION_MARKER: &str = "<earlier stderr output truncated>\n";

type BoxError = Box<dyn Error + Send + Sync>;

/// Throwaway process guard for roadmap task 1.1.1.
///
/// This is deliberately not the managed `Simulator` handle. Delete or fold it
/// into the real runner when roadmap tasks 1.3.1 and 1.3.2 land.
pub struct ThrowawayServerGuard {
    child: Option<Child>,
    stderr_task: JoinHandle<()>,
    base_uri: String,
}

impl ThrowawayServerGuard {
    /// Returns the simulator base URI used by `octocrab`.
    #[must_use]
    pub fn base_uri(&self) -> &str { &self.base_uri }

    /// Gracefully stops the throwaway server and reaps its owned child process.
    ///
    /// # Errors
    ///
    /// Returns an error if the child cannot be reaped after the bounded
    /// graceful shutdown and forced-termination fallback.
    pub async fn shutdown(mut self) -> Result<(), BoxError> {
        let Some(mut child) = self.child.take() else {
            self.stderr_task.abort();
            return Ok(());
        };
        terminate_process_group(child.id());
        let graceful_shutdown = timeout(SHUTDOWN_TIMEOUT, child.wait()).await;
        let result = match graceful_shutdown {
            Ok(Ok(status)) => exit_status_result(status),
            Ok(Err(error)) => force_kill_and_reap(&mut child, error).await,
            Err(_) => force_kill_and_reap(&mut child, "graceful shutdown timed out").await,
        };
        self.stderr_task.abort();
        result
    }
}

impl Drop for ThrowawayServerGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            // Mirror the `force_kill_and_reap` fallback: after the graceful
            // process-group SIGTERM, force-kill the whole owned group so
            // descendants die too, then start the direct-child kill.
            terminate_process_group(child.id());
            force_kill_process_group(child);
            drop(child.start_kill());
        }
        self.stderr_task.abort();
    }
}

/// Starts the throwaway Simulacat Core process and waits for readiness.
///
/// # Errors
///
/// Returns an error if the Bun runner cannot be spawned, its stdio pipes are
/// unavailable, stderr capture cannot be initialized, the process exits before
/// reporting readiness, or the startup timeout elapses.
pub async fn start_throwaway_server() -> Result<ThrowawayServerGuard, BoxError> {
    let mut child = spawn_runner()?;
    let stdout = child_stdout(&mut child)?;
    let stderr = child_stderr(&mut child)?;
    let captured_stderr = Arc::new(Mutex::new(StderrCapture::default()));
    let stderr_task = tokio::spawn(capture_stderr(stderr, Arc::clone(&captured_stderr)));
    let mut guard = ThrowawayServerGuard {
        child: Some(child),
        stderr_task,
        base_uri: String::new(),
    };
    let port = wait_for_port(stdout, &captured_stderr).await?;
    guard.base_uri = format!("http://127.0.0.1:{port}");
    Ok(guard)
}

fn spawn_runner() -> Result<Child, BoxError> {
    let mut command = Command::new("bun");
    command
        .arg("run")
        .arg("--conditions")
        .arg("development")
        .arg(RUNNER_PATH)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    set_process_group(&mut command);
    command.spawn().map_err(|error| {
        Box::new(io::Error::new(
            error.kind(),
            format!("failed to spawn Bun runner {RUNNER_PATH}: {error}"),
        )) as BoxError
    })
}

fn child_stdout(child: &mut Child) -> Result<impl AsyncRead + Unpin + Send + 'static, BoxError> {
    child
        .stdout
        .take()
        .ok_or_else(|| "throwaway runner stdout was not piped".into())
}

fn child_stderr(child: &mut Child) -> Result<impl AsyncRead + Unpin + Send + 'static, BoxError> {
    child
        .stderr
        .take()
        .ok_or_else(|| "throwaway runner stderr was not piped".into())
}

async fn wait_for_port(
    stdout: impl AsyncRead + Unpin,
    captured_stderr: &Arc<Mutex<StderrCapture>>,
) -> Result<u16, BoxError> {
    match timeout(STARTUP_TIMEOUT, read_listening_port(stdout)).await {
        Ok(Ok(port)) => Ok(port),
        Ok(Err(error)) => {
            Err(format!("{error}; captured stderr: {}", stderr(captured_stderr)).into())
        }
        Err(_) => Err(format!(
            "timed out waiting for Simulacat Core: {}",
            stderr(captured_stderr)
        )
        .into()),
    }
}

async fn read_listening_port(stdout: impl AsyncRead + Unpin) -> Result<u16, BoxError> {
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        if let Some(port) = parse_listening_port(&line) {
            return Ok(port);
        }
        if is_error_event(&line) {
            return Err(format!("Simulacat Core startup error: {line}").into());
        }
    }
    Err("Simulacat Core exited before reporting readiness".into())
}

pub(crate) fn parse_listening_port(line: &str) -> Option<u16> {
    let value: Value = serde_json::from_str(line).ok()?;
    let version = value.get("version").and_then(Value::as_u64)?;
    let event = value.get("event").and_then(Value::as_str)?;
    let host = value.get("host").and_then(Value::as_str)?;
    let port = value.get("port").and_then(Value::as_u64)?;
    listening_port(version, event, host, port)
}

fn listening_port(version: u64, event: &str, host: &str, port: u64) -> Option<u16> {
    if is_supported_readiness(version, event) && is_loopback_endpoint(host, port) {
        u16::try_from(port).ok()
    } else {
        None
    }
}

fn is_supported_readiness(version: u64, event: &str) -> bool {
    version == 1 && event == "listening"
}

fn is_loopback_endpoint(host: &str, port: u64) -> bool {
    // The throwaway runner must emit this literal host so the Rust harness
    // never accidentally targets a non-loopback listener.
    host == "127.0.0.1" && port != 0
}

async fn capture_stderr(
    mut stderr: impl AsyncRead + Unpin,
    captured_stderr: Arc<Mutex<StderrCapture>>,
) {
    let mut buffer = [0_u8; 1024];
    while let Ok(bytes_read) = stderr.read(&mut buffer).await {
        if bytes_read == 0 {
            break;
        }
        let Some(output) = buffer.get(..bytes_read) else {
            break;
        };
        append_stderr(&captured_stderr, &String::from_utf8_lossy(output));
    }
}

fn is_error_event(line: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .is_ok_and(|value| value.get("event").and_then(Value::as_str) == Some("error"))
}

fn append_stderr(captured_stderr: &Arc<Mutex<StderrCapture>>, output: &str) {
    if let Ok(mut stderr) = captured_stderr.lock() {
        stderr.append(output);
    }
}

fn stderr(captured_stderr: &Arc<Mutex<StderrCapture>>) -> String {
    captured_stderr.lock().map_or_else(
        |_| String::from("<stderr unavailable>"),
        |stderr| stderr.as_str(),
    )
}

#[derive(Default)]
struct StderrCapture {
    output: String,
    was_truncated: bool,
}

impl StderrCapture {
    fn append(&mut self, output: &str) {
        self.output.push_str(output);
        self.truncate_to_capacity();
    }

    fn as_str(&self) -> String {
        if self.was_truncated {
            format!("{STDERR_TRUNCATION_MARKER}{}", self.output)
        } else {
            self.output.clone()
        }
    }

    fn truncate_to_capacity(&mut self) {
        let bytes_to_remove = self.output.len().saturating_sub(MAX_CAPTURED_STDERR_BYTES);
        if bytes_to_remove == 0 {
            return;
        }

        let start = first_char_boundary_at_or_after(&self.output, bytes_to_remove);
        self.output.drain(..start);
        self.was_truncated = true;
    }
}

const fn first_char_boundary_at_or_after(value: &str, mut index: usize) -> usize {
    while !value.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(unix)]
fn set_process_group(command: &mut Command) { command.process_group(0); }

#[cfg(not(unix))]
fn set_process_group(_command: &mut Command) {}

#[cfg(unix)]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};

#[cfg(unix)]
fn signal_process_group(maybe_child_id: Option<u32>, signal: Signal) {
    if let Some(pid) = maybe_child_id.and_then(process_group_pid) {
        // The group may already be gone; a failed signal is not actionable here.
        match kill(pid, signal) {
            Ok(()) | Err(_) => {}
        }
    }
}

#[cfg(unix)]
fn terminate_process_group(maybe_child_id: Option<u32>) {
    signal_process_group(maybe_child_id, Signal::SIGTERM);
}

#[cfg(unix)]
fn force_kill_process_group(child: &Child) { signal_process_group(child.id(), Signal::SIGKILL); }

#[cfg(not(unix))]
fn force_kill_process_group(_child: &Child) {}

/// Maps a reaped child's exit status onto the shutdown result.
///
/// A non-success status (non-zero exit code, or signal termination on Unix)
/// means the throwaway server failed to shut down cleanly, so surface it rather
/// than swallowing it.
fn exit_status_result(status: std::process::ExitStatus) -> Result<(), BoxError> {
    if status.success() {
        Ok(())
    } else {
        Err(Box::new(io::Error::other(format!(
            "throwaway server exited with failure status: {status}"
        ))) as BoxError)
    }
}

async fn force_kill_and_reap(
    child: &mut Child,
    graceful_shutdown_error: impl std::fmt::Display,
) -> Result<(), BoxError> {
    // SIGKILL the whole process group first so any grandchildren spawned by the
    // runner die too, not just the direct child reaped by `start_kill`.
    force_kill_process_group(child);
    child.start_kill()?;
    child.wait().await.map(|_| ()).map_err(|force_kill_error| {
        Box::new(io::Error::other(format!(
            "graceful shutdown failed: {graceful_shutdown_error}; force-kill reaping failed: \
             {force_kill_error}"
        ))) as BoxError
    })
}

#[cfg(unix)]
fn process_group_pid(process_group_id: u32) -> Option<Pid> {
    i32::try_from(process_group_id)
        .ok()
        .map(|pid| Pid::from_raw(-pid))
}

#[cfg(not(unix))]
fn terminate_process_group(_child_id: Option<u32>) {}

#[cfg(test)]
mod tests;
