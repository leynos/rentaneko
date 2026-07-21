//! Deterministic lifecycle and cancellation tests for [`super::Simulator`].
//!
//! The teardown tests inject a controllable `sh` fake through the private
//! `start_with` seam instead of the real Bun runner, so every shutdown and
//! cancellation path is exercised without Bun and without relying on timing
//! races. Each fake drives a specific outcome (readiness, early exit, ignored
//! signals, non-zero status), and process-group liveness is checked by polling
//! for a recorded grandchild PID. These paths are Unix-only because they depend
//! on POSIX process groups; the NDJSON parser tests remain portable.

use super::output::parse_listening_port;

/// A valid v1 readiness line the fakes echo to stdout.
const READY_LINE: &str = r#"{"version":1,"event":"listening","host":"127.0.0.1","port":50505}"#;

#[rstest::rstest]
#[case::valid(READY_LINE, Some(50505))]
#[case::non_json("Simulacat Core started", None)]
#[case::error_event(
    r#"{"version":1,"event":"error","host":"127.0.0.1","port":50505}"#,
    None
)]
#[case::missing_version(r#"{"event":"listening","host":"127.0.0.1","port":50505}"#, None)]
#[case::unsupported_version(
    r#"{"version":2,"event":"listening","host":"127.0.0.1","port":50505}"#,
    None
)]
#[case::non_loopback(
    r#"{"version":1,"event":"listening","host":"0.0.0.0","port":50505}"#,
    None
)]
#[case::zero_port(
    r#"{"version":1,"event":"listening","host":"127.0.0.1","port":0}"#,
    None
)]
#[case::missing_port(r#"{"version":1,"event":"listening","host":"127.0.0.1"}"#, None)]
fn parse_listening_port_classifies_lines(#[case] line: &str, #[case] expected: Option<u16>) {
    pretty_assertions::assert_eq!(parse_listening_port(line), expected);
}

#[cfg(unix)]
mod lifecycle {
    //! Deterministic teardown and cancellation tests driven by `sh` fakes.

    use std::{path::Path, time::Duration};

    use cap_std::{ambient_authority, fs::Dir};
    use googletest::prelude::*;
    use nix::{sys::signal::kill, unistd::Pid};
    use tokio::{
        process::Command,
        time::{sleep, timeout},
    };

    use super::READY_LINE;
    use crate::{RentanekoError, simulator::Simulator};

    const EXPECTED_BASE_URI: &str = "http://127.0.0.1:50505";
    const SEED_INSTALLATION: u64 = 2000;

    // Emits readiness, ignores SIGTERM/SIGINT, and only exits when stdin closes.
    const SCRIPT_STDIN_EOF: &str = concat!(
        "trap '' TERM INT\n",
        "printf '%s\\n' \"$RENTANEKO_TEST_READY\"\n",
        "cat\n",
    );
    // Exits immediately without ever reporting readiness.
    const SCRIPT_EARLY_EXIT: &str = "exit 0\n";
    // Reports a structured startup error before readiness.
    const SCRIPT_ERROR_EVENT: &str =
        "printf '%s\\n' '{\"version\":1,\"event\":\"error\",\"message\":\"boom\"}'\n";
    // Ready, but traps SIGTERM into a non-zero exit to model a failed shutdown.
    const SCRIPT_FAILURE_STATUS: &str = concat!(
        "trap 'exit 3' TERM\n",
        "printf '%s\\n' \"$RENTANEKO_TEST_READY\"\n",
        "while true; do sleep 0.05; done\n",
    );
    // Never ready: records a grandchild PID, then blocks.
    const SCRIPT_STARTUP_HANG: &str = concat!(
        "sleep 300 &\n",
        "printf '%s' \"$!\" > \"$RENTANEKO_TEST_PIDFILE\"\n",
        "wait\n",
    );
    // Ready, ignores signals and stdin, records a grandchild PID, then blocks so
    // graceful shutdown must time out and force-kill the whole group.
    const SCRIPT_FORCE_KILL: &str = concat!(
        "trap '' TERM INT\n",
        "sleep 300 &\n",
        "printf '%s' \"$!\" > \"$RENTANEKO_TEST_PIDFILE\"\n",
        "printf '%s\\n' \"$RENTANEKO_TEST_READY\"\n",
        "wait\n",
    );
    // Ready, then floods stdout until SIGTERM triggers a clean exit.
    const SCRIPT_STDOUT_FLOOD: &str = concat!(
        "trap 'exit 0' TERM\n",
        "printf '%s\\n' \"$RENTANEKO_TEST_READY\"\n",
        "while true; do printf 'noise\\n'; sleep 0.01; done\n",
    );
    // Ready, then floods stderr until SIGTERM triggers a clean exit.
    const SCRIPT_STDERR_FLOOD: &str = concat!(
        "trap 'exit 0' TERM\n",
        "printf '%s\\n' \"$RENTANEKO_TEST_READY\"\n",
        "while true; do printf 'noise\\n' 1>&2; sleep 0.01; done\n",
    );

