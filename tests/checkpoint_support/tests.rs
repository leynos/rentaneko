//! Tests for bounded checkpoint diagnostics and lifecycle teardown.
//!
//! The teardown tests are Unix-only: they depend on process-group signalling
//! and raw `ExitStatus` construction that have no portable form, so each such
//! item carries its own `#[cfg(unix)]` guard to keep the test module flat.

#[cfg(unix)]
use std::{os::unix::process::ExitStatusExt as _, process::ExitStatus};

use googletest::prelude::*;
#[cfg(unix)]
use nix::{sys::signal::kill, unistd::Pid};
#[cfg(unix)]
use pretty_assertions::assert_eq;
#[cfg(unix)]
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    process::Command,
    time::{Duration, Instant, sleep},
};

use super::{MAX_CAPTURED_STDERR_BYTES, STDERR_TRUNCATION_MARKER, StderrCapture};
#[cfg(unix)]
use super::{exit_status_result, force_kill_and_reap, process_group_pid, set_process_group};

#[test]
fn stderr_capture_retains_only_the_latest_output() -> Result<()> {
    let mut capture = StderrCapture::default();
    capture.append(&"discarded".repeat(MAX_CAPTURED_STDERR_BYTES));
    capture.append("retained");

    let output = capture.as_str();
    verify_that!(output, starts_with(STDERR_TRUNCATION_MARKER))?;
    verify_that!(output, ends_with("retained"))?;
    verify_that!(
        output.len(),
        le(STDERR_TRUNCATION_MARKER.len() + MAX_CAPTURED_STDERR_BYTES)
    )
}

#[cfg(unix)]
#[test]
fn exit_status_result_accepts_clean_exit() -> Result<()> {
    verify_that!(exit_status_result(ExitStatus::from_raw(0)), ok(anything()))
}

#[cfg(unix)]
#[test]
fn exit_status_result_rejects_nonzero_exit() -> Result<()> {
    // A raw wait status of `1 << 8` encodes a normal exit with code 1.
    let error = exit_status_result(ExitStatus::from_raw(1 << 8))
        .expect_err("non-zero exit must be an error");
    verify_that!(error.to_string(), contains_substring("failure status"))
}

#[cfg(unix)]
#[test]
fn exit_status_result_rejects_signal_termination() -> Result<()> {
    // A raw wait status of `9` encodes termination by SIGKILL.
    verify_that!(exit_status_result(ExitStatus::from_raw(9)), err(anything()))
}

#[cfg(unix)]
#[test]
fn process_group_pid_negates_the_group_leader() {
    let pid = process_group_pid(4321).expect("a small pid fits in i32");
    // A negative pid targets the whole process group in `kill(2)`.
    assert_eq!(pid.as_raw(), -4321);
}

#[cfg(unix)]
#[test]
fn process_group_pid_rejects_overflowing_ids() -> Result<()> {
    verify_that!(process_group_pid(u32::MAX), none())
}

// Spawns a shell that backgrounds a grandchild in the same process group, then
// asserts the forced-kill fallback reaps the direct child and terminates the
// grandchild too, proving the group is targeted.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn force_kill_and_reap_terminates_the_process_group() -> Result<()> {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("sleep 300 & printf '%s\\n' \"$!\"; wait")
        .stdout(std::process::Stdio::piped());
    set_process_group(&mut command);
    let mut child = command.spawn().expect("spawn sh runner");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    let pid_line = lines
        .next_line()
        .await
        .expect("read grandchild pid line")
        .expect("grandchild pid line present");
    let grandchild_pid: i32 = pid_line.trim().parse().expect("parse grandchild pid");

    force_kill_and_reap(&mut child, "test-forced-shutdown")
        .await
        .expect("force kill and reap succeeds");

    verify_that!(wait_until_process_absent(grandchild_pid).await, eq(true))
}

// `kill(pid, None)` probes existence without delivering a signal.
#[cfg(unix)]
fn process_is_running(target: Pid) -> bool { kill(target, None).is_ok() }

// Polls for the grandchild's disappearance; once its parent shell dies it is
// reparented to init and reaped, so existence converges to false.
#[cfg(unix)]
async fn wait_until_process_absent(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    let target = Pid::from_raw(pid);
    while process_is_running(target) {
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(20)).await;
    }
    true
}
