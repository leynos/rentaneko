//! Tests for bounded checkpoint diagnostics and lifecycle teardown.
//!
//! The teardown tests are Unix-only: they depend on process-group signalling
//! and raw `ExitStatus` construction that have no portable form, so each such
//! item carries its own `#[cfg(unix)]` guard to keep the test module flat.

use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::{os::unix::process::ExitStatusExt as _, process::ExitStatus};

use googletest::prelude::*;
#[cfg(unix)]
use nix::{sys::signal::kill, unistd::Pid};
#[cfg(unix)]
use pretty_assertions::assert_eq;
#[cfg(unix)]
use rstest::rstest;
use tokio::io::{AsyncWriteExt as _, duplex};
#[cfg(unix)]
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    process::{Child, Command},
    time::{Duration, Instant, sleep},
};

use super::{
    MAX_CAPTURED_STDERR_BYTES,
    STDERR_TRUNCATION_MARKER,
    StderrCapture,
    capture_stderr,
    read_listening_port,
};
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

// Readiness reading over an in-memory pipe: a closed stream before any
// readiness line (the runner exiting or being signalled during startup) is a
// bounded, deterministic error rather than a hang.
#[tokio::test(flavor = "current_thread")]
async fn readiness_reports_exit_before_ready() -> Result<()> {
    let (mut writer, reader) = duplex(4096);
    writer
        .write_all(b"noise before readiness\n")
        .await
        .expect("write noise");
    drop(writer); // EOF: the runner exited before reporting readiness.
    let error = read_listening_port(reader)
        .await
        .expect_err("missing readiness must be an error");
    verify_that!(
        error.to_string(),
        contains_substring("exited before reporting readiness")
    )
}

