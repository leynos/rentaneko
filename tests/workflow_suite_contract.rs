//! Contract test that each pull request runs the test suite once.
//!
//! `make test` runs the suite with `--all-targets --all-features` and then
//! the doctests. The coverage step in `ci.yml`'s `build-test` job runs the
//! same tests, but not the doctests, which `build-test` runs in a step of its
//! own. The repository used to carry an `act-validation.yml` workflow running
//! `make test WITH_ACT=1`; nothing reads `WITH_ACT` and no test is gated on
//! Act, so it ran the whole suite a second time and was removed.
//!
//! These tests read the workflows textually and the manifest as TOML, and
//! hold the split:
//!
//! - no workflow line runs the suite, in any spelling of `make test`, `make all`, `cargo test`,
//!   `cargo nextest` or `cargo llvm-cov`, options before the subcommand included, except the one
//!   doctest step;
//! - that doctest step is in `build-test`, and neither the job nor the step carries an `if:`;
//! - `build-test` runs the coverage action in one unguarded step, and no workflow turns on its
//!   doctests;
//! - the crate declares no feature, in a `[features]` table or through an optional dependency,
//!   which is what makes `--all-features` the coverage default.
//!
//! File access goes through a `cap_std` directory handle rooted at the crate
//! manifest directory.

use rstest::rstest;

#[path = "workflow_suite/reading.rs"]
mod reading;

use reading::{
    command_text,
    job_is_conditional,
    jobs,
    manifest_dir,
    manifest_features,
    run_command,
    runs_suite,
    step_is_conditional,
    steps,
    workflows,
};

/// The one suite command a workflow may run outside the coverage step.
const DOCTEST_COMMAND: &str = "cargo test --doc --workspace --all-features";

/// The coverage action every pull request's suite run goes through.
const COVERAGE_ACTION: &str = "leynos/shared-actions/.github/actions/generate-coverage@";

/// The job that must run both the coverage step and the doctest step.
const SUITE_JOB: &str = "build-test";

/// Returns the `build-test` job of `ci.yml`.
fn suite_job(found: &[(String, String)]) -> Vec<&str> {
    found
        .iter()
        .filter(|(name, _)| name == "ci.yml")
        .flat_map(|(_, text)| jobs(text))
        .find(|(name, _)| *name == SUITE_JOB)
        .map(|(_, lines)| lines)
        .unwrap_or_default()
}

#[rstest]
#[case::plain("make test", true)]
#[case::act_flag("make test WITH_ACT=1", true)]
#[case::make_option("make -j2 test", true)]
#[case::make_directory("make -C . test", true)]
#[case::make_all("make all", true)]
#[case::cargo("cargo test --all-features", true)]
#[case::cargo_config("cargo --config tools/dev-fast/config.toml test", true)]
#[case::cargo_toolchain("cargo +nightly test", true)]
#[case::nextest("cargo nextest run --all-targets", true)]
#[case::llvm_cov("cargo llvm-cov nextest --lcov", true)]
#[case::chained("set -eu && make test", true)]
#[case::named_target("make test-workflow-contracts", false)]
#[case::lint("make lint", false)]
#[case::build("cargo build --all-targets", false)]
#[case::run_argument("cargo run -- test", false)]
fn suite_spellings_are_recognized(#[case] line: &str, #[case] expected: bool) {
    assert_eq!(runs_suite(line), expected, "misread {line:?}");
}

/// Returns every workflow line that runs the suite, as
/// `(workflow, job, command)`.
fn suite_runs(found: &[(String, String)]) -> Vec<(&str, &str, &str)> {
    found
        .iter()
        .flat_map(|(name, text)| {
            jobs(text).into_iter().flat_map(move |(job, lines)| {
                lines
                    .into_iter()
                    .map(command_text)
                    .filter(|command| runs_suite(command))
                    .map(move |command| (name.as_str(), job, command))
            })
        })
        .collect()
}

#[test]
fn only_the_doctest_step_runs_the_suite_outside_coverage() {
    let found = workflows().expect("failed to read the workflows");
    let (doctests, repeated): (Vec<_>, Vec<_>) =
        suite_runs(&found)
            .into_iter()
            .partition(|(name, job, command)| {
                *command == DOCTEST_COMMAND && *name == "ci.yml" && *job == SUITE_JOB
            });
    assert!(
        repeated.is_empty(),
        "the suite runs outside coverage in {repeated:?}"
    );
    assert_eq!(
        doctests.len(),
        1,
        "`{DOCTEST_COMMAND}` must run once, in {SUITE_JOB}"
    );
}

#[test]
fn build_test_runs_the_doctests_unconditionally() {
    let found = workflows().expect("failed to read the workflows");
    let job = suite_job(&found);
    assert!(!job.is_empty(), "ci.yml must define {SUITE_JOB}");
    assert!(
        !job_is_conditional(&job),
        "{SUITE_JOB} must run on every pull request"
    );
    let doctest_steps: Vec<Vec<&str>> = steps(&job)
        .into_iter()
        .filter(|step| {
            step.iter()
                .any(|line| run_command(line) == Some(DOCTEST_COMMAND))
        })
        .collect();
    assert_eq!(
        doctest_steps.len(),
        1,
        "{SUITE_JOB} must run the doctests in one step"
    );
    assert!(
        !doctest_steps.iter().any(|step| step_is_conditional(step)),
        "the doctest step must run on every event"
    );
}

#[test]
fn build_test_runs_coverage_unconditionally() {
    let found = workflows().expect("failed to read the workflows");
    let job = suite_job(&found);
    let coverage_steps: Vec<Vec<&str>> = steps(&job)
        .into_iter()
        .filter(|step| step.iter().any(|line| line.contains(COVERAGE_ACTION)))
        .collect();
    assert_eq!(
        coverage_steps.len(),
        1,
        "{SUITE_JOB} must run the coverage action once"
    );
    assert!(
        !coverage_steps.iter().any(|step| step_is_conditional(step)),
        "the coverage step must run on every event"
    );
}

#[test]
fn coverage_leaves_the_doctests_off() {
    let found = workflows().expect("failed to read the workflows");
    let enabling: Vec<&str> = found
        .iter()
        .filter(|(_, text)| {
            text.lines().any(|raw| {
                let line = raw.trim();
                line.starts_with("doctests:") && line.contains("true")
            })
        })
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        enabling.is_empty(),
        "coverage would repeat the doctests in {enabling:?}"
    );
}

#[test]
fn the_crate_declares_no_features() {
    let manifest = manifest_dir()
        .and_then(|dir| dir.read_to_string("Cargo.toml"))
        .expect("failed to read Cargo.toml");
    let features = manifest_features(&manifest).expect("Cargo.toml must parse as TOML");
    assert!(
        features.is_empty(),
        "Cargo.toml declares {features:?}; recheck make test against the coverage run"
    );
}

#[rstest]
#[case::none("[package]\nname = \"x\"\n", &[])]
#[case::commented_table("[features] # flags\nextra = []\n", &["extra"])]
#[case::tabbed_optional("[dependencies]\nserde = { version = \"1\", optional\t=\ttrue }\n", &["serde"])]
#[case::target_optional("[target.'cfg(unix)'.dependencies]\nlibc = { version = \"1\", optional = true }\n", &["libc"])]
#[case::required("[dependencies]\nserde = { version = \"1\" }\n", &[])]
fn manifest_features_are_read(#[case] manifest: &str, #[case] expected: &[&str]) {
    let features = manifest_features(manifest).expect("the fixture must parse");
    assert_eq!(features, expected);
}
