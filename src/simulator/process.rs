//! Owned-child process-group teardown primitives.
//!
//! On Unix the runner is spawned as a process-group leader so the whole owned
//! group can be signalled together. Other platforms fall back to signalling the
//! direct child only. None of these helpers inspect or signal processes the
//! [`super::Simulator`] does not own.

use std::process::ExitStatus;

#[cfg(unix)]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use tokio::process::{Child, Command};

use crate::error::RentanekoError;

/// Spawns the runner as a process-group leader where the platform supports it.
#[cfg(unix)]
pub(super) fn set_process_group(command: &mut Command) { command.process_group(0); }

/// No-op on platforms without POSIX process groups.
#[cfg(not(unix))]
pub(super) fn set_process_group(_command: &mut Command) {}

/// Requests a graceful stop of the owned process group with `SIGTERM`.
#[cfg(unix)]
pub(super) fn terminate_process_group(maybe_child_id: Option<u32>) {
    signal_process_group(maybe_child_id, Signal::SIGTERM);
}

/// No-op on platforms without POSIX process groups.
#[cfg(not(unix))]
pub(super) fn terminate_process_group(_maybe_child_id: Option<u32>) {}

#[cfg(unix)]
fn signal_process_group(maybe_child_id: Option<u32>, signal: Signal) {
    if let Some(pid) = maybe_child_id.and_then(process_group_pid) {
        // The group may already be gone; a failed signal is not actionable here.
        match kill(pid, signal) {
            Ok(()) | Err(_) => {}
        }
    }
}

/// Force-kills the owned process group with `SIGKILL`.
#[cfg(unix)]
pub(super) fn force_kill_process_group(child: &Child) {
    signal_process_group(child.id(), Signal::SIGKILL);
}

/// No-op on platforms without POSIX process groups.
#[cfg(not(unix))]
pub(super) fn force_kill_process_group(_child: &Child) {}

/// Maps the direct child's group id onto the negative pid `kill(2)` uses to
/// target the entire process group.
#[cfg(unix)]
pub(super) fn process_group_pid(process_group_id: u32) -> Option<Pid> {
    i32::try_from(process_group_id)
        .ok()
        .map(|pid| Pid::from_raw(-pid))
}

/// Maps a reaped child's exit status onto the shutdown result.
///
/// A non-success status (a non-zero exit code, or signal termination on Unix)
/// means the runner failed to shut down cleanly, so it is surfaced rather than
/// swallowed.
pub(super) fn exit_status_result(status: ExitStatus) -> Result<(), RentanekoError> {
    if status.success() {
        Ok(())
    } else {
        Err(RentanekoError::ShutdownFailed {
            message: format!("runner exited with failure status: {status}"),
        })
    }
}

/// Force-kills the owned process group, then reaps the direct child.
///
/// The whole group is `SIGKILL`ed first so any grandchildren the runner spawned
/// die too, not just the direct child reaped by [`Child::start_kill`].
///
/// # Errors
///
/// Returns [`RentanekoError::ShutdownFailed`] if the force-kill signal cannot be
/// delivered or the child cannot be reaped afterwards.
pub(super) async fn force_kill_and_reap(
    child: &mut Child,
    graceful_shutdown_error: impl std::fmt::Display,
) -> Result<(), RentanekoError> {
    force_kill_process_group(child);
    child
        .start_kill()
        .map_err(|force_kill_error| RentanekoError::ShutdownFailed {
            message: format!(
                "graceful shutdown failed: {graceful_shutdown_error}; force-kill failed: \
                 {force_kill_error}"
            ),
        })?;
    child
        .wait()
        .await
        .map(|_| ())
        .map_err(|reap_error| RentanekoError::ShutdownFailed {
            message: format!(
                "graceful shutdown failed: {graceful_shutdown_error}; force-kill reaping failed: \
                 {reap_error}"
            ),
        })
}
