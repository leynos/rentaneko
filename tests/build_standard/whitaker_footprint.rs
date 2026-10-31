//! The Whitaker and footprint clauses of the build-standard contract: the Whitaker route runs from
//! a clean environment instead of restating the development flags, `make lint` still runs Clippy
//! and Whitaker, and the development footprint settings stay in `Cargo.toml` and
//! `.cargo/config.toml`. It is a module of `build_standard_contract.rs`, so it shares that file's
//! readers.

use cap_std::{ambient_authority, fs::Dir};

use super::{Host, dry_run};

/// Returns whether a word names a program: its final path component matches.
fn names_program(word: &str, program: &str) -> bool {
    word.rsplit('/').next().is_some_and(|name| name == program)
}

/// Splits the text after an `=` into a quoted or bare value and the rest.
fn split_value(after: &str) -> (&str, &str) {
    let quoted = |quote: char| {
        after
            .strip_prefix(quote)
            .map(|body| body.split_once(quote).unwrap_or((body, "")))
    };
    quoted('"')
        .or_else(|| quoted('\''))
        .unwrap_or_else(|| after.split_once(char::is_whitespace).unwrap_or((after, "")))
}

/// Returns the command line after its leading `NAME=value` assignments.
fn after_assignments(command: &str) -> &str {
    let mut rest = command.trim_start();
    while let Some((name, after)) = rest.split_once('=') {
        let is_name = name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !is_name {
            break;
        }
        rest = split_value(after).1.trim_start();
    }
    rest
}

/// Skips the options and assignments an `env` wrapper takes before the program it runs.
fn skip_env_arguments(mut rest: &str) -> &str {
    loop {
        if let Some(after) = rest.strip_prefix("-i ") {
            rest = after.trim_start();
        } else if let Some(after) = rest.strip_prefix("-u ") {
            rest = after
                .trim_start()
                .split_once(char::is_whitespace)
                .map_or("", |(_, tail)| tail)
                .trim_start();
        } else {
            let stripped = after_assignments(rest);
            if stripped.len() == rest.len() {
                return rest;
            }
            rest = stripped;
        }
    }
}

