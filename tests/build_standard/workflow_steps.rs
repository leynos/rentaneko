//! The CI workflow and recipe clauses of the build-standard contract: the
//! doctest step restates both flags, a step reader stays inside its own step,
//! the coverage recipe selects LLVM, and a `--target` in the flag variables,
//! which Make cannot read, is an error rather than an assumed Linux target. It is
//! a module of `build_standard_contract.rs`, so it shares that file's readers.

use std::process::{Command, Output};

use cap_std::{ambient_authority, fs::Dir};

use super::{Flags, Host, MOLD_FLAG, THREADS_FLAG, check_development_targets, dry_run};

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
    let stdout = dry_run("coverage", Host::Linux, None).expect("run `make -n coverage`");
    let line = stdout
        .lines()
        .find(|line| line.contains("llvm-cov"))
        .expect("`make coverage` runs cargo llvm-cov");
    assert!(
        line.contains("CARGO_PROFILE_DEV_CODEGEN_BACKEND=llvm"),
        "the coverage command does not select LLVM: {line}"
    );
}

/// Runs `make -n TARGET` as `host` with `variable` set to `value`.
fn dry_run_with(host: &str, target: &str, variable: &str, value: &str) -> std::io::Result<Output> {
    Command::new("make")
        .args(["-n", "-B", &format!("BUILD_HOST_OS={host}"), target])
        .arg(format!("{variable}={value}"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
}

/// A `--target` in any flag variable, in either spelling, stops each development
/// target with a message naming `CARGO_BUILD_TARGET`, on a Linux host and on a
/// macOS host alike: the guard does not depend on whether mold would apply.
#[test]
fn a_target_in_the_flag_variables_is_an_error() {
    for host in ["Linux", "Darwin"] {
        for (target, variable, value) in [
            ("typecheck", "CARGO_FLAGS", "--target aarch64-apple-darwin"),
            ("test", "TEST_FLAGS", "--target=aarch64-apple-darwin"),
            (
                "lint",
                "CLIPPY_FLAGS",
                "--all-targets --target aarch64-apple-darwin",
            ),
            ("build", "CARGO_FLAGS", "--target=x86_64-unknown-linux-gnu"),
        ] {
            let output = dry_run_with(host, target, variable, value).expect("run `make -n`");
            assert!(
                !output.status.success(),
                "`make {target} {variable}='{value}'` was accepted on {host}"
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("set CARGO_BUILD_TARGET instead"),
                "`make {target}` on {host} did not name CARGO_BUILD_TARGET: {stderr}"
            );
        }
    }
}

/// Flags that merely contain the word are not a target selection, so the guard
/// does not reject `--target-dir` or `--all-targets`.
#[test]
fn flags_that_only_resemble_a_target_are_accepted() {
    for (variable, value) in [
        ("CARGO_FLAGS", "--all-targets --target-dir /tmp/x"),
        ("TEST_FLAGS", "--all-targets --all-features"),
    ] {
        let output = dry_run_with("Linux", "typecheck", variable, value).expect("run `make -n`");
        assert!(
            output.status.success(),
            "`{variable}='{value}'` was rejected: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Complete flags a caller might export: each is one or more words that stand
/// together, so any sequence of them is a valid `RUSTFLAGS`.
const CALLER_FLAGS: [&str; 5] = [
    "--cfg one",
    "--cfg feature=\"x\"",
    "-C target-cpu=native",
    "-C link-arg=-Wl,--gc-sections",
    "-D warnings",
];

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(24))]

    /// For any sequence of caller flags (including none, repeats, and `-C value`
    /// pairs), every development command assigns a `RUSTFLAGS` that begins with
    /// the caller's words in order, before the flags the recipe appends, and
    /// still carries the frontend flag.
    #[test]
    fn development_targets_lead_with_any_inherited_sequence(
        flags in proptest::collection::vec(proptest::sample::select(CALLER_FLAGS.to_vec()), 0..4)
    ) {
        let caller = flags.join(" ");
        let problems = check_development_targets(Host::Linux, Some(&caller))
            .expect("read `make -n` output");
        proptest::prop_assert!(problems.is_empty(), "inherited {caller:?}: {problems:#?}");
    }
}

/// The order check itself: a list leads with the caller's words only when they
/// come first and in order, so a recipe that appends its flags before the
/// inherited ones, or reorders them, fails even though every word is present.
#[test]
fn leading_with_the_callers_flags_requires_them_first_and_in_order() {
    let assigned = Flags::from_words(&["--cfg", "a", "-C", "x=1", "-Zthreads=8"]);
    assert!(assigned.leads_with("--cfg a -C x=1"));
    assert!(assigned.leads_with(""));
    assert!(!assigned.leads_with("-C x=1"), "a later run is not a lead");
    assert!(!assigned.leads_with("-C x=1 --cfg a"), "order matters");
    let appended_first = Flags::from_words(&["-Zthreads=8", "--cfg", "a"]);
    assert!(!appended_first.leads_with("--cfg a"));
}
