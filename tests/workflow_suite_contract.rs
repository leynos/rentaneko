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
//! - no workflow line runs the suite, in any spelling of `make test`, `make all`, a bare `make`,
//!   `cargo test`, `cargo nextest` or `cargo llvm-cov`, whatever the options or separators around
//!   it, except the one doctest step;
//! - that doctest step is in `build-test`, and neither the job nor the step carries an `if:`;
//! - `build-test` runs the coverage action in one unguarded step, and no workflow turns on its
//!   doctests;
//! - the crate declares no feature, in a `[features]` table or through an optional dependency,
//!   which is what makes `--all-features` the coverage default.
//!
//! The readers live in `workflow_suite/reading.rs`.

use rstest::rstest;

#[path = "workflow_suite/reading.rs"]
mod reading;

use reading::{
    Command,
    Job,
    Manifest,
    Workflow,
    default_goal,
    default_goal_of,
    manifest_dir,
    workflows,
};

/// The one suite command a workflow may run outside the coverage step.
const DOCTEST_COMMAND: &str = "cargo test --doc --workspace --all-features";

/// The coverage action every pull request's suite run goes through.
const COVERAGE_ACTION: &str = "leynos/shared-actions/.github/actions/generate-coverage@";

/// The default goal the spelling cases assume: one that runs the suite.
const SUITE_GOAL: &str = "all";

/// The job that must run both the coverage step and the doctest step.
const SUITE_JOB: &str = "build-test";

/// Returns the `build-test` job of `ci.yml`, if it exists.
fn suite_job(found: &[(String, String)]) -> Option<Job<'_>> {
    found
        .iter()
        .filter(|(name, _)| name == "ci.yml")
        .flat_map(|(_, text)| Workflow(text).jobs())
        .find(|job| job.name == SUITE_JOB)
}

/// Reads the manifest at `path` under the crate directory.
fn manifest(path: &str) -> std::io::Result<Manifest> {
    let text = manifest_dir()?.read_to_string(path)?;
    Manifest::parse(&text)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// Returns every workflow line that runs the suite, as
/// `(workflow, job, command)`.
fn suite_runs<'a>(
    found: &'a [(String, String)],
    goal: &'a str,
) -> Vec<(&'a str, &'a str, &'a str)> {
    found
        .iter()
        .flat_map(|(name, text)| {
            Workflow(text).jobs().into_iter().flat_map(move |job| {
                job.commands()
                    .filter(|command| command.runs_suite(goal))
                    .map(|command| (name.as_str(), job.name, command.text()))
                    .collect::<Vec<_>>()
            })
        })
        .collect()
}

