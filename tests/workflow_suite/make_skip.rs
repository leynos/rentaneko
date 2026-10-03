//! The skip half of the make-backed tests: probing `make --version`, deciding
//! whether GNU make is available, and reporting why the tests skip.
//!
//! The probe (`make_version`, `version_from`), the decision (`require_gnu_make`,
//! which returns a typed [`Skip`] as an error) and the report (`report_skip`,
//! which writes to a caller-supplied writer) are separate units, each tested on
//! its own, and `stop_unless_gnu` joins them with an injectable probe and writer.
//! Kept apart from `make_probe.rs` so each file stays under the repository's
//! 400-line limit.

use rstest::rstest;

/// Why the make-backed tests cannot run on this host.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Skip {
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
pub(super) fn stop_without_gnu_make() -> std::io::Result<bool> {
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
