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
use rstest::rstest;
#[cfg(unix)]
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    process::{Child, Command},
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
#[rstest]
#[case::clean_exit(0, Ok(()))]
#[case::nonzero_exit(1 << 8, Err(Some("failure status")))]
#[case::signal_termination(9, Err(None))]
fn exit_status_result_maps_status(
    #[case] raw_status: i32,
    #[case] expected: std::result::Result<(), Option<&str>>,
) -> Result<()> {
    // Raw wait statuses: `0` is a clean exit, `1 << 8` a normal exit with code
    // 1, and `9` termination by SIGKILL.
    let result = exit_status_result(ExitStatus::from_raw(raw_status));
    match expected {
        Ok(()) => verify_that!(result, ok(anything())),
        Err(None) => verify_that!(result, err(anything())),
        Err(Some(substring)) => verify_that!(
            result
                .expect_err("non-success status must be an error")
                .to_string(),
            contains_substring(substring)
        ),
    }
}

#[cfg(unix)]
#[rstest]
#[case::negates_group_leader(4321, Some(-4321))]
#[case::rejects_overflow(u32::MAX, None)]
fn process_group_pid_maps_ids(#[case] input: u32, #[case] expected: Option<i32>) {
    // A negative pid targets the whole process group in `kill(2)`.
    assert_eq!(process_group_pid(input).map(Pid::as_raw), expected);
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

// Spawns a process-group leader that backgrounds a grandchild, returning the
// owned child (with `kill_on_drop` so the direct child is reaped, matching the
// real runner) and the grandchild PID it reported on stdout.
#[cfg(unix)]
async fn spawn_group_leader_with_grandchild() -> (Child, i32) {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("sleep 300 & printf '%s\\n' \"$!\"; wait")
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true);
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
    (child, grandchild_pid)
}

// Wraps `child` in a `ThrowawayServerGuard` without real stderr capture; the
// Drop-only path just needs a task handle to abort.
#[cfg(unix)]
fn guard_around(child: Child) -> super::ThrowawayServerGuard {
    super::ThrowawayServerGuard {
        child: Some(child),
        stderr_task: tokio::spawn(async {}),
        base_uri: String::new(),
    }
}

// Dropping the guard WITHOUT `shutdown()` must terminate the whole owned process
// group, including the backgrounded grandchild, not merely the direct child.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn drop_terminates_the_process_group() -> Result<()> {
    let (child, grandchild_pid) = spawn_group_leader_with_grandchild().await;
    let guard = guard_around(child);

    drop(guard);

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
