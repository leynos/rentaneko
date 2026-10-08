//! The make-backed half of the suite-once contract: the reader's default goal
//! is compared with the one GNU make itself settles on, so the behaviour is
//! pinned to make and not to anyone's reading of its manual.
//!
//! Linux only, where GNU make is the make in use. The tests skip, writing the
//! reason to stderr, on a host where `make` is absent or is not GNU make. This
//! module holds the make comparison (the probe and the pure `disagreement`
//! decision); availability checks and skip reporting live in `make_skip`, which
//! it uses.

use rstest::rstest;

use super::{make_skip::stop_without_gnu_make, reading::default_goal_of};

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
    make_default_goal_in(makefile, &[])
}

/// Make's own flag variables, which the probe must not inherit: a `-q` in any
/// of them makes make exit non-zero when a target needs updating, which would
/// read as a refused fixture.
pub(super) const INHERITED_FLAGS: [&str; 3] = ["MAKEFLAGS", "GNUMAKEFLAGS", "MAKELEVEL"];

/// Runs the probe with `environment` set on the child first, then removes
/// [`INHERITED_FLAGS`], so a test can prove an inherited flag has no effect.
/// The environment is the child's alone; the test process is never mutated.
///
/// # Errors
///
/// Returns the error raised starting `make` or feeding it the fixture.
fn make_default_goal_in(
    makefile: &str,
    environment: &[(&str, &str)],
) -> std::io::Result<GoalProbe> {
    use std::{
        io::Write as _,
        process::{Command as Process, Stdio},
    };

    let mut command = Process::new("make");
    command
        .args(["-f", "-", "-pn"])
        .envs(environment.iter().copied());
    for flag in INHERITED_FLAGS {
        command.env_remove(flag);
    }
    let mut child = command
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

/// Returns a note where the reader and make disagree about a fixture, else
/// `None`. `goal` is what make settled on, or `None` where it refused;
/// `reference` is the reference fold's goal for the same assignments;
/// `by_reader` is what the reader returned; `diagnostic` is make's stderr.
///
/// A refusal is only expected where the reference fold also reads no single
/// goal (several words), and then the reader must fall back to `build`. A
/// refusal in any other shape means the reader or the probe is wrong, which is
/// reachable only through such a bug, so the shapes are tested directly below.
fn disagreement(
    goal: Option<&str>,
    reference: Option<&str>,
    by_reader: &str,
    diagnostic: &str,
) -> Option<String> {
    match goal {
        Some(by_make) if by_reader != by_make => {
            Some(format!("make says {by_make}, the reader says {by_reader}"))
        }
        Some(_) => None,
        None if reference.is_none() && by_reader == "build" => None,
        None => Some(format!(
            "make refused a fixture the reference accepts or the reader misreads: {diagnostic}"
        )),
    }
}

/// The reader agrees with GNU make over every sequence of up to three
/// `.DEFAULT_GOAL` assignments, so the behaviour is pinned to make exhaustively
/// and not to a sample. A sequence make refuses must be one the reference fold
/// also reads as having no single goal; any other refusal is a disagreement.
#[test]
fn the_reader_agrees_with_gnu_make_on_every_bounded_sequence() {
    if stop_without_gnu_make().expect("stderr must be writable") {
        return;
    }
    let mut disagreements = Vec::new();
    for operations in super::properties::sequences(3) {
        let text = super::properties::makefile(&operations);
        let probe = make_default_goal(&text).expect("make must run");
        let reference = super::properties::reference(&operations);
        if let Some(note) = disagreement(
            probe.goal.as_deref(),
            reference.as_deref(),
            &default_goal_of(&text),
            &probe.diagnostic,
        ) {
            disagreements.push((text, note));
        }
    }
    assert!(
        disagreements.is_empty(),
        "the reader disagrees with make on {:?}",
        disagreements.iter().take(3).collect::<Vec<_>>()
    );
}

/// The decision is exercised directly, without the reader or make, so each
/// shape it can meet is held, including the refusals only a bug could produce.
#[rstest]
#[case::agree(Some("a"), Some("a"), "a", None)]
#[case::differ(Some("a"), Some("a"), "b", Some("make says a, the reader says b"))]
#[case::expected_refusal(None, None, "build", None)]
#[case::refusal_the_reference_accepts(
    None,
    Some("a"),
    "a",
    Some("make refused a fixture the reference accepts or the reader misreads: why")
)]
#[case::refusal_the_reference_accepts_with_a_build_fallback(
    None,
    Some("a"),
    "build",
    Some("make refused a fixture the reference accepts or the reader misreads: why")
)]
#[case::refusal_with_the_wrong_fallback(
    None,
    None,
    "test",
    Some("make refused a fixture the reference accepts or the reader misreads: why")
)]
fn a_disagreement_is_named_for_every_shape(
    #[case] goal: Option<&str>,
    #[case] reference: Option<&str>,
    #[case] by_reader: &str,
    #[case] expected: Option<&str>,
) {
    assert_eq!(
        disagreement(goal, reference, by_reader, "why").as_deref(),
        expected
    );
}

/// A `-q` in make's flag variables must not change what the probe reads: with
/// it, make would exit non-zero for a target that needs updating and the
/// fixture would read as refused.
#[rstest]
#[case::makeflags("MAKEFLAGS")]
#[case::gnumakeflags("GNUMAKEFLAGS")]
fn an_inherited_question_flag_does_not_hide_the_goal(#[case] variable: &str) {
    if stop_without_gnu_make().expect("stderr must be writable") {
        return;
    }
    let makefile = "build:\n\t@echo built\ntest:\n";
    let probe = make_default_goal_in(makefile, &[(variable, "-q")]).expect("make must run");
    assert_eq!(probe.goal.as_deref(), Some("build"), "{}", probe.diagnostic);
}