#[rstest]
#[case::plain("make test", true)]
#[case::act_flag("make test WITH_ACT=1", true)]
#[case::make_option("make -j2 test", true)]
#[case::make_directory("make -C . test", true)]
#[case::make_all("make all", true)]
#[case::make_default_goal("make", true)]
#[case::make_coverage("make coverage", true)]
#[case::make_dev_test("make dev-test", true)]
#[case::quoted_target("make \"test\"", true)]
#[case::cargo("cargo test --all-features", true)]
#[case::cargo_config("cargo --config tools/dev-fast/config.toml test", true)]
#[case::cargo_toolchain("cargo +nightly test", true)]
#[case::nextest("cargo nextest run --all-targets", true)]
#[case::llvm_cov("cargo llvm-cov nextest --lcov", true)]
#[case::chained("set -eu && make test", true)]
#[case::unspaced("make lint&&make test", true)]
#[case::assignment("RUSTFLAGS=-Dwarnings cargo test", true)]
#[case::named_target("make test-workflow-contracts", false)]
#[case::lint("make lint", false)]
#[case::build("cargo build --all-targets", false)]
#[case::run_argument("cargo run -- test", false)]
#[case::echo("echo cargo test", false)]
#[case::step_item("- run: cargo test --all-features", true)]
#[case::env_wrapper("env RUSTFLAGS=x cargo test", true)]
#[case::timeout_wrapper("timeout 30m make test", true)]
#[case::bash_script("bash -c \"cargo test\"", true)]
#[case::sh_script("sh -c 'make test'", true)]
#[case::compound("if true; then cargo test; fi", true)]
#[case::shell_file("bash scripts/check.sh", false)]
#[case::nohup_wrapper("nohup cargo test", true)]
#[case::sudo_wrapper("sudo -u ci make test", true)]
#[case::shell_option_cluster("bash -lc 'make test'", true)]
#[case::trailing_comment("make test # the suite", true)]
#[case::quoted_separator("echo \"done; cargo test\"", false)]
#[case::commented_command("make lint # then make test", false)]
#[case::script_operand_only("bash -c 'cargo' test", false)]
#[case::make_dry_run("make -n", false)]
#[case::make_clustered_dry_run("make -ns", false)]
#[case::make_help("make --help", false)]
#[case::make_jobs("make -j4", true)]
#[case::make_jobs_number("make -j 4", true)]
#[case::make_load_number("make -l 4", true)]
#[case::make_jobs_target("make -j test", true)]
#[case::make_jobs_harmless_target("make -j lint", false)]
#[case::make_jobs_other_target("make -j 4 lint", false)]
#[case::escaped_quote_hides_separator("echo \"a \\\" ; make test\"", false)]
#[case::escaped_quote_then_suite("echo \"a \\\" b\" ; make test", true)]
#[case::escaped_backslash_closes("echo \"a \\\\\" ; make test", true)]
#[case::unterminated_quote_runs_on("echo \"make test", false)]
#[case::unterminated_quote_suite("make \"test", true)]
#[case::cargo_alias("cargo t", true)]
#[case::subshell("(cd crate && cargo test)", true)]
#[case::command_substitution("echo $(make test)", true)]
#[case::continuation_inside_a_word("cargo te\\\nst", true)]
#[case::continuation_after_a_space("make \\\ntest", true)]
#[case::continuation_joins_words("make\\\ntest", false)]
#[case::empty("", false)]
#[case::blank("   ", false)]
#[case::lone_separator("&", false)]
#[case::quoted_assignment("RUSTFLAGS='-D warnings' cargo test", true)]
#[case::commented_separator("echo ok # ; make test", false)]
#[case::clustered_shell_options("sh -ec 'make test'", true)]
#[case::command_lookup("command -v make", false)]
fn suite_spellings_are_recognized(#[case] line: &str, #[case] expected: bool) {
    assert_eq!(
        Command::from_line(line).runs_suite(SUITE_GOAL),
        expected,
        "misread {line:?}"
    );
}

#[test]
fn only_the_doctest_step_runs_the_suite_outside_coverage() {
    let found = workflows().expect("failed to read the workflows");
    let goal = default_goal().expect("failed to read the Makefile");
    let (doctests, repeated): (Vec<_>, Vec<_>) =
        suite_runs(&found, &goal)
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
    let job = suite_job(&found).expect("ci.yml must define build-test");
    assert!(
        !job.is_conditional(),
        "{SUITE_JOB} must run on every pull request"
    );
    let doctest = Command::from_line(DOCTEST_COMMAND);
    let steps: Vec<_> = job
        .steps()
        .into_iter()
        .filter(|step| step.runs(doctest))
        .collect();
    assert_eq!(
        steps.len(),
        1,
        "{SUITE_JOB} must run the doctests in one step"
    );
    assert!(
        !steps.iter().any(reading::Step::is_conditional),
        "the doctest step must always run"
    );
}