    fn fake_runner(script: &str) -> Command {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(script)
            .env("RENTANEKO_TEST_READY", READY_LINE);
        command
    }

    fn fake_runner_with_pidfile(script: &str, pid_path: &Path) -> Command {
        let mut command = fake_runner(script);
        command.env("RENTANEKO_TEST_PIDFILE", pid_path);
        command
    }

    fn read_pid(path: &Path) -> Option<i32> {
        // Read through a capability handle on the parent directory rather than an
        // ambient `std::fs` call, per the repository filesystem policy.
        let parent = path.parent()?;
        let file_name = path.file_name()?;
        let dir = Dir::open_ambient_dir(parent, ambient_authority()).ok()?;
        let bytes = dir.read(file_name).ok()?;
        String::from_utf8(bytes).ok()?.trim().parse().ok()
    }

    async fn poll_pid_file(path: &Path) -> Option<i32> {
        for _ in 0..250 {
            if let Some(pid) = read_pid(path) {
                return Some(pid);
            }
            sleep(Duration::from_millis(20)).await;
        }
        None
    }

    // `kill(pid, None)` probes existence without delivering a signal; a reaped
    // or reparented-and-collected process reports `ESRCH`.
    async fn wait_until_process_absent(pid: i32) -> bool {
        let target = Pid::from_raw(pid);
        for _ in 0..250 {
            if kill(target, None).is_err() {
                return true;
            }
            sleep(Duration::from_millis(20)).await;
        }
        false
    }

