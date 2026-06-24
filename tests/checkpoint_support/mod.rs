//! Throwaway Rust harness for the 1.1.1 compatibility checkpoint.

use std::{
    error::Error,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{Child, Command},
    task::JoinHandle,
    time::timeout,
};

const RUNNER_PATH: &str = "tests/checkpoint_support/checkpoint_runner.ts";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

type BoxError = Box<dyn Error + Send + Sync>;

/// Throwaway process guard for roadmap task 1.1.1.
///
/// This is deliberately not the managed `Simulator` handle. Delete or fold it
/// into the real runner when roadmap tasks 1.3.1 and 1.3.2 land.
pub struct ThrowawayServerGuard {
    child: Child,
    stderr_task: JoinHandle<()>,
    base_uri: String,
}

impl ThrowawayServerGuard {
    /// Returns the simulator base URI used by `octocrab`.
    #[must_use]
    pub fn base_uri(&self) -> &str { &self.base_uri }
}

impl Drop for ThrowawayServerGuard {
    fn drop(&mut self) {
        terminate_process_group(self.child.id());
        drop(self.child.start_kill());
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
    let captured_stderr = Arc::new(Mutex::new(String::new()));
    let stderr_task = tokio::spawn(capture_stderr(stderr, Arc::clone(&captured_stderr)));
    let mut guard = ThrowawayServerGuard {
        child,
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
    captured_stderr: &Arc<Mutex<String>>,
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
    let event = value.get("event").and_then(Value::as_str)?;
    let host = value.get("host").and_then(Value::as_str)?;
    let port = value.get("port").and_then(Value::as_u64)?;
    listening_port(event, host, port)
}

fn listening_port(event: &str, host: &str, port: u64) -> Option<u16> {
    // The throwaway runner must emit this literal host so the Rust harness
    // never accidentally targets a non-loopback listener.
    if event == "listening" && host == "127.0.0.1" {
        u16::try_from(port).ok()
    } else {
        None
    }
}

async fn capture_stderr(stderr: impl AsyncRead + Unpin, captured_stderr: Arc<Mutex<String>>) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        append_stderr(&captured_stderr, &line);
    }
}

fn is_error_event(line: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .is_ok_and(|value| value.get("event").and_then(Value::as_str) == Some("error"))
}

fn append_stderr(captured_stderr: &Arc<Mutex<String>>, line: &str) {
    if let Ok(mut stderr) = captured_stderr.lock() {
        stderr.push_str(line);
        stderr.push('\n');
    }
}

fn stderr(captured_stderr: &Arc<Mutex<String>>) -> String {
    captured_stderr.lock().map_or_else(
        |_| String::from("<stderr unavailable>"),
        |stderr| stderr.clone(),
    )
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
fn terminate_process_group(maybe_child_id: Option<u32>) {
    if let Some(pid) = maybe_child_id.and_then(process_group_pid) {
        match kill(pid, Signal::SIGTERM) {
            Ok(()) | Err(_) => {}
        }
    }
}

#[cfg(unix)]
fn process_group_pid(process_group_id: u32) -> Option<Pid> {
    i32::try_from(process_group_id)
        .ok()
        .map(|pid| Pid::from_raw(-pid))
}

#[cfg(not(unix))]
fn terminate_process_group(_child_id: Option<u32>) {}
