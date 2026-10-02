//! Exhaustive bounded checks over the suite reader, standing in for generated
//! property tests: every combination of the vocabulary the reader promises to
//! read is enumerated, which is stronger than sampling it.
//!
//! The vocabularies are small and closed (wrappers, joiners, quote
//! characters, `.DEFAULT_GOAL` operators), so enumeration is complete and needs
//! no generator, no shrinking and no extra dependency. The comparison with real
//! GNU make over the same assignment sequences is in `make_probe.rs`.

use super::{
    SUITE_GOAL,
    reading::{Command, default_goal_of},
};

/// Commands that run the suite.
const SUITE: [&str; 7] = [
    "make test",
    "make all",
    "make",
    "cargo test",
    "cargo nextest run",
    "make coverage",
    "bash -c 'cargo test'",
];

/// Commands that run nothing of the suite.
const HARMLESS: [&str; 5] = [
    "make lint",
    "echo ok",
    "cargo build",
    "make -n test",
    "make --help",
];

/// Every way one command can follow another.
const JOINERS: [&str; 7] = [";", " ; ", "&&", " || ", " | ", "\n", "&"];

/// Every prefix the reader looks through to the command behind it.
const PREFIXES: [&str; 9] = [
    "",
    "X=1 ",
    "env X=1 ",
    "timeout 5m ",
    "then ",
    "do ",
    "nohup ",
    "sudo -u ci ",
    "command ",
];

/// Returns every `first JOINER PREFIX second` line over the given lists.
fn lines(firsts: &[&str], seconds: &[&str]) -> Vec<String> {
    let joined: Vec<(&str, &str)> = JOINERS
        .iter()
        .flat_map(|joiner| PREFIXES.iter().map(move |prefix| (*joiner, *prefix)))
        .collect();
    firsts
        .iter()
        .flat_map(|first| {
            joined.iter().flat_map(move |(joiner, prefix)| {
                seconds
                    .iter()
                    .map(move |second| format!("{first}{joiner}{prefix}{second}"))
            })
        })
        .collect()
}

/// Returns `true` if the line is read as running the suite.
fn runs(line: &str) -> bool { Command::from_line(line).runs_suite(SUITE_GOAL) }

#[test]
fn a_suite_run_is_found_after_every_harmless_command_joiner_and_prefix() {
    let missed: Vec<String> = lines(&HARMLESS, &SUITE)
        .into_iter()
        .filter(|line| !runs(line))
        .collect();
    assert!(missed.is_empty(), "suite runs missed: {missed:?}");
}

#[test]
fn two_harmless_commands_are_never_a_suite_run() {
    let found: Vec<String> = lines(&HARMLESS, &HARMLESS)
        .into_iter()
        .filter(|line| runs(line))
        .collect();
    assert!(
        found.is_empty(),
        "harmless lines read as suite runs: {found:?}"
    );
}

/// Every string of up to `length` characters over `alphabet`.
fn strings(alphabet: &[char], length: usize) -> Vec<String> {
    let mut found = vec![String::new()];
    let mut frontier = vec![String::new()];
    for _ in 0..length {
        frontier = frontier
            .iter()
            .flat_map(|prefix| alphabet.iter().map(move |c| format!("{prefix}{c}")))
            .collect();
        found.extend(frontier.iter().cloned());
    }
    found
}

#[test]
fn single_quoted_text_never_adds_a_suite_run() {
    // A single-quoted string is one word to the shell, so
    // `echo '<anything but a quote>'` runs only `echo`.
    let alphabet = ['a', ' ', ';', '&', '|', '"', '\\', '\n', '#', '$'];
    let found: Vec<String> = strings(&alphabet, 3)
        .into_iter()
        .map(|text| format!("echo '{text} make test {text}'"))
        .filter(|line| runs(line))
        .collect();
    assert!(
        found.is_empty(),
        "quoted text read as a suite run: {found:?}"
    );
}

/// One `.DEFAULT_GOAL` operation: an operator and a value.
type Operation = (&'static str, &'static str);

/// Every operation the sequences are built from.
fn operations() -> Vec<Operation> {
    let mut found = Vec::new();
    for operator in [":=", "=", "?=", "+="] {
        for value in ["", "a", "test"] {
            found.push((operator, value));
        }
    }
    found
}

/// Every sequence of up to `length` operations.
pub(super) fn sequences(length: usize) -> Vec<Vec<Operation>> {
    let mut found = vec![Vec::new()];
    let mut frontier: Vec<Vec<Operation>> = vec![Vec::new()];
    for _ in 0..length {
        frontier = frontier
            .iter()
            .flat_map(|prefix| {
                operations().into_iter().map(move |operation| {
                    let mut next = prefix.clone();
                    next.push(operation);
                    next
                })
            })
            .collect();
        found.extend(frontier.iter().cloned());
    }
    found
}

/// Returns a Makefile that applies the operations and then declares its rules.
pub(super) fn makefile(operations: &[Operation]) -> String {
    let assignments = operations
        .iter()
        .map(|(operator, value)| format!(".DEFAULT_GOAL {operator} {value}\n"))
        .collect::<Vec<_>>()
        .concat();
    format!("{assignments}build:\na:\nb:\ntest:\n")
}

/// Returns `.DEFAULT_GOAL` after the operations, applied as make applies them,
/// or `None` where it holds several words.
fn reference(operations: &[Operation]) -> Option<String> {
    let mut goal = String::new();
    for (operator, value) in operations {
        match *operator {
            ":=" | "=" => (*value).clone_into(&mut goal),
            "+=" => format!("{goal} {value}").trim().clone_into(&mut goal),
            _ => {}
        }
    }
    (goal.split_whitespace().count() == 1).then_some(goal)
}

#[test]
fn the_default_goal_is_what_every_bounded_sequence_leaves() {
    let disagreements: Vec<String> = sequences(3)
        .into_iter()
        .filter_map(|operations| {
            let expected = reference(&operations).unwrap_or_else(|| "build".to_owned());
            let text = makefile(&operations);
            (default_goal_of(&text) != expected).then_some(text)
        })
        .collect();
    assert!(
        disagreements.is_empty(),
        "the reader disagrees with the reference fold on {:?}",
        disagreements.iter().take(3).collect::<Vec<_>>()
    );
}
