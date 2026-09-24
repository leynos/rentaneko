//! Contract test that each pull request runs the test suite once.
//!
//! `make test` runs the suite with `--all-targets --all-features` and then
//! the doctests. The coverage step in `ci.yml` runs the same tests, since the
//! crate declares no features and has no examples or benches, but not the
//! doctests. The repository used to carry an `act-validation.yml` workflow
//! running `make test WITH_ACT=1`; nothing reads `WITH_ACT` and no test is
//! gated on Act, so it ran the whole suite a second time and was removed.
//!
//! These tests read the workflows and the manifest textually and hold the
//! split:
//!
//! - no workflow line, single-line `run:` or inside a `run: |` block, runs the suite outside the
//!   coverage step, in any spelling of `make test`, `cargo test` or `nextest run`, other than the
//!   doctest command;
//! - `build-test` runs the doctests, in a step with no `if:`;
//! - no workflow turns on the coverage action's doctests;
//! - the crate declares no feature, explicitly or through an optional dependency, which is what
//!   makes `--all-features` the coverage default.
//!
//! File access goes through a `cap_std` directory handle rooted at the crate
//! manifest directory.

use cap_std::{ambient_authority, fs::Dir};
use rstest::rstest;

/// The one suite command a workflow may run outside the coverage step.
const DOCTEST_COMMAND: &str = "cargo test --doc --workspace --all-features";

/// Opens the crate manifest directory as a capability-scoped handle.
fn manifest_dir() -> std::io::Result<Dir> {
    Dir::open_ambient_dir(env!("CARGO_MANIFEST_DIR"), ambient_authority())
}

/// Returns every workflow file's name and text.
fn workflows() -> std::io::Result<Vec<(String, String)>> {
    let dir = manifest_dir()?.open_dir(".github/workflows")?;
    let mut found = Vec::new();
    for entry in dir.entries()? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        let is_workflow = std::path::Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"));
        if is_workflow {
            let text = dir.read_to_string(&name)?;
            found.push((name, text));
        }
    }
    Ok(found)
}

/// Returns `true` if `words` holds `make`, then only options or variable
/// assignments, then the `test` target itself.
fn runs_make_test(words: &[&str]) -> bool {
    words
        .iter()
        .position(|word| *word == "make")
        .and_then(|start| words.get(start + 1..))
        .is_some_and(|rest| {
            rest.iter()
                .find(|word| !word.starts_with('-') && !word.contains('='))
                .is_some_and(|target| *target == "test")
        })
}

/// Returns `true` if a shell command line runs the suite, in any spelling.
fn runs_suite(line: &str) -> bool {
    let words: Vec<&str> = line.split_whitespace().collect();
    let cargo_test = words.windows(2).any(|pair| pair == ["cargo", "test"]);
    let nextest = words.windows(2).any(|pair| pair == ["nextest", "run"]);
    runs_make_test(&words) || cargo_test || nextest
}

/// Returns the command a `run:` line carries, or `None` for other lines.
fn run_command(line: &str) -> Option<&str> { line.trim_start().strip_prefix("run:").map(str::trim) }

/// Returns a workflow line as a command: without a `run:` prefix, and empty
/// for a comment. Every line is read, so a suite run inside a multi-line
/// `run: |` block is seen as well as a single-line one.
fn command_text(line: &str) -> &str {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return "";
    }
    run_command(trimmed).unwrap_or(trimmed)
}

/// Splits a workflow into its steps, each a run of lines starting at a
/// `- ` list item, so a step can be found by what it runs rather than by its
/// name.
fn steps(workflow: &str) -> Vec<Vec<&str>> {
    let mut found: Vec<Vec<&str>> = Vec::new();
    for line in workflow.lines() {
        if line.trim_start().starts_with("- ") {
            found.push(Vec::new());
        }
        if let Some(step) = found.last_mut() {
            step.push(line);
        }
    }
    found
}

#[rstest]
#[case::plain("make test", true)]
#[case::act_flag("make test WITH_ACT=1", true)]
#[case::make_option("make -j2 test", true)]
#[case::cargo("cargo test --all-features", true)]
#[case::nextest("cargo nextest run --all-targets", true)]
#[case::named_target("make test-workflow-contracts", false)]
#[case::lint("make lint", false)]
fn suite_spellings_are_recognized(#[case] line: &str, #[case] expected: bool) {
    assert_eq!(runs_suite(line), expected, "misread {line:?}");
}

#[test]
fn no_workflow_runs_the_suite_outside_coverage() {
    let found = workflows().expect("failed to read the workflows");
    let repeated: Vec<String> = found
        .iter()
        .flat_map(|(name, text)| {
            text.lines()
                .map(command_text)
                .filter(|command| runs_suite(command) && *command != DOCTEST_COMMAND)
                .map(move |command| format!("{name}: {command}"))
        })
        .collect();
    assert!(
        repeated.is_empty(),
        "the suite runs outside coverage in {repeated:?}"
    );
}

#[test]
fn build_test_runs_the_doctests_unconditionally() {
    let ci = manifest_dir()
        .and_then(|dir| dir.read_to_string(".github/workflows/ci.yml"))
        .expect("failed to read ci.yml");
    let doctest_steps: Vec<Vec<&str>> = steps(&ci)
        .into_iter()
        .filter(|step| {
            step.iter()
                .any(|line| run_command(line) == Some(DOCTEST_COMMAND))
        })
        .collect();
    assert_eq!(
        doctest_steps.len(),
        1,
        "ci.yml must run `{DOCTEST_COMMAND}` in one step"
    );
    assert!(
        !doctest_steps.iter().flatten().any(|line| line
            .trim_start()
            .trim_start_matches("- ")
            .starts_with("if:")),
        "the doctest step must run on every event"
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
    let explicit = manifest.lines().any(|line| line.trim() == "[features]");
    let implicit = manifest
        .lines()
        .any(|line| line.replace(' ', "").contains("optional=true"));
    assert!(
        !explicit && !implicit,
        "Cargo.toml declares features; recheck make test against the coverage run"
    );
}