#[test]
fn build_test_runs_coverage_unconditionally() {
    let found = workflows().expect("failed to read the workflows");
    let job = suite_job(&found).expect("ci.yml must define build-test");
    let steps: Vec<_> = job
        .steps()
        .into_iter()
        .filter(|step| step.uses(COVERAGE_ACTION))
        .collect();
    assert_eq!(
        steps.len(),
        1,
        "{SUITE_JOB} must run the coverage action once"
    );
    assert!(
        !steps.iter().any(reading::Step::is_conditional),
        "coverage must always run"
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

#[rstest]
#[case::none("[package]\nname = \"x\"\n", &[])]
#[case::commented_table("[features] # flags\nextra = []\n", &["extra"])]
#[case::tabbed_optional("[dependencies]\nserde = { version = \"1\", optional\t=\ttrue }\n", &["serde"])]
#[case::target_optional("[target.'cfg(unix)'.dependencies]\nlibc = { version = \"1\", optional = true }\n", &["libc"])]
#[case::dep_syntax("[features]\nextra = [\"dep:serde\"]\n\n[dependencies]\nserde = { version = \"1\", optional = true }\n", &["extra"])]
#[case::required("[dependencies]\nserde = { version = \"1\" }\n", &[])]
fn manifest_features_are_read(#[case] text: &str, #[case] expected: &[&str]) {
    let parsed = Manifest::parse(text).expect("the fixture must parse");
    assert_eq!(parsed.features(), expected);
}

#[test]
fn the_crate_declares_no_features() {
    let features = manifest("Cargo.toml")
        .expect("failed to read Cargo.toml")
        .features();
    assert!(
        features.is_empty(),
        "Cargo.toml declares {features:?}; recheck make test against the coverage run"
    );
}

/// Commands that run the suite, for the bounded composition tests.
const SUITE_RUNS: [&str; 4] = ["make test", "make", "cargo test", "cargo nextest run"];

/// Commands that run nothing of the suite, for the same tests.
const HARMLESS: [&str; 3] = ["make lint", "echo ok", "cargo build"];

/// Every way one command can follow another on a `run:` body.
const JOINERS: [&str; 6] = [";", " ; ", "&&", " || ", " | ", "\n"];

/// Every prefix the reader must look through to the command behind it.
const PREFIXES: [&str; 6] = ["", "X=1 ", "env X=1 ", "timeout 5m ", "then ", "do "];

/// Returns each harmless command joined to each command behind each prefix.
fn compositions(commands: &[&str]) -> Vec<String> {
    HARMLESS
        .iter()
        .flat_map(|first| {
            JOINERS.iter().flat_map(move |joiner| {
                PREFIXES.iter().flat_map(move |prefix| {
                    commands
                        .iter()
                        .map(move |command| format!("{first}{joiner}{prefix}{command}"))
                })
            })
        })
        .collect()
}

/// Every suite run is found after any joiner and behind any prefix; the
/// inputs are enumerated exhaustively rather than sampled.
#[test]
fn a_suite_run_is_found_wherever_it_is_joined() {
    let missed: Vec<String> = compositions(&SUITE_RUNS)
        .into_iter()
        .filter(|line| !Command::from_line(line).runs_suite(SUITE_GOAL))
        .collect();
    assert!(missed.is_empty(), "suite runs missed: {missed:?}");
}

/// No harmless command reads as a suite run, however it is joined.
#[test]
fn nothing_is_found_in_harmless_commands() {
    let found: Vec<String> = compositions(&HARMLESS)
        .into_iter()
        .filter(|line| Command::from_line(line).runs_suite(SUITE_GOAL))
        .collect();
    assert!(
        found.is_empty(),
        "harmless commands read as suite runs: {found:?}"
    );
}

/// A bare `make` runs the suite only when the Makefile's default goal does.
#[rstest]
#[case::builds("build", false)]
#[case::runs_all("all", true)]
#[case::runs_test("test", true)]
fn a_bare_make_runs_the_default_goal(#[case] goal: &str, #[case] expected: bool) {
    assert_eq!(Command::from_line("make").runs_suite(goal), expected);
}

#[rstest]
#[case::first_rule(".PHONY: a\nbuild: x\nall: y\n", "build")]
#[case::assigned(".DEFAULT_GOAL := test\nbuild:\n", "test")]
#[case::conditional_assignment_changes_nothing(".DEFAULT_GOAL ?= test\nbuild:\n", "build")]
#[case::plain_assignment(".DEFAULT_GOAL = test\nbuild:\n", "test")]
#[case::last_assignment_wins(".DEFAULT_GOAL := first\n.DEFAULT_GOAL := second\nbuild:\n", "second")]
#[case::conditional_keeps_the_first(
    ".DEFAULT_GOAL := first\n.DEFAULT_GOAL ?= second\nbuild:\n",
    "first"
)]
#[case::conditional_then_set(".DEFAULT_GOAL ?= second\n.DEFAULT_GOAL := first\nbuild:\n", "first")]
#[case::a_later_change_to_test(".DEFAULT_GOAL := build\nlint:\n.DEFAULT_GOAL := test\n", "test")]
#[case::an_empty_value_clears_it(".DEFAULT_GOAL := first\n.DEFAULT_GOAL :=\nbuild:\n", "build")]
#[case::several_words_are_refused(
    ".DEFAULT_GOAL := first\n.DEFAULT_GOAL += second\nbuild:\n",
    "build"
)]
#[case::spaced_operator(".DEFAULT_GOAL   :=   spaced\nbuild:\n", "spaced")]
#[case::appending_to_nothing_sets_it(".DEFAULT_GOAL += test\nbuild:\n", "test")]
#[case::recipe_text_is_not_an_assignment("first:\n\t.DEFAULT_GOAL = test\n", "first")]
#[case::comments_are_skipped("# build: not a rule\nrun: z\n", "run")]
#[case::special_targets_are_skipped(".PHONY: a\n.SUFFIXES:\nrun: z\n", "run")]
#[case::skips_variables_and_patterns("X := 1\n\tfoo: bar\n%.o: %.c\nrun: z\n", "run")]
#[case::none("X := 1\n", "")]
fn the_default_goal_is_read(#[case] makefile: &str, #[case] expected: &str) {
    assert_eq!(default_goal_of(makefile), expected);
}

