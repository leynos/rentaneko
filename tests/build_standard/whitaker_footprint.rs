//! The Whitaker and footprint clauses of the build-standard contract: the Whitaker route runs from
//! a clean environment instead of restating the development flags, `make lint` still runs Clippy
//! and Whitaker, and the development footprint settings stay in `Cargo.toml` and
//! `.cargo/config.toml`. It is a module of `build_standard_contract.rs`, so it shares that file's
//! readers.

use super::{Host, dry_run};

/// Returns the command `make -n lint-whitaker` prints on the host: the line whose program is the
/// Whitaker executable, found by its words rather than by a substring.
fn whitaker_command(host: Host) -> Result<String, Box<dyn std::error::Error>> {
    let recipe = dry_run("lint-whitaker", host, None)?;
    recipe
        .lines()
        .find(|line| {
            line.split_whitespace().any(|word| {
                word.rsplit('/')
                    .next()
                    .is_some_and(|name| name == "whitaker")
            })
        })
        .map(str::to_owned)
        .ok_or_else(|| {
            format!("`make lint-whitaker` on {host:?} must run the Whitaker executable").into()
        })
}

/// Whitaker builds with its own installer-managed toolchain, so its route clears every inherited
/// flag, backend and linker setting instead of restating the standard, and passes the warning
/// policy through Dylint's own input. It neither assigns `RUSTFLAGS` nor selects LLVM, so Cargo's
/// repository defaults stay in force for the checked crate. The route runs on Linux and macOS
/// hosts, so both are read.
#[test]
fn whitaker_clears_inherited_settings_and_takes_the_repository_defaults() {
    for host in [Host::Linux, Host::Darwin] {
        let whitaker = whitaker_command(host).expect("read `make -n lint-whitaker` output");
        for setting in [
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
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
        ] {
            assert!(
                whitaker.contains(&format!("-u {setting} ")),
                "Whitaker on {host:?} must remove inherited {setting}: {whitaker}"
            );
        }
        assert!(
            !whitaker.contains(" RUSTFLAGS=\"") && !whitaker.contains("=llvm"),
            "Whitaker must not replace Cargo defaults with assigned flags or LLVM: {whitaker}"
        );
        assert!(
            whitaker.contains("DYLINT_RUSTFLAGS=\"-D warnings"),
            "warnings must reach the check through Dylint's input: {whitaker}"
        );
        for option in ["--all-targets", "--all-features"] {
            assert!(
                whitaker.split_whitespace().any(|word| word == option),
                "the check must keep {option}: {whitaker}"
            );
        }
    }
}

/// `make lint` runs both halves, so isolating Whitaker cannot drop either from the gate: one
/// command runs Clippy through Cargo and another runs the Whitaker executable.
#[test]
fn lint_runs_clippy_and_whitaker() {
    let recipe = dry_run("lint", Host::Linux, None).expect("read `make -n lint` output");
    let runs = |program: &str, subcommand: Option<&str>| {
        recipe.lines().any(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            words
                .iter()
                .any(|word| word.rsplit('/').next().is_some_and(|name| name == program))
                && subcommand.is_none_or(|sub| words.contains(&sub))
        })
    };
    assert!(
        runs("cargo", Some("clippy")),
        "`make lint` no longer runs `cargo clippy`: {recipe}"
    );
    assert!(
        runs("whitaker", None),
        "`make lint` no longer runs Whitaker: {recipe}"
    );
}

/// The footprint rule needs the default development profile to keep no incremental state and no
/// debug information, and the native Cargo setting that builds without embedded metadata on a dated
/// nightly.
#[test]
fn the_repository_keeps_the_development_footprint_settings() {
    let manifest: toml::Value =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .expect("read Cargo.toml")
            .parse()
            .expect("parse Cargo.toml");
    let dev = manifest
        .get("profile")
        .and_then(|profile| profile.get("dev"));
    assert_eq!(
        dev.and_then(|dev| dev.get("incremental"))
            .and_then(toml::Value::as_bool),
        Some(false),
        "profile.dev must set incremental = false"
    );
    assert_eq!(
        dev.and_then(|dev| dev.get("debug"))
            .and_then(toml::Value::as_integer),
        Some(0),
        "profile.dev must set debug = 0"
    );
    let config: toml::Value =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/.cargo/config.toml"))
            .expect("read .cargo/config.toml")
            .parse()
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