#[tokio::test(flavor = "current_thread")]
async fn readiness_reports_error_event() -> Result<()> {
    let (mut writer, reader) = duplex(4096);
    writer
        .write_all(br#"{"version":1,"event":"error","message":"boom"}"#)
        .await
        .expect("write error event");
    writer.write_all(b"\n").await.expect("write newline");
    drop(writer);
    let error = read_listening_port(reader)
        .await
        .expect_err("an error event must be an error");
    verify_that!(error.to_string(), contains_substring("startup error"))
}

#[tokio::test(flavor = "current_thread")]
async fn readiness_ignores_noise_before_listening() -> Result<()> {
    let (mut writer, reader) = duplex(4096);
    writer
        .write_all(b"warming up\n")
        .await
        .expect("write noise");
    writer
        .write_all(br#"{"version":1,"event":"listening","host":"127.0.0.1","port":50505}"#)
        .await
        .expect("write readiness");
    writer.write_all(b"\n").await.expect("write newline");
    drop(writer);
    let port = read_listening_port(reader)
        .await
        .expect("readiness parsed after noise");
    verify_that!(port, eq(50505))
}

// Stderr capture drains to EOF deterministically: the write, an explicit EOF
// (dropping the writer), and joining the task form the barrier — no sleep.
#[tokio::test(flavor = "current_thread")]
async fn stderr_capture_records_stream_to_eof() -> Result<()> {
    let capture = Arc::new(Mutex::new(StderrCapture::default()));
    let (mut writer, reader) = duplex(4096);
    let task = tokio::spawn(capture_stderr(reader, Arc::clone(&capture)));
    writer
        .write_all(b"captured-stderr-marker")
        .await
        .expect("write marker");
    drop(writer); // EOF barrier: the capture task drains and completes.
    task.await.expect("capture task joins on EOF");
    let recorded = capture.lock().expect("capture lock").as_str();
    verify_that!(recorded, eq("captured-stderr-marker"))
}

// The capture buffer stays bounded even when flooded past its capacity.
#[tokio::test(flavor = "current_thread")]
async fn stderr_capture_bounds_unbounded_input() -> Result<()> {
    let capture = Arc::new(Mutex::new(StderrCapture::default()));
    let (mut writer, reader) = duplex(4096);
    let task = tokio::spawn(capture_stderr(reader, Arc::clone(&capture)));
    let flood = "x".repeat(MAX_CAPTURED_STDERR_BYTES * 2);
    writer
        .write_all(flood.as_bytes())
        .await
        .expect("write flood");
    drop(writer);
    task.await.expect("capture task joins on EOF");
    let recorded = capture.lock().expect("capture lock").as_str();
    verify_that!(recorded, starts_with(STDERR_TRUNCATION_MARKER))?;
    verify_that!(
        recorded.len(),
        le(STDERR_TRUNCATION_MARKER.len() + MAX_CAPTURED_STDERR_BYTES)
    )
}

// Aborting the readiness read while it is mid-line terminates the task cleanly
// (the join resolves as cancelled) instead of leaking or hanging.
#[tokio::test(flavor = "current_thread")]
async fn readiness_read_aborts_cleanly_while_in_flight() -> Result<()> {
    let (mut writer, reader) = duplex(64);
    let task = tokio::spawn(read_listening_port(reader));
    // A partial line (no newline) keeps the reader awaiting more when aborted.
    writer
        .write_all(b"partial-noise")
        .await
        .expect("write partial noise");
    task.abort();
    let join_error = task.await.expect_err("readiness task must be cancelled");
    drop(writer);
    verify_that!(join_error.is_cancelled(), eq(true))
}

// Aborting the stderr capture while it is awaiting more input terminates the
// task cleanly.
#[tokio::test(flavor = "current_thread")]
async fn stderr_capture_aborts_cleanly_while_in_flight() -> Result<()> {
    let capture = Arc::new(Mutex::new(StderrCapture::default()));
    let (mut writer, reader) = duplex(64);
    let task = tokio::spawn(capture_stderr(reader, Arc::clone(&capture)));
    writer
        .write_all(b"in-flight stderr")
        .await
        .expect("write stderr");
    task.abort();
    let join_error = task.await.expect_err("stderr task must be cancelled");
    drop(writer);
    verify_that!(join_error.is_cancelled(), eq(true))
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

// Spawns a process-group leader running `script`, returning the owned child
// (with `kill_on_drop` so the direct child is reaped, matching the real runner)
// and its first stdout line. Scripts print that marker only after arming their
// signal traps, so reading it synchronises the test on an explicit marker
// rather than a sleep.
#[cfg(unix)]
async fn spawn_scripted_leader(script: &str) -> (Child, String) {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(script)
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true);
    set_process_group(&mut command);
    let mut child = command.spawn().expect("spawn sh runner");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    let marker = lines
        .next_line()
        .await
        .expect("read readiness marker line")
        .expect("readiness marker line present");
    (child, marker)
}

// Backgrounds a grandchild in the leader's process group and reports its PID.
#[cfg(unix)]
const SCRIPT_GRANDCHILD: &str = "sleep 300 & printf '%s\\n' \"$!\"; wait\n";
// Traps SIGTERM into a clean direct-child exit after a descendant has reported
// that it is ignoring SIGTERM. Shutdown must still remove the whole group.
#[cfg(unix)]
const SCRIPT_CLEAN_ON_TERM: &str = concat!(
    "trap 'exit 0' TERM\n",
    "sh -c 'trap \"\" TERM; printf \"%s\\n\" \"$$\"; exec sleep 300' &\n",
    "wait\n",
);
// Traps SIGTERM into a non-zero exit after reporting a grandchild PID, so
// graceful shutdown observes a failure status and leaves no descendant.
#[cfg(unix)]
const SCRIPT_FAIL_ON_TERM: &str = "trap 'exit 3' TERM\nsleep 300 &\nprintf '%s\\n' \"$!\"\nwait\n";
// Ignores SIGTERM in both the direct child and its descendant, forcing the
// bounded shutdown timeout and process-group SIGKILL fallback.
#[cfg(unix)]
const SCRIPT_IGNORE_TERM: &str = concat!(
    "trap '' TERM\n",
    "sh -c 'trap \"\" TERM; printf \"%s\\n\" \"$$\"; exec sleep 300' &\n",
    "wait\n",
);

#[cfg(unix)]
async fn spawn_group_leader_with_grandchild() -> (Child, i32) {
    let (child, marker) = spawn_scripted_leader(SCRIPT_GRANDCHILD).await;
    (child, marker.trim().parse().expect("parse grandchild pid"))
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

// Graceful shutdown signals the owned group: the trapped clean exit lets the
// direct child exit 0, then final group cleanup removes the resistant
// descendant.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn shutdown_reaps_owned_process_group() -> Result<()> {
    let (child, marker) = spawn_scripted_leader(SCRIPT_CLEAN_ON_TERM).await;
    let grandchild_pid: i32 = marker.trim().parse().expect("parse grandchild pid");
    let guard = guard_around(child);

    guard.shutdown().await.expect("graceful shutdown succeeds");

    verify_that!(wait_until_process_absent(grandchild_pid).await, eq(true))
}

// A non-zero graceful exit must be surfaced by `shutdown`, not swallowed.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn shutdown_surfaces_runner_failure_status() -> Result<()> {
    let (child, marker) = spawn_scripted_leader(SCRIPT_FAIL_ON_TERM).await;
    let grandchild_pid: i32 = marker.trim().parse().expect("parse grandchild pid");
    let guard = guard_around(child);

    let error = guard
        .shutdown()
        .await
        .expect_err("non-zero graceful exit must surface");
    verify_that!(error.to_string(), contains_substring("failure status"))?;
    verify_that!(wait_until_process_absent(grandchild_pid).await, eq(true))
}

// A TERM-resistant process group exercises the bounded graceful-wait timeout;
// the fallback must reap the direct child and remove its descendant.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn shutdown_force_kills_after_graceful_timeout() -> Result<()> {
    let (child, marker) = spawn_scripted_leader(SCRIPT_IGNORE_TERM).await;
    let grandchild_pid: i32 = marker.trim().parse().expect("parse grandchild pid");
    let guard = guard_around(child);

    guard.shutdown().await.expect("forced shutdown succeeds");

    verify_that!(wait_until_process_absent(grandchild_pid).await, eq(true))
}

// Reaping an already-reaped child is deterministic and safe: Tokio caches the
// exit status, so the fallback returns `Ok` rather than hanging or panicking.
// The reaping-error branch is not deterministically reachable.
#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn force_kill_and_reap_is_idempotent_on_a_reaped_child() -> Result<()> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg("exit 0")
        .spawn()
        .expect("spawn short-lived child");
    child.wait().await.expect("reap short-lived child");

    force_kill_and_reap(&mut child, "test graceful failure")
        .await
        .expect("force kill and reap is safe on an already-reaped child");
    verify_that!(child.id(), none())
}

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
