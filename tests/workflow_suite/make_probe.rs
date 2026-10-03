//! The make-backed half of the suite-once contract: the reader's default goal
//! is compared with the one GNU make itself settles on, so the behaviour is
//! pinned to make and not to anyone's reading of its manual.
//!
//! Linux only, where GNU make is the make in use. The tests skip, writing the
//! reason to stderr, on a host where `make` is absent or is not GNU make. The
//! probe, the decision and the report are separate units so each is tested on
//! its own.

use rstest::rstest;

use super::reading::default_goal_of;

/// What GNU make made of a fixture: the default goal it settled on, if it
/// accepted the Makefile, and what it wrote to stderr.
struct GoalProbe {
    /// The `.DEFAULT_GOAL` make settled on, or `None` where it refused.
    goal: Option<String>,
    /// Make's stderr, so a caller can show why a fixture was refused.
    diagnostic: String,
}

/// Returns what GNU make itself takes as the default goal of `makefile`, with
/// its diagnostic. `make -pn` prints the variable database without running a
/// recipe, and `.DEFAULT_GOAL` is the value make settled on after reading every
/// assignment (GNU make manual, "Other Special Variables"). It only runs the
/// probe and returns its result; reporting is the caller's.
///
/// # Errors
///
/// Returns the error raised starting `make` or feeding it the fixture.
fn make_default_goal(makefile: &str) -> std::io::Result<GoalProbe> {
    use std::{
        io::Write as _,
        process::{Command as Process, Stdio},
    };

    let mut child = Process::new("make")
        .args(["-f", "-", "-pn"])
        .env_remove("MAKEFLAGS")
        .env_remove("MAKELEVEL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("make's stdin was not piped"))?
        .write_all(makefile.as_bytes())?;
    let output = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let goal = output.status.success().then_some(()).and_then(|()| {
        text.lines().find_map(|line| {
            let (name, value) = line.split_once(" = ").or_else(|| line.split_once(" := "))?;
            (name == ".DEFAULT_GOAL").then(|| value.to_owned())
        })
    });
    Ok(GoalProbe {
        goal,
        diagnostic: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Why the make-backed tests cannot run on this host.
#[derive(Debug, PartialEq, Eq)]
enum Skip {
    /// No `make` could be started, or it did not answer `--version`.
    Absent,
    /// A `make` that does not identify itself as GNU make.
    NotGnu,
}

impl Skip {
    /// Returns the reason printed when the tests skip.
    const fn reason(&self) -> &'static str {
        match self {
            Self::Absent => {
                "skipped: make could not be run, so the reader cannot be pinned to it\n"
            }
            Self::NotGnu => "skipped: make is not GNU make, so the reader cannot be pinned to it\n",
        }
    }
}

/// Reads what `make --version` printed, from the process's exit status and
/// standard output. A failed run is an error, so the tests skip as `Absent`
/// instead of reading whatever a broken `make` printed.
///
/// # Errors
///
/// Returns an error where `make --version` did not exit successfully.
fn version_from(succeeded: bool, stdout: &[u8]) -> std::io::Result<String> {
    if succeeded {
        Ok(String::from_utf8_lossy(stdout).into_owned())
    } else {
        Err(std::io::Error::other("make --version failed"))
    }
}

/// Runs `make --version` and returns its standard output. The only process
/// call: it starts `make` and hands the outcome to [`version_from`].
fn make_version() -> std::io::Result<String> {
    let output = std::process::Command::new("make")
        .arg("--version")
        .output()?;
    version_from(output.status.success(), &output.stdout)
}

/// Decides from the `make --version` result whether GNU make is available.
/// Pure: a missing or unstartable make is `Absent`, any other make is
/// `NotGnu`.
///
/// # Errors
///
/// Returns the [`Skip`] reason where the tests cannot run.
fn require_gnu_make(version: std::io::Result<String>) -> Result<(), Skip> {
    match version {
        Err(_) => Err(Skip::Absent),
        Ok(text) if text.contains("GNU Make") => Ok(()),
        Ok(_) => Err(Skip::NotGnu),
    }
}

/// Writes the reason for a skip to `out`, which the caller chooses.
///
/// # Errors
///
/// Returns the error `out` raised while writing.
fn report_skip(out: &mut impl std::io::Write, skip: &Skip) -> std::io::Result<()> {
    out.write_all(skip.reason().as_bytes())
}

/// Joins a probe result to a report: writes the skip reason to `out` and
/// returns `true` where the test must stop, `false` (writing nothing) where
/// GNU make is available. The probe and the writer are supplied, so both
/// outcomes are tested without a host that lacks make.
///
/// # Errors
///
/// Returns the error `out` raised while writing the reason.
fn stop_unless_gnu(
    version: std::io::Result<String>,
    out: &mut impl std::io::Write,
) -> std::io::Result<bool> {
    match require_gnu_make(version) {
        Ok(()) => Ok(false),
        Err(skip) => report_skip(out, &skip).map(|()| true),
    }
}

/// Reports a skip on stderr, which `--nocapture` shows (`eprintln!` is denied
/// here), and returns `true` where the test must stop.
///
/// # Errors
///
/// Returns the error stderr raised while writing the reason.
fn stop_without_gnu_make() -> std::io::Result<bool> {
    stop_unless_gnu(make_version(), &mut std::io::stderr())
}

/// The boundary stops and reports for a make that is absent or not GNU, runs
/// and writes nothing for GNU make, and surfaces a failing writer.
#[rstest]
#[case::gnu(Ok("GNU Make 4.4.1\n".to_owned()), false, "")]
#[case::bsd(
    Ok("bmake 20240101\n".to_owned()),
    true,
    "skipped: make is not GNU make, so the reader cannot be pinned to it\n"
)]
#[case::missing(
    Err(std::io::ErrorKind::NotFound.into()),
    true,
    "skipped: make could not be run, so the reader cannot be pinned to it\n"
)]
fn the_boundary_stops_and_reports_or_runs(
    #[case] version: std::io::Result<String>,
    #[case] stops: bool,
    #[case] written: &str,
) {
    let mut out = Vec::new();
    assert_eq!(
        stop_unless_gnu(version, &mut out).expect("a Vec accepts every write"),
        stops
    );
    assert_eq!(
        String::from_utf8(out).expect("the report is UTF-8"),
        written
    );
}

