//! Cases for the suite-once contract's reader: which command lines run the
//! suite however they are joined, wrapped or spelled, which default goal a
//! Makefile gives a bare `make`, and how workflow jobs, steps and manifests are
//! read.
//!
//! These drive the readers with constructed text, because a rule checked only
//! against workflows that already conform passes whether or not it
//! discriminates. They live apart from the contract so each file stays under the
//! repository's 400-line limit.

use rstest::rstest;

use super::{
    SUITE_GOAL,
    reading::{Command, Manifest, Workflow, default_goal_of},
};

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
#[case::an_empty_append_keeps_it(".DEFAULT_GOAL := first\n.DEFAULT_GOAL +=\nbuild:\n", "first")]
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