#[rstest]
#[case::step_key("      - name: a\n        if: x\n        run: y\n", true)]
#[case::item_key("      - if: x\n        run: y\n", true)]
#[case::under_with("      - uses: a\n        with:\n          if: x\n", false)]
#[case::in_run_body("      - run: |\n          if: x\n", false)]
fn a_step_condition_is_read_at_step_level(#[case] steps: &str, #[case] expected: bool) {
    let text = format!("jobs:\n  j:\n    steps:\n{steps}");
    let job = Workflow(&text).jobs().pop().expect("one job");
    let step = job.steps().pop().expect("one step");
    assert_eq!(step.is_conditional(), expected);
}

/// Every wrapper is looked through, with an option of its own, to the
/// command it runs.
#[rstest]
#[case::env("env -u HOME")]
#[case::timeout("timeout -s KILL 5m")]
#[case::nice("nice -n 5")]
#[case::command("command")]
#[case::exec("exec -a name")]
#[case::time("time")]
#[case::nohup("nohup")]
#[case::setsid("setsid")]
#[case::stdbuf("stdbuf -oL")]
#[case::sudo("sudo -u ci")]
fn every_wrapper_is_looked_through(#[case] prefix: &str) {
    let runs =
        |command: &str| Command::from_line(&format!("{prefix} {command}")).runs_suite(SUITE_GOAL);
    assert!(runs("make test"), "{prefix} hid a suite run");
    assert!(
        !runs("make lint"),
        "{prefix} made a harmless command a suite run"
    );
}

