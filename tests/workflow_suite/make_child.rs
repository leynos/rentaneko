//! The end-to-end half of the make-backed tests: this test binary is run as a
//! child, with `PATH` naming no `make` or with it present, and every make-backed
//! test in `make_probe` must skip with its reason or run for real.
//!
//! The child gets none of make's flag variables, and its selection is
//! `make_probe::` only, which no child-run test path contains, so a child can
//! never select the child-run tests and recurse; a test holds that.
//! Kept apart from `make_probe.rs` so each file stays under the repository's
//! 400-line limit.

use super::{make_probe::INHERITED_FLAGS, make_skip::stop_without_gnu_make};

/// Runs this test binary as a child with `search_path` as its `PATH` (or its own when
/// `None`) and `filter` as the only test selection, returning its output. The
/// environment is the child's alone; the test process is never mutated.
///
/// # Errors
///
/// Returns the error raised locating or starting the test binary.
fn run_self(search_path: Option<&str>) -> std::io::Result<std::process::Output> {
    child_command(
        std::env::current_exe()?,
        search_path,
        &make_backed_selection(),
    )
    .output()
}

/// The tests in `make_probe` that do not call `stop_without_gnu_make`: they run
/// the same with or without make, so the child selection leaves them out. Every
/// other test in `make_probe` is make-backed, so a new make-backed test is
/// covered by the child runs without being listed, and a new pure test is
/// listed here. The child-run tests live in this module, which the selection
/// never matches, so a child cannot select them.
const NOT_MAKE_BACKED: [&str; 1] = ["a_disagreement_is_named_for_every_shape"];

/// The child's test selection: `make_probe`'s tests, less [`NOT_MAKE_BACKED`],
/// so the child runs every make-backed test and none of the child-run tests.
fn make_backed_selection() -> Vec<String> {
    let mut selection = vec!["make_probe::".to_owned(), "--nocapture".to_owned()];
    for name in NOT_MAKE_BACKED {
        selection.push("--skip".to_owned());
        selection.push(name.to_owned());
    }
    selection
}

/// Builds the child command: the test binary at `program` with `selection` as
/// its arguments, `search_path` as its `PATH` when given and without the make
/// flag variables a parent `make test` may pass down (a jobserver entry in
/// `MAKEFLAGS` names descriptors the child does not hold), so the child-run
/// tests do not depend on the parent's make context.
fn child_command(
    program: std::path::PathBuf,
    search_path: Option<&str>,
    selection: &[String],
) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    command.args(selection);
    for flag in INHERITED_FLAGS {
        command.env_remove(flag);
    }
    if let Some(directories) = search_path {
        command.env("PATH", directories);
    }
    command
}

/// The child inherits none of make's flag variables, whatever the parent
/// carries, and sets `PATH` only when asked.
#[test]
fn the_child_run_drops_make_flags_and_sets_only_the_requested_path() {
    let command = child_command(
        "test-binary".into(),
        Some("/nowhere"),
        &["filter".to_owned()],
    );
    let envs: Vec<_> = command.get_envs().collect();
    for flag in INHERITED_FLAGS {
        assert!(
            envs.iter()
                .any(|(name, value)| *name == flag && value.is_none()),
            "{flag} must be removed from the child's environment: {envs:?}"
        );
    }
    assert!(
        envs.iter().any(|(name, value)| *name == "PATH"
            && value.is_some_and(|directories| directories == "/nowhere")),
        "PATH must be set for the child: {envs:?}"
    );
    let inherited = child_command("test-binary".into(), None, &["filter".to_owned()]);
    assert!(
        inherited.get_envs().all(|(name, _)| name != "PATH"),
        "PATH must be left alone when none is requested"
    );
}

/// Returns how many tests the child reported passing.
fn passed(stdout: &str) -> usize {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("test result: ok. "))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}

/// With no `make` on `PATH`, every make-backed test skips, succeeds and says
/// why on stderr, so the skip is proved end to end and not only by its parts.
#[test]
fn a_host_without_make_skips_every_make_backed_test() {
    let output = run_self(Some("/nonexistent-no-make-here")).expect("the test binary must run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the child failed: {stdout}{stderr}"
    );
    let ran = passed(&stdout);
    assert!(ran > 0, "the child ran no make-backed test: {stdout}");
    let skips = stderr.matches("skipped: make could not be run").count();
    assert_eq!(
        skips, ran,
        "every make-backed test must say why it skipped: {stderr}"
    );
}

/// With GNU make on `PATH`, the same tests run for real and print no skip.
#[test]
fn a_host_with_make_runs_the_make_backed_tests() {
    if stop_without_gnu_make().expect("stderr must be writable") {
        return;
    }
    let output = run_self(None).expect("the test binary must run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the child failed: {stdout}{stderr}"
    );
    assert!(
        passed(&stdout) > 0,
        "the child ran no make-backed test: {stdout}"
    );
    assert!(
        !stderr.contains("skipped:"),
        "a host with make skipped: {stderr}"
    );
}

/// The child's selection must never match the child-run tests, or a child would
/// run them and spawn children without end. Selection is by substring of the
/// test path, so the child-run tests' own paths must not contain it.
#[test]
fn the_child_selection_cannot_match_the_child_run_tests() {
    let selection = make_backed_selection();
    let pattern = selection.first().expect("the selection names a filter");
    for name in [
        "a_host_without_make_skips_every_make_backed_test",
        "a_host_with_make_runs_the_make_backed_tests",
        "the_child_run_drops_make_flags_and_sets_only_the_requested_path",
        "the_child_selection_cannot_match_the_child_run_tests",
    ] {
        let path = format!("{}::{name}", module_path!());
        assert!(
            !path.contains(pattern.as_str()),
            "the child selection {pattern:?} matches the child-run test {path}, which would \
             recurse"
        );
    }
}