#[test]
fn the_boundary_surfaces_a_failing_writer_only_when_it_must_report() {
    let error = stop_unless_gnu(Err(std::io::ErrorKind::NotFound.into()), &mut Refusing)
        .expect_err("a skip that cannot be reported is an error");
    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    assert!(
        !stop_unless_gnu(Ok("GNU Make 4.4.1\n".to_owned()), &mut Refusing)
            .expect("GNU make writes nothing, so a refusing writer is never reached")
    );
}

/// A failed `make --version` is an error whatever it printed, and a
/// successful one is its output, so a broken make skips as `Absent`.
#[rstest]
#[case::ok(true, b"GNU Make 4.4.1\n", Some("GNU Make 4.4.1\n"))]
#[case::failed_but_gnu_looking(false, b"GNU Make 4.4.1\n", None)]
#[case::failed_and_silent(false, b"", None)]
fn a_failed_version_probe_is_an_error(
    #[case] succeeded: bool,
    #[case] stdout: &[u8],
    #[case] expected: Option<&str>,
) {
    assert_eq!(version_from(succeeded, stdout).ok().as_deref(), expected);
    if !succeeded {
        let outcome = require_gnu_make(version_from(succeeded, stdout));
        assert_eq!(outcome, Err(Skip::Absent));
    }
}

/// Each way `make --version` can answer is either GNU make or a named skip.
#[rstest]
#[case::gnu(Ok("GNU Make 4.4.1\nBuilt for x86_64-pc-linux-gnu\n".to_owned()), Ok(()))]
#[case::bsd(Ok("bmake 20240101\n".to_owned()), Err(Skip::NotGnu))]
#[case::silent(Ok(String::new()), Err(Skip::NotGnu))]
#[case::missing(Err(std::io::ErrorKind::NotFound.into()), Err(Skip::Absent))]
#[case::refused(Err(std::io::ErrorKind::PermissionDenied.into()), Err(Skip::Absent))]
fn make_is_classified_from_its_version(
    #[case] version: std::io::Result<String>,
    #[case] expected: Result<(), Skip>,
) {
    assert_eq!(require_gnu_make(version), expected);
}

/// The skip messages are user-visible and stable, so they are held exactly,
/// and the report writes what the reason says and nothing more.
#[rstest]
#[case::absent(
    Skip::Absent,
    "skipped: make could not be run, so the reader cannot be pinned to it\n"
)]
#[case::not_gnu(
    Skip::NotGnu,
    "skipped: make is not GNU make, so the reader cannot be pinned to it\n"
)]
fn a_skip_is_reported_with_its_exact_reason(#[case] skip: Skip, #[case] expected: &str) {
    let mut out = Vec::new();
    report_skip(&mut out, &skip).expect("a Vec accepts every write");
    assert_eq!(
        String::from_utf8(out).expect("the report is UTF-8"),
        expected
    );
}