/// A job's `if:` is read at the job's own level, not from a step or a
/// nested mapping.
#[rstest]
#[case::job_condition(
    "  a:\n    if: github.event_name == 'push'\n    steps:\n      - run: y\n",
    true
)]
#[case::step_condition("  a:\n    steps:\n      - if: x\n        run: y\n", false)]
#[case::nested_mapping("  a:\n    env:\n      if: x\n    steps:\n      - run: y\n", false)]
fn a_job_condition_is_read_at_job_level(#[case] body: &str, #[case] expected: bool) {
    let text = format!("jobs:\n{body}");
    let jobs = Workflow(&text).jobs();
    assert_eq!(jobs.len(), 1);
    let job = jobs.first().expect("one job");
    assert_eq!(job.is_conditional(), expected);
}

#[test]
fn a_malformed_manifest_is_an_error() {
    assert!(Manifest::parse("[features\nextra = []\n").is_err());
}

/// Jobs are found by indentation, whatever its width, and a comment at column
/// zero between jobs neither ends the section nor hides a later job.
#[rstest]
#[case::two_spaces(
    "jobs:\n  a:\n    steps:\n      - run: x\n  b:\n    steps:\n      - run: y\n",
    &["a", "b"]
)]
#[case::four_spaces(
    "jobs:\n    a:\n        steps:\n            - run: x\n    b:\n        steps:\n            - run: y\n",
    &["a", "b"]
)]
#[case::comment_between(
    "jobs:\n  a:\n    steps:\n      - run: x\n# note\n  b:\n    steps:\n      - run: y\n",
    &["a", "b"]
)]
#[case::one_job("jobs:\n  only:\n    steps:\n      - run: x\n", &["only"])]
#[case::none("name: x\n", &[])]
fn jobs_are_found_by_indentation(#[case] text: &str, #[case] expected: &[&str]) {
    let jobs = Workflow(text).jobs();
    let names: Vec<&str> = jobs.iter().map(|job| job.name).collect();
    assert_eq!(names, expected);
}

/// A step is a list item at the step list's own indentation: a list nested
/// under one of its keys, or a `- ` line inside a multiline `run:` block, does
/// not split it.
#[rstest]
#[case::plain("      - run: x\n      - run: y\n", 2)]
#[case::nested_list(
    "      - name: n\n        with:\n          items:\n            - one\n            - two\n        run: x\n      - run: y\n",
    2
)]
#[case::dash_in_run_block(
    "      - run: |\n          - not a step\n          make test\n      - run: y\n",
    2
)]
#[case::one_step("      - run: x\n", 1)]
fn steps_are_split_at_the_step_list_level(#[case] steps: &str, #[case] expected: usize) {
    let text = format!("jobs:\n  a:\n    steps:\n{steps}");
    let jobs = Workflow(&text).jobs();
    let job = jobs.first().expect("one job");
    assert_eq!(job.steps().len(), expected);
}

/// A command inside a `run:` block is read as a command of the job, so a suite
/// run behind `set -eu` is seen.
#[test]
fn a_run_block_line_is_a_command_of_its_job() {
    let text = "jobs:\n  a:\n    steps:\n      - run: |\n          set -eu\n          make test\n";
    let jobs = Workflow(text).jobs();
    let job = jobs.first().expect("one job");
    assert!(job.commands().any(|command| command.runs_suite(SUITE_GOAL)));
}

/// Every make target the reader counts as running the suite.
#[rstest]
#[case::make_test("test")]
#[case::make_all("all")]
#[case::make_coverage("coverage")]
#[case::make_dev_test("dev-test")]
#[case::make_test_fast("test-fast")]
fn every_suite_target_runs_the_suite(#[case] target: &str) {
    assert!(Command::from_line(&format!("make {target}")).runs_suite("build"));
    assert!(Command::from_line(&format!("make -j 4 {target}")).runs_suite("build"));
    assert!(!Command::from_line(&format!("make {target}-not")).runs_suite("build"));
}

