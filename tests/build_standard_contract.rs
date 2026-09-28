//! Contract tests for the Rust build standard.
//!
//! The standard makes the parallel `rustc` frontend the default for every
//! development build and mold the default linker on Linux. Cargo reads both
//! from `.cargo/config.toml`, but it applies a single `rustflags` source rather
//! than merging them, and an assigned `RUSTFLAGS` replaces every source. So the
//! flags must be repeated in each configuration source, restated wherever the
//! Makefile assigns `RUSTFLAGS` for a gate target, and kept out of the
//! coverage and release recipes, which measure or ship and so stay on the
//! default flags.
//!
//! The Makefile clauses run `make -n` and read the commands it would run,
//! rather than the Makefile's text, so a flag lost through a variable or a
//! recipe edit fails here. File access goes through a `cap_std` directory
//! handle rooted at the crate manifest directory.

use std::{error::Error, process::Command};

use cap_std::{ambient_authority, fs::Dir};
use rstest::rstest;

/// The parallel-frontend flag every `rustflags` source must carry.
const THREADS_FLAG: &str = "-Zthreads=8";

/// The linker flag the Linux source must add, normalized to one token.
const MOLD_FLAG: &str = "-Clink-arg=-fuse-ld=mold";

/// Target table keys that apply on Linux alone.
const LINUX_TABLES: [&str; 2] = ["x86_64-unknown-linux-gnu", "cfg(target_os = \"linux\")"];

/// Joins `-C value` pairs into `-Cvalue`, so both spellings compare equal.
fn normalized(flags: &[String]) -> Vec<String> {
    let mut joined: Vec<String> = Vec::new();
    for flag in flags {
        match joined.last_mut() {
            Some(last) if last == "-C" => *last = format!("-C{flag}"),
            _ => joined.push(flag.clone()),
        }
    }
    joined
}

/// Reads one table's `rustflags` as a list of strings, if it has one.
fn table_flags(table: &toml::Value) -> Option<Vec<String>> {
    let flags = table.get("rustflags")?.as_array()?;
    Some(
        flags
            .iter()
            .filter_map(|flag| flag.as_str().map(str::to_owned))
            .collect(),
    )
}

/// The result of a reader, which the tests unwrap.
type Read<T> = Result<T, Box<dyn Error>>;

/// Returns every `rustflags` source in the configuration, by table name.
fn sources() -> Read<Vec<(String, Vec<String>)>> {
    let root = Dir::open_ambient_dir(env!("CARGO_MANIFEST_DIR"), ambient_authority())?;
    let text = root.read_to_string(".cargo/config.toml")?;
    let config: toml::Value = toml::from_str(&text)?;
    let mut found = Vec::new();
    if let Some(flags) = config.get("build").and_then(table_flags) {
        found.push(("build".to_owned(), normalized(&flags)));
    }
    if let Some(targets) = config.get("target").and_then(toml::Value::as_table) {
        for (key, table) in targets {
            if let Some(flags) = table_flags(table) {
                found.push((key.clone(), normalized(&flags)));
            }
        }
    }
    Ok(found)
}

/// Returns the `RUSTFLAGS` each cargo or whitaker command of `make -n TARGET`
/// assigns, split into words.
fn make_rustflags(target: &str) -> Read<Vec<Vec<String>>> {
    let output = Command::new("make")
        .args(["-n", "-B", "BUILD_HOST_OS=Linux", target])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;
    if !output.status.success() {
        return Err(format!("`make -n {target}` failed").into());
    }
    // A recipe continued with a trailing backslash is one command.
    let stdout = String::from_utf8_lossy(&output.stdout).replace("\\\n", " ");
    Ok(stdout
        .lines()
        .filter(|line| line.contains("cargo") || line.contains("whitaker"))
        .filter_map(|line| {
            let (_, rest) = line.split_once("RUSTFLAGS=\"")?;
            let (value, _) = rest.split_once('"')?;
            Some(value.split_whitespace().map(str::to_owned).collect())
        })
        .collect())
}

#[test]
fn every_rustflags_source_carries_the_parallel_frontend() {
    let found = sources().expect("read the configuration sources");
    assert!(
        found.iter().any(|(key, _)| key == "build"),
        "no [build] rustflags for non-Linux hosts"
    );
    let missing: Vec<&str> = found
        .iter()
        .filter(|(_, flags)| !flags.iter().any(|flag| flag == THREADS_FLAG))
        .map(|(key, _)| key.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "{THREADS_FLAG} missing from {missing:?}"
    );
}

#[test]
fn mold_is_confined_to_linux() {
    let found = sources().expect("read the configuration sources");
    let has_mold = |flags: &[String]| flags.iter().any(|flag| flag == MOLD_FLAG);
    let linux: Vec<_> = found
        .iter()
        .filter(|(key, _)| LINUX_TABLES.contains(&key.as_str()))
        .collect();
    assert!(!linux.is_empty(), "no Linux target table carries rustflags");
    assert!(
        linux.iter().all(|(_, flags)| has_mold(flags)),
        "a Linux table lost mold"
    );
    let wider: Vec<&str> = found
        .iter()
        .filter(|(key, flags)| !LINUX_TABLES.contains(&key.as_str()) && has_mold(flags))
        .map(|(key, _)| key.as_str())
        .collect();
    assert!(wider.is_empty(), "mold named beyond Linux in {wider:?}");
}

#[test]
fn sources_differ_only_by_the_linker() {
    let mut stripped: Vec<Vec<String>> = sources()
        .expect("read the configuration sources")
        .into_iter()
        .map(|(_, flags)| flags.into_iter().filter(|flag| flag != MOLD_FLAG).collect())
        .collect();
    stripped.dedup();
    assert_eq!(
        stripped.len(),
        1,
        "rustflags sources disagree: {stripped:?}"
    );
}

#[rstest]
#[case("test")]
#[case("typecheck")]
#[case("lint")]
fn gate_targets_restate_both_flags(#[case] target: &str) {
    let assignments = make_rustflags(target).expect("read `make -n` output");
    assert!(
        !assignments.is_empty(),
        "`make {target}` assigns no RUSTFLAGS"
    );
    for flags in assignments {
        assert!(
            flags.iter().any(|flag| flag == THREADS_FLAG),
            "`make {target}` drops {THREADS_FLAG}: {flags:?}"
        );
        assert!(
            flags.iter().any(|flag| flag == MOLD_FLAG),
            "`make {target}` drops {MOLD_FLAG}: {flags:?}"
        );
    }
}

/// Coverage measures and release ships, so both stay on the default flags;
/// each must also assign `RUSTFLAGS`, since only an assignment displaces the
/// configuration's sources.
#[rstest]
#[case("coverage")]
#[case("release")]
fn coverage_and_release_take_neither_flag(#[case] target: &str) {
    let assignments = make_rustflags(target).expect("read `make -n` output");
    assert!(
        !assignments.is_empty(),
        "`make {target}` assigns no RUSTFLAGS, so it takes the configuration's"
    );
    for flags in assignments {
        assert!(
            !flags.iter().any(|flag| flag == THREADS_FLAG),
            "`make {target}` takes {THREADS_FLAG}"
        );
        assert!(
            !flags.iter().any(|flag| flag == MOLD_FLAG),
            "`make {target}` takes {MOLD_FLAG}"
        );
    }
}