/// Returns the program a shell command executes: its first word after any leading assignments and
/// any `env` wrapper with its `-i` and `-u NAME` options and its own assignments. A word that only
/// appears as an argument (`echo whitaker --all`) is not the program.
fn executed_program(command: &str) -> Option<&str> {
    let mut rest = after_assignments(command.trim());
    loop {
        let (word, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if word.is_empty() {
            return None;
        }
        if !names_program(word, "env") {
            return Some(word);
        }
        rest = skip_env_arguments(after.trim_start());
    }
}

/// The inherited settings the Whitaker route must clear, so its installer-managed toolchain builds
/// with Cargo's repository defaults: flags, encoded flags, the build table's flags, backend,
/// target, the linker and rustflags of each supported host's target, and the C compiler's flags.
const WHITAKER_CLEARED: [&str; 13] = [
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "CARGO_PROFILE_DEV_CODEGEN_BACKEND",
    "CARGO_BUILD_TARGET",
    "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER",
    "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
    "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER",
    "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS",
    "CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER",
    "CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS",
    "CFLAGS",
    "LDFLAGS",
];

/// Returns the complaints about the `make -n lint-whitaker` output of one host. The command is the
/// line whose executed program is Whitaker (after assignments and the `env` wrapper), so a line
/// that only prints its name does not count; it must clear every inherited setting, assign no
/// flag, backend or profile in any spelling, pass the warning policy through Dylint's input and
/// keep the target and feature selections as words, since other Cargo options may follow them.
fn whitaker_route_problems(recipe: &str, host: &str) -> Vec<String> {
    let Some(whitaker) = recipe
        .lines()
        .find(|line| executed_program(line).is_some_and(|word| names_program(word, "whitaker")))
    else {
        return vec![format!(
            "`make lint-whitaker` on {host} must execute the Whitaker program: {recipe}"
        )];
    };
    let mut problems = Vec::new();
    for setting in WHITAKER_CLEARED {
        if !whitaker.contains(&format!("-u {setting} ")) {
            problems.push(format!(
                "Whitaker on {host} must remove inherited {setting}: {whitaker}"
            ));
        }
    }
    // Any assignment form replaces the defaults: empty, bare, single or double quoted.
    let assigns_flags = whitaker.split_whitespace().any(|word| {
        word.starts_with("RUSTFLAGS=")
            || word.starts_with("CARGO_ENCODED_RUSTFLAGS=")
            || word.starts_with("CARGO_BUILD_RUSTFLAGS=")
            || (word.starts_with("CARGO_PROFILE_") && word.contains("_CODEGEN_BACKEND="))
    });
    if assigns_flags || whitaker.contains("=llvm") {
        problems.push(format!(
            "Whitaker must not replace Cargo defaults with assigned flags or LLVM: {whitaker}"
        ));
    }
    if !whitaker.contains("DYLINT_RUSTFLAGS=\"-D warnings") {
        problems.push(format!(
            "warnings must reach the check through Dylint's input: {whitaker}"
        ));
    }
    for option in ["--all-targets", "--all-features"] {
        if !whitaker.split_whitespace().any(|word| word == option) {
            problems.push(format!("the check must keep {option}: {whitaker}"));
        }
    }
    problems
}

/// Returns the complaints about the `make -n lint` output: one command executes Cargo with the
/// `clippy` word and another executes Whitaker, so isolating the Whitaker route cannot drop either
/// from the gate, and a recipe that only prints their names runs neither.
fn lint_problems(recipe: &str) -> Vec<String> {
    let runs = |program: &str, subcommand: Option<&str>| {
        recipe.lines().any(|line| {
            executed_program(line).is_some_and(|word| names_program(word, program))
                && subcommand.is_none_or(|sub| line.split_whitespace().any(|word| word == sub))
        })
    };
    let mut problems = Vec::new();
    if !runs("cargo", Some("clippy")) {
        problems.push(format!(
            "`make lint` no longer executes `cargo clippy`: {recipe}"
        ));
    }
    if !runs("whitaker", None) {
        problems.push(format!("`make lint` no longer executes Whitaker: {recipe}"));
    }
    problems
}

/// Whitaker builds with its own installer-managed toolchain, so its route clears every inherited
/// flag, backend and linker setting instead of restating the standard, and passes the warning
/// policy through Dylint's own input. It neither assigns `RUSTFLAGS` nor selects LLVM, so Cargo's
/// repository defaults stay in force for the checked crate. The route runs on Linux and macOS
/// hosts, so both are read.
#[test]
fn whitaker_clears_inherited_settings_and_takes_the_repository_defaults() {
    for host in [Host::Linux, Host::Darwin] {
        let recipe = dry_run("lint-whitaker", host, None).expect("read `make -n lint-whitaker`");
        let problems = whitaker_route_problems(&recipe, &format!("{host:?}"));
        assert!(problems.is_empty(), "{problems:#?}");
    }
}

/// `make lint` runs both halves, so isolating Whitaker cannot drop either from the gate.
#[test]
fn lint_runs_clippy_and_whitaker() {
    let recipe = dry_run("lint", Host::Linux, None).expect("read `make -n lint` output");
    let problems = lint_problems(&recipe);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// A compliant `lint-whitaker` line, as `make -n` prints it.
const WHITAKER_OK: &str = concat!(
    "env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u CARGO_BUILD_RUSTFLAGS ",
    "-u CARGO_PROFILE_DEV_CODEGEN_BACKEND -u CARGO_BUILD_TARGET ",
    "-u CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER ",
    "-u CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS ",
    "-u CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER ",
    "-u CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS ",
    "-u CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER ",
    "-u CARGO_TARGET_X86_64_APPLE_DARWIN_RUSTFLAGS -u CFLAGS -u LDFLAGS ",
    "DYLINT_RUSTFLAGS=\"-D warnings\" whitaker --all -- --all-targets --all-features",
);

/// The readers judge canned recipes, so the checks above cannot pass by finding nothing: the
/// compliant line passes, with and without a trailing Cargo option, and each broken variant is
/// refused.
#[test]
fn the_readers_refuse_each_broken_recipe() {
    let ok = whitaker_route_problems(WHITAKER_OK, "Linux");
    assert!(ok.is_empty(), "the compliant line was refused: {ok:#?}");
    let locked = whitaker_route_problems(&format!("{WHITAKER_OK} --locked"), "Linux");
    assert!(
        locked.is_empty(),
        "a trailing Cargo option was refused: {locked:#?}"
    );
    let broken = [
        (
            "echoed",
            WHITAKER_OK.replace("whitaker --all", "echo whitaker --all"),
        ),
        (
            "empty RUSTFLAGS",
            WHITAKER_OK.replace("DYLINT", "RUSTFLAGS= DYLINT"),
        ),
        (
            "single quoted",
            WHITAKER_OK.replace("DYLINT", "RUSTFLAGS='-Zthreads=8' DYLINT"),
        ),
        (
            "bare",
            WHITAKER_OK.replace("DYLINT", "RUSTFLAGS=-Zthreads=8 DYLINT"),
        ),
        (
            "LLVM",
            WHITAKER_OK.replace("DYLINT", "CARGO_PROFILE_DEV_CODEGEN_BACKEND=llvm DYLINT"),
        ),
        ("kept flags", WHITAKER_OK.replace("-u RUSTFLAGS ", "")),
        (
            "kept build flags",
            WHITAKER_OK.replace("-u CARGO_BUILD_RUSTFLAGS ", ""),
        ),
        (
            "no warning policy",
            WHITAKER_OK.replace("DYLINT_RUSTFLAGS=\"-D warnings\" ", ""),
        ),
    ];
    for (name, recipe) in broken {
        assert!(
            !whitaker_route_problems(&recipe, "Linux").is_empty(),
            "a recipe with {name} passed: {recipe}"
        );
    }
    let printed = "echo cargo clippy --all-targets\necho whitaker --all";
    assert_eq!(
        lint_problems(printed).len(),
        2,
        "a lint recipe that only prints passed"
    );
    let real = "RUSTFLAGS=\"-D warnings\" cargo clippy --all-targets\nenv -u X \
                DYLINT_RUSTFLAGS=\"-D warnings\" whitaker --all";
    let real_problems = lint_problems(real);
    assert!(
        real_problems.is_empty(),
        "a real lint recipe was refused: {real_problems:#?}"
    );
}

/// The program reader skips assignments and the `env` wrapper, and a printed name is not the
/// program that runs.
#[test]
fn the_program_reader_names_the_word_the_shell_executes() {
    let cases = [
        ("cargo clippy --all-targets", Some("cargo")),
        ("RUSTFLAGS=\"-D warnings\" cargo clippy", Some("cargo")),
        ("A=\"x y\" B='z w' C=v whitaker --all", Some("whitaker")),
        (
            "env -u RUSTFLAGS -u CFLAGS whitaker --all",
            Some("whitaker"),
        ),
        ("env -u X Y=\"a b\" whitaker --all", Some("whitaker")),
        ("env -i PATH=/bin cargo test", Some("cargo")),
        (
            "/home/me/.local/bin/whitaker --all",
            Some("/home/me/.local/bin/whitaker"),
        ),
        ("echo whitaker --all", Some("echo")),
        (
            "env -u X DYLINT_RUSTFLAGS=\"-D warnings\" echo whitaker",
            Some("echo"),
        ),
        ("A=1", None),
    ];
    for (command, expected) in cases {
        assert_eq!(executed_program(command), expected, "{command}");
    }
}

/// The footprint rule needs the default development profile to keep no incremental state and no
/// debug information, and the native Cargo setting that builds without embedded metadata on a dated
/// nightly.
#[test]
fn the_repository_keeps_the_development_footprint_settings() {
    let root = Dir::open_ambient_dir(env!("CARGO_MANIFEST_DIR"), ambient_authority())
        .expect("open the crate directory");
    let manifest: toml::Value =
        toml::from_str(&root.read_to_string("Cargo.toml").expect("read Cargo.toml"))
            .expect("parse Cargo.toml");
    let dev = manifest
        .get("profile")
        .and_then(|profile| profile.get("dev"));
    assert_eq!(
        dev.and_then(|table| table.get("incremental"))
            .and_then(toml::Value::as_bool),
        Some(false),
        "profile.dev must set incremental = false"
    );
    assert_eq!(
        dev.and_then(|table| table.get("debug"))
            .and_then(toml::Value::as_integer),
        Some(0),
        "profile.dev must set debug = 0"
    );
    let config: toml::Value = toml::from_str(
        &root
            .read_to_string(".cargo/config.toml")
            .expect("read .cargo/config.toml"),
    )
    .expect("parse .cargo/config.toml");
    assert_eq!(
        config
            .get("unstable")
            .and_then(|unstable| unstable.get("embed-metadata"))
            .and_then(toml::Value::as_bool),
        Some(false),
        "[unstable] must set embed-metadata = false"
    );
}