/// A writer that refuses every write, to prove the report's failure surfaces.
struct Refusing;

impl std::io::Write for Refusing {
    fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

#[test]
fn a_failed_report_is_an_error_not_a_silent_skip() {
    let error = report_skip(&mut Refusing, &Skip::Absent).expect_err("the write must fail");
    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
}

/// The reader agrees with GNU make on every Makefile it can be run on, so the
/// behaviour is pinned to make and not to anyone's reading of its manual.
#[rstest]
#[case::first_rule(".PHONY: a\nbuild: x\nx:\n")]
#[case::assigned(".DEFAULT_GOAL := test\nbuild:\ntest:\n")]
#[case::conditional(".DEFAULT_GOAL ?= test\nbuild:\ntest:\n")]
#[case::plain(".DEFAULT_GOAL = test\nbuild:\ntest:\n")]
#[case::last_assignment_wins(
    ".DEFAULT_GOAL := first\n.DEFAULT_GOAL := second\nfirst:\nsecond:\nbuild:\n"
)]
#[case::conditional_keeps_the_first(
    ".DEFAULT_GOAL := first\n.DEFAULT_GOAL ?= second\nfirst:\nsecond:\n"
)]
#[case::conditional_then_set(".DEFAULT_GOAL ?= second\n.DEFAULT_GOAL := first\nfirst:\nsecond:\n")]
#[case::a_later_change_to_test(".DEFAULT_GOAL := build\nbuild:\ntest:\n.DEFAULT_GOAL := test\n")]
#[case::an_empty_value_clears_it(".DEFAULT_GOAL := first\n.DEFAULT_GOAL :=\nbuild:\nfirst:\n")]
#[case::appending_to_nothing(".DEFAULT_GOAL += test\nbuild:\ntest:\n")]
#[case::an_empty_append_keeps_the_value(
    ".DEFAULT_GOAL := first\n.DEFAULT_GOAL +=\nbuild:\nfirst:\n"
)]
#[case::comments_are_skipped("# build: not a rule\nrun:\n")]
#[case::special_targets_are_skipped(".PHONY: a\n.SUFFIXES:\nrun:\n")]
#[case::recipe_text_is_not_an_assignment("first:\n\t@: .DEFAULT_GOAL = test\nsecond:\n")]
fn the_reader_agrees_with_gnu_make(#[case] makefile: &str) {
    if stop_without_gnu_make().expect("stderr must be writable") {
        return;
    }
    let probe = make_default_goal(makefile).expect("make must run");
    let Some(by_make) = probe.goal else {
        panic!("make refused {makefile:?}: {}", probe.diagnostic);
    };
    assert_eq!(default_goal_of(makefile), by_make, "{makefile:?}");
}

/// Make refuses a default goal of several targets; the reader does not read
/// such a value and falls back to the first rule.
#[test]
fn make_refuses_several_words_and_the_reader_does_not_read_them() {
    if stop_without_gnu_make().expect("stderr must be writable") {
        return;
    }
    let makefile = ".DEFAULT_GOAL := first\n.DEFAULT_GOAL += second\nbuild:\nfirst:\nsecond:\n";
    let probe = make_default_goal(makefile).expect("make must run");
    assert_eq!(probe.goal, None, "make must refuse several words");
    assert!(
        !probe.diagnostic.is_empty(),
        "make must say why it refused the fixture"
    );
    assert_eq!(default_goal_of(makefile), "build");
}

/// The reader agrees with GNU make over every sequence of up to three
/// `.DEFAULT_GOAL` assignments, so the behaviour is pinned to make exhaustively
/// and not to a sample. A sequence make refuses (several words) is skipped
/// here and covered by the several-words case above.
#[test]
fn the_reader_agrees_with_gnu_make_on_every_bounded_sequence() {
    if stop_without_gnu_make().expect("stderr must be writable") {
        return;
    }
    let mut disagreements = Vec::new();
    for operations in super::properties::sequences(3) {
        let text = super::properties::makefile(&operations);
        let probe = make_default_goal(&text).expect("make must run");
        if let Some(by_make) = probe.goal
            && default_goal_of(&text) != by_make
        {
            disagreements.push((text, by_make));
        }
    }
    assert!(
        disagreements.is_empty(),
        "the reader disagrees with make on {:?}",
        disagreements.iter().take(3).collect::<Vec<_>>()
    );
}