    #[tokio::test(flavor = "current_thread")]
    async fn start_parses_readiness_and_shuts_down_via_stdin_eof() -> Result<()> {
        let simulator =
            Simulator::start_with(fake_runner(SCRIPT_STDIN_EOF), None, SEED_INSTALLATION)
                .await
                .expect("runner starts and reports readiness");
        verify_that!(simulator.base_uri(), eq(EXPECTED_BASE_URI))?;
        verify_that!(simulator.installation_id(), eq(SEED_INSTALLATION))?;
        // SIGTERM is ignored, so a clean exit proves stdin-EOF drove shutdown.
        simulator
            .shutdown()
            .await
            .expect("clean shutdown via stdin EOF");
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runner_exit_before_readiness_is_reported() -> Result<()> {
        let error = Simulator::start_with(fake_runner(SCRIPT_EARLY_EXIT), None, SEED_INSTALLATION)
            .await
            .expect_err("early exit must fail startup");
        verify_that!(
            matches!(error, RentanekoError::RunnerExitedBeforeReady { .. }),
            eq(true)
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runner_error_event_before_readiness_is_reported() -> Result<()> {
        let error = Simulator::start_with(fake_runner(SCRIPT_ERROR_EVENT), None, SEED_INSTALLATION)
            .await
            .expect_err("error event must fail startup");
        verify_that!(
            matches!(&error, RentanekoError::RunnerErrorEvent { message } if message.as_str() == "boom"),
            eq(true)
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_cancellation_reaps_the_process_group() -> Result<()> {
        let pid_dir = tempfile::tempdir().expect("temporary directory");
        let pid_path = pid_dir.path().join("pid");
        let command = fake_runner_with_pidfile(SCRIPT_STARTUP_HANG, &pid_path);

        // Dropping the start future mid-readiness cancels startup; `Drop` must
        // then reap the owned group.
        let outcome = timeout(
            Duration::from_millis(300),
            Simulator::start_with(command, None, SEED_INSTALLATION),
        )
        .await;
        verify_that!(outcome.is_err(), eq(true))?;

        let pid = poll_pid_file(&pid_path)
            .await
            .expect("grandchild pid recorded");
        verify_that!(wait_until_process_absent(pid).await, eq(true))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn graceful_timeout_forces_process_group_kill() -> Result<()> {
        let pid_dir = tempfile::tempdir().expect("temporary directory");
        let pid_path = pid_dir.path().join("pid");
        let mut simulator = Simulator::start_with(
            fake_runner_with_pidfile(SCRIPT_FORCE_KILL, &pid_path),
            None,
            SEED_INSTALLATION,
        )
        .await
        .expect("runner reports readiness");
        simulator.set_shutdown_timeout(Duration::from_millis(150));
        let pid = poll_pid_file(&pid_path)
            .await
            .expect("grandchild pid recorded");

        simulator
            .shutdown()
            .await
            .expect("forced shutdown reaps the group");
        verify_that!(wait_until_process_absent(pid).await, eq(true))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn drop_after_start_terminates_the_process_group() -> Result<()> {
        let pid_dir = tempfile::tempdir().expect("temporary directory");
        let pid_path = pid_dir.path().join("pid");
        let simulator = Simulator::start_with(
            fake_runner_with_pidfile(SCRIPT_FORCE_KILL, &pid_path),
            None,
            SEED_INSTALLATION,
        )
        .await
        .expect("runner reports readiness");
        let pid = poll_pid_file(&pid_path)
            .await
            .expect("grandchild pid recorded");

        // Exercise the synchronous last-resort `Drop` path directly, with no
        // `shutdown`/`teardown` call. `Drop` must SIGKILL the whole owned group
        // so the grandchild dies too, not merely the direct runner child.
        drop(simulator);

        verify_that!(wait_until_process_absent(pid).await, eq(true))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_surfaces_runner_failure_status() -> Result<()> {
        let simulator =
            Simulator::start_with(fake_runner(SCRIPT_FAILURE_STATUS), None, SEED_INSTALLATION)
                .await
                .expect("runner reports readiness");
        let error = simulator
            .shutdown()
            .await
            .expect_err("non-zero exit status must surface");
        verify_that!(
            matches!(error, RentanekoError::ShutdownFailed { .. }),
            eq(true)
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_joins_in_flight_stdout_capture() -> Result<()> {
        let mut simulator =
            Simulator::start_with(fake_runner(SCRIPT_STDOUT_FLOOD), None, SEED_INSTALLATION)
                .await
                .expect("runner reports readiness");
        simulator.teardown().await.expect("clean shutdown");
        verify_that!(simulator.stdout_task.is_none(), eq(true))?;
        verify_that!(simulator.stderr_task.is_none(), eq(true))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_joins_in_flight_stderr_capture() -> Result<()> {
        let mut simulator =
            Simulator::start_with(fake_runner(SCRIPT_STDERR_FLOOD), None, SEED_INSTALLATION)
                .await
                .expect("runner reports readiness");
        simulator.teardown().await.expect("clean shutdown");
        verify_that!(simulator.stdout_task.is_none(), eq(true))?;
        verify_that!(simulator.stderr_task.is_none(), eq(true))
    }
}
