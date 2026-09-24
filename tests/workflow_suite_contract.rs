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

use cap_std::{ambient_authority, fs::Dir};
use rstest::rstest;

/// The one suite command a workflow may run outside the coverage step.
const DOCTEST_COMMAND: &str = "cargo test --doc --workspace --all-features";

/// The coverage action every pull request's suite run goes through.
const COVERAGE_ACTION: &str = "leynos/shared-actions/.github/actions/generate-coverage@";

/// The job that must run both the coverage step and the doctest step.
const SUITE_JOB: &str = "build-test";

/// Cargo options that take their value as the next word.
const CARGO_VALUE_OPTIONS: [&str; 6] = [
    "--config",
    "-Z",
    "-C",
    "--manifest-path",
    "--color",
    "--target-dir",
];

/// Make options that take their value as the next word.
const MAKE_VALUE_OPTIONS: [&str; 8] = [
    "-C",
    "-f",
    "-I",
    "-o",
    "-W",
    "--directory",
    "--file",
    "--makefile",
];

/// Cargo subcommands that run the suite.
const SUITE_SUBCOMMANDS: [&str; 3] = ["test", "nextest", "llvm-cov"];

/// Make targets that run the suite.
const SUITE_TARGETS: [&str; 2] = ["test", "all"];

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

/// Splits a command into the word lists of its shell segments, so a suite
/// run after `&&`, `||`, `;` or `|` is read as its own command.
fn segments(command: &str) -> Vec<Vec<&str>> {
    let mut found = Vec::new();
    let mut current = Vec::new();
    for word in command.split_whitespace() {
        let trimmed = word.trim_end_matches(';');
        let separator = matches!(word, "&&" | "||" | "|") || trimmed.is_empty();
        if !separator {
            current.push(trimmed);
        }
        if separator || word.ends_with(';') {
            found.push(std::mem::take(&mut current));
        }
    }
    found.push(current);
    found
}

/// Returns the words after the first word equal to `program`, or naming it
/// by path, in one segment.
fn arguments_of<'a>(words: &[&'a str], program: &str) -> Option<Vec<&'a str>> {
    let suffix = format!("/{program}");
    let start = words
        .iter()
        .position(|word| *word == program || word.ends_with(&suffix))?;
    Some(words.get(start + 1..).unwrap_or_default().to_vec())
}

/// Returns `true` if a word is an operand rather than an option, a
/// toolchain selector or a variable assignment.
fn is_operand(word: &str) -> bool {
    let is_flag = word.starts_with('-') || word.starts_with('+');
    !is_flag && !word.contains('=')
}

/// Returns the operands of a command line: its words less options, their
/// values, toolchain selectors and variable assignments.
fn operands<'a>(arguments: &[&'a str], value_options: &[&str]) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        if value_options.contains(word) {
            words.next();
        } else if is_operand(word) {
            found.push(*word);
        }
    }
    found
}

/// Returns `true` if one shell segment runs the suite.
fn segment_runs_suite(words: &[&str]) -> bool {
    let cargo = arguments_of(words, "cargo").is_some_and(|arguments| {
        operands(&arguments, &CARGO_VALUE_OPTIONS)
            .first()
            .is_some_and(|subcommand| SUITE_SUBCOMMANDS.contains(subcommand))
    });
    let make = arguments_of(words, "make").is_some_and(|arguments| {
        operands(&arguments, &MAKE_VALUE_OPTIONS)
            .iter()
            .any(|target| SUITE_TARGETS.contains(target))
    });
    cargo || make
}

/// Returns `true` if a shell command line runs the suite, in any spelling.
fn runs_suite(line: &str) -> bool { segments(line).iter().any(|words| segment_runs_suite(words)) }

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

/// Returns the name of the job a line opens, for a `  name:` line under
/// `jobs:`.
fn job_header(line: &str) -> Option<&str> {
    let name = line.strip_prefix("  ")?.strip_suffix(':')?;
    let is_name = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    is_name.then_some(name)
}

/// Returns each job's name and lines, in order.
fn jobs(workflow: &str) -> Vec<(&str, Vec<&str>)> {
    let mut found: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut in_jobs = false;
    for line in workflow.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            in_jobs = line.trim_end() == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        if let Some(name) = job_header(line) {
            found.push((name, Vec::new()));
        } else if let Some((_, lines)) = found.last_mut() {
            lines.push(line);
        }
    }
    found
}

/// Splits a job into its steps, each a run of lines starting at a `- ` list
/// item, so a step can be found by what it runs rather than by its name.
fn steps<'a>(job: &[&'a str]) -> Vec<Vec<&'a str>> {
    let mut found: Vec<Vec<&'a str>> = Vec::new();
    for line in job {
        if line.trim_start().starts_with("- ") {
            found.push(Vec::new());
        }
        if let Some(step) = found.last_mut() {
            step.push(line);
        }
    }
    found
}

/// Returns `true` if a job carries its own `if:` condition.
fn job_is_conditional(job: &[&str]) -> bool { job.iter().any(|line| line.starts_with("    if:")) }

/// Returns `true` if a step carries an `if:` condition.
fn step_is_conditional(step: &[&str]) -> bool {
    step.iter().any(|line| {
        line.trim_start()
            .trim_start_matches("- ")
            .starts_with("if:")
    })
}

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

/// Returns a manifest's feature names, with each optional dependency, which
/// Cargo turns into an implicit feature, added by name.
fn manifest_features(manifest: &str) -> Result<Vec<String>, toml::de::Error> {
    let parsed: toml::Value = toml::from_str(manifest)?;
    let mut found: Vec<String> = parsed
        .get("features")
        .and_then(toml::Value::as_table)
        .map(|table| table.keys().cloned().collect())
        .unwrap_or_default();
    let tables = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut dependency_tables: Vec<&toml::Value> =
        tables.iter().filter_map(|name| parsed.get(*name)).collect();
    if let Some(targets) = parsed.get("target").and_then(toml::Value::as_table) {
        dependency_tables.extend(
            targets
                .values()
                .flat_map(|platform| tables.iter().filter_map(move |name| platform.get(*name))),
        );
    }
    found.extend(
        dependency_tables
            .iter()
            .filter_map(|table| table.as_table())
            .flat_map(|table| table.iter())
            .filter(|(_, spec)| spec.get("optional").and_then(toml::Value::as_bool) == Some(true))
            .map(|(name, _)| name.clone()),
    );
    Ok(found)
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
