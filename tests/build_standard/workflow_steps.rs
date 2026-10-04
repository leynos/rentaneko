//! The CI workflow and coverage-recipe clauses of the build-standard contract:
//! the doctest step restates both flags, a step reader stays inside its own
//! step, and the coverage recipe selects LLVM. It is a module of
//! `build_standard_contract.rs`, so it shares that file's readers and constants.

use std::process::Command;

use cap_std::{ambient_authority, fs::Dir};

use super::{Flags, MOLD_FLAG, THREADS_FLAG};

/// Returns the `RUSTFLAGS` a named workflow step assigns.
///
/// The step runs from its `- name:` line to the next line indented no deeper
/// than that list item, so a re-indented workflow, or a later step that also
/// assigns `RUSTFLAGS`, cannot lend the step flags it does not carry.
fn step_rustflags(workflow: &str, name: &str) -> Option<String> {
    let marker = format!("- name: {name}");
    let mut lines = workflow.lines().skip_while(|line| line.trim() != marker);
    let item_indent = {
        let first = lines.next()?;
        first.len() - first.trim_start().len()
    };
    lines
        .take_while(|line| {
            line.trim().is_empty() || line.len() - line.trim_start().len() > item_indent
        })
        .find_map(|line| line.trim().strip_prefix("RUSTFLAGS:"))
        .map(|value| value.trim().to_owned())
}

/// A later step assigning `RUSTFLAGS` must not be read as the named step's.
#[test]
fn a_step_does_not_borrow_a_later_steps_rustflags() {
    let workflow = "  steps:\n    - name: First\n      run: true\n    - name: Second\n      \
                    env:\n        RUSTFLAGS: -Zthreads=8\n";
    assert_eq!(step_rustflags(workflow, "First"), None);
    assert_eq!(
        step_rustflags(workflow, "Second").as_deref(),
        Some("-Zthreads=8")
    );
}

/// CI runs the doctests directly, outside `make`, so the step's own assignment
/// is the only thing that keeps the standard's flags on that build: the toolchain
/// action's `RUSTFLAGS` would otherwise replace every configuration source.
#[test]
fn the_ci_doctest_step_restates_both_flags() {
    let root = Dir::open_ambient_dir(env!("CARGO_MANIFEST_DIR"), ambient_authority())
        .expect("open the manifest directory");
    let workflow = root
        .read_to_string(".github/workflows/ci.yml")
        .expect("read ci.yml");
    let value =
        step_rustflags(&workflow, "Run doctests").expect("the doctest step assigns RUSTFLAGS");
    let words: Vec<String> = value.split_whitespace().map(str::to_owned).collect();
    let flags = Flags::from_words(&words);
    assert!(
        flags.names(THREADS_FLAG),
        "the doctest step drops {THREADS_FLAG}: {flags:?}"
    );
    assert!(
        flags.names(MOLD_FLAG),
        "the doctest step drops {MOLD_FLAG}: {flags:?}"
    );
}

/// Instrumentation needs LLVM, and the development profile defaults to
/// Cranelift, so the coverage recipe must select LLVM on the command itself.
#[test]
fn the_coverage_recipe_selects_llvm() {
    let output = Command::new("make")
        .args(["-n", "-B", "BUILD_HOST_OS=Linux", "coverage"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run `make -n coverage`");
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\\\n", " ");
    let line = stdout
        .lines()
        .find(|line| line.contains("llvm-cov"))
        .expect("`make coverage` runs cargo llvm-cov");
    assert!(
        line.contains("CARGO_PROFILE_DEV_CODEGEN_BACKEND=llvm"),
        "the coverage command does not select LLVM: {line}"
    );
}
