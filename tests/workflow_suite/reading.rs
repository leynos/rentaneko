//! Readers for the suite-once contract: shell commands, workflow jobs and
//! steps, and manifest features, all read textually or as TOML.
//!
//! Kept apart from `workflow_suite_contract.rs` so each file stays under the
//! repository's 400-line limit. Only that contract uses these helpers.

use cap_std::{ambient_authority, fs::Dir};

/// Cargo options that take their value as the next word.
pub(crate) const CARGO_VALUE_OPTIONS: [&str; 6] = [
    "--config",
    "-Z",
    "-C",
    "--manifest-path",
    "--color",
    "--target-dir",
];

/// Make options that take their value as the next word.
pub(crate) const MAKE_VALUE_OPTIONS: [&str; 8] = [
    "-C",
    "-f",
    "-I",
    "-o",
    "-W",
    "--directory",
    "--file",
    "--makefile",
];

/// Cargo subcommands that run the suite.
pub(crate) const SUITE_SUBCOMMANDS: [&str; 3] = ["test", "nextest", "llvm-cov"];

/// Make targets that run the suite.
pub(crate) const SUITE_TARGETS: [&str; 2] = ["test", "all"];

/// Opens the crate manifest directory as a capability-scoped handle.
pub(crate) fn manifest_dir() -> std::io::Result<Dir> {
    Dir::open_ambient_dir(env!("CARGO_MANIFEST_DIR"), ambient_authority())
}

/// Returns every workflow file's name and text.
pub(crate) fn workflows() -> std::io::Result<Vec<(String, String)>> {
    let dir = manifest_dir()?.open_dir(".github/workflows")?;
    let mut found = Vec::new();
    for entry in dir.entries()? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        let is_workflow = std::path::Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"));
        if is_workflow {
            let text = dir.read_to_string(&name)?;
            found.push((name, text));
        }
    }
    Ok(found)
}

/// Splits a command into the word lists of its shell segments, so a suite
/// run after `&&`, `||`, `;` or `|` is read as its own command.
pub(crate) fn segments(command: &str) -> Vec<Vec<&str>> {
    let mut found = Vec::new();
    let mut current = Vec::new();
    for word in command.split_whitespace() {
        let trimmed = word.trim_end_matches(';');
        let separator = matches!(word, "&&" | "||" | "|") || trimmed.is_empty();
        if !separator {
            current.push(trimmed);
        }
        if separator || word.ends_with(';') {
            found.push(std::mem::take(&mut current));
        }
    }
    found.push(current);
    found
}

/// Returns the words after the first word equal to `program`, or naming it
/// by path, in one segment.
pub(crate) fn arguments_of<'a>(words: &[&'a str], program: &str) -> Option<Vec<&'a str>> {
    let suffix = format!("/{program}");
    let start = words
        .iter()
        .position(|word| *word == program || word.ends_with(&suffix))?;
    Some(words.get(start + 1..).unwrap_or_default().to_vec())
}

/// Returns `true` if a word is an operand rather than an option, a
/// toolchain selector or a variable assignment.
pub(crate) fn is_operand(word: &str) -> bool {
    let is_flag = word.starts_with('-') || word.starts_with('+');
    !is_flag && !word.contains('=')
}

/// Returns the operands of a command line: its words less options, their
/// values, toolchain selectors and variable assignments.
pub(crate) fn operands<'a>(arguments: &[&'a str], value_options: &[&str]) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        if value_options.contains(word) {
            words.next();
        } else if is_operand(word) {
            found.push(*word);
        }
    }
    found
}

/// Returns `true` if one shell segment runs the suite.
pub(crate) fn segment_runs_suite(words: &[&str]) -> bool {
    let cargo = arguments_of(words, "cargo").is_some_and(|arguments| {
        operands(&arguments, &CARGO_VALUE_OPTIONS)
            .first()
            .is_some_and(|subcommand| SUITE_SUBCOMMANDS.contains(subcommand))
    });
    let make = arguments_of(words, "make").is_some_and(|arguments| {
        operands(&arguments, &MAKE_VALUE_OPTIONS)
            .iter()
            .any(|target| SUITE_TARGETS.contains(target))
    });
    cargo || make
}

/// Returns `true` if a shell command line runs the suite, in any spelling.
pub(crate) fn runs_suite(line: &str) -> bool {
    segments(line).iter().any(|words| segment_runs_suite(words))
}

/// Returns the command a `run:` line carries, or `None` for other lines.
pub(crate) fn run_command(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("run:").map(str::trim)
}

/// Returns a workflow line as a command: without a `run:` prefix, and empty
/// for a comment. Every line is read, so a suite run inside a multi-line
/// `run: |` block is seen as well as a single-line one.
pub(crate) fn command_text(line: &str) -> &str {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return "";
    }
    run_command(trimmed).unwrap_or(trimmed)
}

/// Returns the name of the job a line opens, for a `  name:` line under
/// `jobs:`.
pub(crate) fn job_header(line: &str) -> Option<&str> {
    let name = line.strip_prefix("  ")?.strip_suffix(':')?;
    let is_name = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    is_name.then_some(name)
}

/// Returns each job's name and lines, in order.
pub(crate) fn jobs(workflow: &str) -> Vec<(&str, Vec<&str>)> {
    let mut found: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut in_jobs = false;
    for line in workflow.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            in_jobs = line.trim_end() == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        if let Some(name) = job_header(line) {
            found.push((name, Vec::new()));
        } else if let Some((_, lines)) = found.last_mut() {
            lines.push(line);
        }
    }
    found
}

/// Splits a job into its steps, each a run of lines starting at a `- ` list
/// item, so a step can be found by what it runs rather than by its name.
pub(crate) fn steps<'a>(job: &[&'a str]) -> Vec<Vec<&'a str>> {
    let mut found: Vec<Vec<&'a str>> = Vec::new();
    for line in job {
        if line.trim_start().starts_with("- ") {
            found.push(Vec::new());
        }
        if let Some(step) = found.last_mut() {
            step.push(line);
        }
    }
    found
}

/// Returns `true` if a job carries its own `if:` condition.
pub(crate) fn job_is_conditional(job: &[&str]) -> bool {
    job.iter().any(|line| line.starts_with("    if:"))
}

/// Returns `true` if a step carries an `if:` condition.
pub(crate) fn step_is_conditional(step: &[&str]) -> bool {
    step.iter().any(|line| {
        line.trim_start()
            .trim_start_matches("- ")
            .starts_with("if:")
    })
}

/// Returns a manifest's feature names, with each optional dependency, which
/// Cargo turns into an implicit feature, added by name.
pub(crate) fn manifest_features(manifest: &str) -> Result<Vec<String>, toml::de::Error> {
    let parsed: toml::Value = toml::from_str(manifest)?;
    let mut found: Vec<String> = parsed
        .get("features")
        .and_then(toml::Value::as_table)
        .map(|table| table.keys().cloned().collect())
        .unwrap_or_default();
    let tables = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut dependency_tables: Vec<&toml::Value> =
        tables.iter().filter_map(|name| parsed.get(*name)).collect();
    if let Some(targets) = parsed.get("target").and_then(toml::Value::as_table) {
        dependency_tables.extend(
            targets
                .values()
                .flat_map(|platform| tables.iter().filter_map(move |name| platform.get(*name))),
        );
    }
    found.extend(
        dependency_tables
            .iter()
            .filter_map(|table| table.as_table())
            .flat_map(|table| table.iter())
            .filter(|(_, spec)| spec.get("optional").and_then(toml::Value::as_bool) == Some(true))
            .map(|(name, _)| name.clone()),
    );
    Ok(found)
}
