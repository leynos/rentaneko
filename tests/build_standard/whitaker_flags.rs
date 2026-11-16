//! The individual complaints about one `make -n lint-whitaker` command line. Each check reads the
//! command as words and reports at most one complaint (or one per missing setting), so
//! `whitaker_footprint.rs` can compose them without one function carrying every branch.

/// The inherited settings the Whitaker route must clear: encoded and build flags, target, each
/// host's linker and rustflags, and the C compiler's flags. `RUSTFLAGS` and the development
/// backend are assigned rather than cleared.
const WHITAKER_CLEARED: [&str; 11] = [
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
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

/// One complaint for each inherited setting the Whitaker command does not remove.
pub(super) fn uncleared_settings(whitaker: &str, host: &str) -> Vec<String> {
    WHITAKER_CLEARED
        .iter()
        .filter(|setting| !whitaker.contains(&format!("-u {setting} ")))
        .map(|setting| format!("Whitaker on {host} must remove inherited {setting}: {whitaker}"))
        .collect()
}

/// Complains unless exactly one `RUSTFLAGS` assignment carries the warning policy alone.
pub(super) fn rustflags_problem(whitaker: &str, words: &[&str]) -> Option<String> {
    let assigned = words
        .iter()
        .filter(|word| word.starts_with("RUSTFLAGS="))
        .count();
    let value = whitaker
        .split_once(" RUSTFLAGS=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(value, _)| value);
    (assigned != 1 || value != Some("-D warnings"))
        .then(|| format!("Whitaker must assign RUSTFLAGS the warning policy alone: {whitaker}"))
}

/// Complains unless the command selects LLVM and opts Dylint's driver build into the backend key.
pub(super) fn backend_problem(whitaker: &str, words: &[&str]) -> Option<String> {
    let selects_backend = words.contains(&"CARGO_PROFILE_DEV_CODEGEN_BACKEND=llvm")
        && words.contains(&"CARGO_UNSTABLE_CODEGEN_BACKEND=true");
    (!selects_backend).then(|| {
        format!(
            "Whitaker must select LLVM for the development profile and opt Dylint's driver build \
             into the backend key: {whitaker}"
        )
    })
}

/// Complains when the command assigns encoded or build flags.
pub(super) fn encoded_flags_problem(whitaker: &str, words: &[&str]) -> Option<String> {
    let assigns_flags = words.iter().any(|word| {
        word.starts_with("CARGO_ENCODED_RUSTFLAGS=") || word.starts_with("CARGO_BUILD_RUSTFLAGS=")
    });
    assigns_flags.then(|| format!("Whitaker must not assign encoded or build flags: {whitaker}"))
}

/// Complains unless the warning policy also reaches the check through Dylint's input.
pub(super) fn dylint_input_problem(whitaker: &str) -> Option<String> {
    (!whitaker.contains("DYLINT_RUSTFLAGS=\"-D warnings"))
        .then(|| format!("warnings must reach the check through Dylint's input: {whitaker}"))
}

/// One complaint for each of the target and feature selections the command dropped.
pub(super) fn missing_options(whitaker: &str, words: &[&str]) -> Vec<String> {
    ["--all-targets", "--all-features"]
        .into_iter()
        .filter(|option| !words.contains(option))
        .map(|option| format!("the check must keep {option}: {whitaker}"))
        .collect()
}