/// Every make option that reads or describes the makefile without running a
/// goal, alone and beside a suite target.
#[rstest]
#[case::just_print("--just-print")]
#[case::dry_run("--dry-run")]
#[case::recon("--recon")]
#[case::short_dry_run("-n")]
#[case::question("--question")]
#[case::short_question("-q")]
#[case::help("--help")]
#[case::version("--version")]
#[case::short_version("-v")]
#[case::short_help("-h")]
#[case::clustered("-ns")]
fn every_inert_make_option_runs_no_goal(#[case] option: &str) {
    assert!(!Command::from_line(&format!("make {option}")).runs_suite("all"));
    assert!(!Command::from_line(&format!("make {option} test")).runs_suite("all"));
    assert!(!Command::from_line(&format!("make test {option}")).runs_suite("all"));
}

/// `command -v` and `command -V` describe a command and run nothing.
#[rstest]
#[case::lower("command -v make test")]
#[case::upper("command -V make test")]
#[case::upper_bare("command -V make")]
fn command_lookup_runs_nothing(#[case] line: &str) {
    assert!(!Command::from_line(line).runs_suite("all"));
}

/// Returns what GNU make itself takes as the default goal of `makefile`, or
/// `None` when make refuses it. `make -pn` prints the variable database
/// without running a recipe, and `.DEFAULT_GOAL` is the value make settled on
/// after reading every assignment (GNU make manual, "Other Special Variables").
#[cfg(target_os = "linux")]
fn make_default_goal(makefile: &str) -> std::io::Result<Option<String>> {
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
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("make's stdin was not piped"))?
        .write_all(makefile.as_bytes())?;
    let output = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(output.status.success().then_some(()).and_then(|()| {
        text.lines().find_map(|line| {
            let (name, value) = line.split_once(" = ").or_else(|| line.split_once(" := "))?;
            (name == ".DEFAULT_GOAL").then(|| value.to_owned())
        })
    }))
}

/// Returns whether the `make` on `PATH` is GNU make, saying why not on stderr
/// when it is not, so a host without it skips the make-backed tests visibly
/// instead of failing on a missing binary.
#[cfg(target_os = "linux")]
fn gnu_make_is_available() -> bool {
    let is_gnu = std::process::Command::new("make")
        .arg("--version")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("GNU Make"));
    if !is_gnu {
        // Written to stderr directly: the reason must be visible in a run
        // with `--nocapture`, and `eprintln!` is denied here.
        std::io::Write::write_all(
            &mut std::io::stderr(),
            b"skipped: GNU make is not on PATH, so the reader cannot be pinned to it\n",
        )
        .ok();
    }
    is_gnu
}

/// The reader agrees with GNU make on every Makefile it can be run on, so the
/// behaviour is pinned to make and not to anyone's reading of its manual.
#[cfg(target_os = "linux")]
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
#[case::comments_are_skipped("# build: not a rule\nrun:\n")]
#[case::special_targets_are_skipped(".PHONY: a\n.SUFFIXES:\nrun:\n")]
#[case::recipe_text_is_not_an_assignment("first:\n\t@: .DEFAULT_GOAL = test\nsecond:\n")]
fn the_reader_agrees_with_gnu_make(#[case] makefile: &str) {
    if !gnu_make_is_available() {
        return;
    }
    let by_make = make_default_goal(makefile)
        .expect("make must run")
        .expect("make must accept the fixture");
    assert_eq!(default_goal_of(makefile), by_make, "{makefile:?}");
}

/// Make refuses a default goal of several targets; the reader does not read
/// such a value and falls back to the first rule.
#[cfg(target_os = "linux")]
#[test]
fn make_refuses_several_words_and_the_reader_does_not_read_them() {
    if !gnu_make_is_available() {
        return;
    }
    let makefile = ".DEFAULT_GOAL := first\n.DEFAULT_GOAL += second\nbuild:\nfirst:\nsecond:\n";
    assert_eq!(make_default_goal(makefile).expect("make must run"), None);
    assert_eq!(default_goal_of(makefile), "build");
}
