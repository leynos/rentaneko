//! Readers for the suite-once contract: shell commands, workflow jobs and
//! steps, and manifest features, read textually or as TOML.
//!
//! Kept apart from `workflow_suite_contract.rs` so each file stays under the
//! repository's 400-line limit. Only that contract uses these readers. The
//! text sits behind small wrapper types (`Command`, `Workflow`, `Job`,
//! `Step`, `Manifest`), so each question is a method on what it reads.

use cap_std::{ambient_authority, fs::Dir};

mod shell;
mod tokenizer;

/// Opens the crate manifest directory as a capability-scoped handle.
pub(crate) fn manifest_dir() -> std::io::Result<Dir> {
    Dir::open_ambient_dir(env!("CARGO_MANIFEST_DIR"), ambient_authority())
}

/// Returns the Makefile's default goal: what a bare `make` runs.
pub(crate) fn default_goal() -> std::io::Result<String> {
    Ok(default_goal_of(
        &manifest_dir()?.read_to_string("Makefile")?,
    ))
}

/// Reads the default goal from Makefile text: what `.DEFAULT_GOAL` holds once
/// every assignment has been applied in order (GNU make manual, "Other
/// Special Variables"), otherwise the first rule that
/// is not a special or pattern target.
pub(crate) fn default_goal_of(makefile: &str) -> String {
    let assigned = makefile
        .lines()
        .filter(|line| !line.starts_with('\t'))
        .filter_map(assignment_of)
        .fold(None, apply)
        .filter(|goal| !goal.contains(char::is_whitespace));
    assigned
        .or_else(|| makefile.lines().find_map(first_goal))
        .unwrap_or_default()
}

/// How an assignment to `.DEFAULT_GOAL` combines with the value before it.
#[derive(Clone, Copy)]
enum Assignment<'a> {
    /// `=` and `:=` replace the value, and an empty one clears it.
    Set(&'a str),
    /// `+=` appends a word, and make refuses a default goal of several.
    Append(&'a str),
}

/// Returns the assignment to `.DEFAULT_GOAL` a line makes, if it makes one.
///
/// A `?=` line is not one: make defines `.DEFAULT_GOAL` itself, empty, before
/// it reads a makefile, so `?=` finds it defined and changes nothing.
fn assignment_of(line: &str) -> Option<Assignment<'_>> {
    let (left, right) = line.split_once(":=").or_else(|| line.split_once('='))?;
    let (name, value) = (left.trim_end(), right.trim());
    let is_goal = |base: &str| base.trim_end() == ".DEFAULT_GOAL";
    if name.strip_suffix('+').is_some_and(is_goal) {
        Some(Assignment::Append(value))
    } else {
        is_goal(name.trim_end_matches(':')).then_some(Assignment::Set(value))
    }
}

/// Returns the value of `.DEFAULT_GOAL` after one more assignment.
fn apply(current: Option<String>, assignment: Assignment<'_>) -> Option<String> {
    let value = match assignment {
        Assignment::Set(value) => Some(value.to_owned()),
        Assignment::Append(value) => Some(format!("{} {value}", current.unwrap_or_default())),
    };
    value
        .map(|goal| goal.trim().to_owned())
        .filter(|goal| !goal.is_empty())
}

/// Returns the first goal a rule line names, or `None` for any other line.
fn first_goal(line: &str) -> Option<String> {
    if line.starts_with(['\t', ' ', '#', '.']) {
        return None;
    }
    let (names, rest) = line.split_once(':')?;
    if rest.starts_with('=') || names.contains(['=', '%', '$']) {
        return None;
    }
    names.split_whitespace().next().map(str::to_owned)
}

/// Returns every workflow file's name and text.
pub(crate) fn workflows() -> std::io::Result<Vec<(String, String)>> {
    let dir = manifest_dir()?.open_dir(".github/workflows")?;
    let mut found = Vec::new();
    for entry in dir.entries()? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        let is_workflow = name.rsplit_once('.').is_some_and(|(_, extension)| {
            extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
        });
        if is_workflow {
            let text = dir.read_to_string(&name)?;
            found.push((name, text));
        }
    }
    Ok(found)
}

/// One shell command line from a workflow.
#[derive(Clone, Copy)]
pub(crate) struct Command<'a>(&'a str);

impl<'a> Command<'a> {
    /// Reads a workflow line as a command: without a `run:` prefix, whether
    /// or not it opens a step with `- `, and empty for a comment. Every line
    /// is read, so a suite run inside a multi-line `run: |` block is seen as
    /// well as a single-line one.
    pub(crate) fn from_line(line: &'a str) -> Self {
        let trimmed = line.trim();
        let item = trimmed.strip_prefix("- ").map_or(trimmed, str::trim_start);
        let command = if trimmed.starts_with('#') {
            ""
        } else {
            item.strip_prefix("run:").map_or(item, str::trim)
        };
        Self(command)
    }

    /// Returns the command's text.
    pub(crate) const fn text(self) -> &'a str { self.0 }

    /// Returns `true` if the command runs the suite.
    pub(crate) fn runs_suite(self, default_goal: &str) -> bool {
        shell::runs_suite(self.0, default_goal)
    }
}

/// Returns a line's indentation width.
fn indent(line: &str) -> usize { line.len() - line.trim_start().len() }

/// Returns `true` for a line that opens a YAML list item.
fn is_item(line: &str) -> bool { line.trim_start().starts_with("- ") }

/// Returns `true` for a line that carries no YAML content.
fn is_blank_or_comment(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// A workflow document's text.
#[derive(Clone, Copy)]
pub(crate) struct Workflow<'a>(pub(crate) &'a str);

/// One job of a workflow: its name and the lines under it.
pub(crate) struct Job<'a> {
    /// The job's key under `jobs:`.
    pub(crate) name: &'a str,
    /// The lines between this job's key and the next one.
    lines: Vec<&'a str>,
}

/// One step of a job, as its lines.
pub(crate) struct Step<'a>(Vec<&'a str>);

impl<'a> Workflow<'a> {
    /// Returns each job, in order. A job key is any line indented one level
    /// under `jobs:`, whatever that level's width.
    pub(crate) fn jobs(self) -> Vec<Job<'a>> {
        let section: Vec<&'a str> = self
            .0
            .lines()
            .skip_while(|line| line.trim_end() != "jobs:")
            .skip(1)
            .filter(|line| !is_blank_or_comment(line))
            .take_while(|line| indent(line) > 0)
            .collect();
        let level = section.first().map_or(0, |line| indent(line));
        let mut found: Vec<Job<'a>> = Vec::new();
        for line in section {
            match line.trim().strip_suffix(':') {
                Some(name) if indent(line) == level => found.push(Job {
                    name,
                    lines: Vec::new(),
                }),
                _ => found
                    .last_mut()
                    .into_iter()
                    .for_each(|job| job.lines.push(line)),
            }
        }
        found
    }
}

impl<'a> Job<'a> {
    /// Returns `true` if the job carries its own `if:` condition, read at
    /// the job's own key indentation, whatever its width.
    pub(crate) fn is_conditional(&self) -> bool {
        let level = self.lines.iter().map(|line| indent(line)).min();
        self.lines
            .iter()
            .any(|line| Some(indent(line)) == level && line.trim_start().starts_with("if:"))
    }

    /// Returns the job's lines as commands.
    pub(crate) fn commands(&self) -> impl Iterator<Item = Command<'a>> + '_ {
        self.lines.iter().map(|line| Command::from_line(line))
    }

    /// Splits the job into steps, each a run of lines starting at a `- `
    /// item at the step list's own indentation, so a step can be found by
    /// what it does, not its name, and a nested list inside a step does not
    /// split it.
    pub(crate) fn steps(&self) -> Vec<Step<'a>> {
        let items = self
            .lines
            .iter()
            .skip_while(|line| line.trim() != "steps:")
            .skip(1);
        let level = items
            .clone()
            .find(|line| is_item(line))
            .map(|line| indent(line));
        let mut found: Vec<Step<'a>> = Vec::new();
        for line in items {
            if is_item(line) && Some(indent(line)) == level {
                found.push(Step(Vec::new()));
            }
            found
                .last_mut()
                .into_iter()
                .for_each(|step| step.0.push(line));
        }
        found
    }
}

impl Step<'_> {
    /// Returns `true` if the step carries an `if:` condition of its own: on
    /// its item line or at the indentation of the item's other keys, so an
    /// `if:` under `with:` or inside a `run:` body does not count.
    pub(crate) fn is_conditional(&self) -> bool {
        let Some((first, rest)) = self.0.split_first() else {
            return false;
        };
        let item = first.trim_start();
        let Some(body) = item.strip_prefix("- ") else {
            return false;
        };
        let level = indent(first) + (item.len() - body.trim_start().len());
        body.trim_start().starts_with("if:")
            || rest
                .iter()
                .any(|line| indent(line) == level && line.trim_start().starts_with("if:"))
    }

    /// Returns `true` if one of the step's lines runs exactly `command`,
    /// whether inline after `run:` or as a line of a `run: |` block.
    pub(crate) fn runs(&self, command: Command<'_>) -> bool {
        self.0
            .iter()
            .any(|line| Command::from_line(line).text() == command.text())
    }

    /// Returns `true` if the step uses an action whose reference contains
    /// `action`.
    pub(crate) fn uses(&self, action: &str) -> bool {
        self.0
            .iter()
            .any(|line| line.contains("uses:") && line.contains(action))
    }
}

/// A parsed `Cargo.toml`.
pub(crate) struct Manifest(toml::Value);

impl Manifest {
    /// Parses a manifest.
    ///
    /// # Errors
    ///
    /// Returns the parser's error when the text is not TOML.
    pub(crate) fn parse(text: &str) -> Result<Self, toml::de::Error> {
        Ok(Self(toml::from_str(text)?))
    }

    /// Returns the manifest's feature names: the `[features]` keys, plus
    /// each optional dependency Cargo turns into an implicit feature. An
    /// optional dependency named anywhere as `dep:<name>` exposes no
    /// implicit feature, as Cargo documents, so it is left out.
    pub(crate) fn features(&self) -> Vec<String> {
        let table = self.0.get("features").and_then(toml::Value::as_table);
        let mut found: Vec<String> = table
            .map(|t| t.keys().cloned().collect())
            .unwrap_or_default();
        let suppressed: Vec<String> = found
            .iter()
            .flat_map(|name| self.feature_values(name))
            .filter_map(|value| value.strip_prefix("dep:").map(str::to_owned))
            .collect();
        found.extend(
            self.optional_dependencies()
                .into_iter()
                .filter(|name| !suppressed.contains(name)),
        );
        found
    }

    /// Returns the values one feature enables.
    fn feature_values(&self, feature: &str) -> Vec<String> {
        self.0
            .get("features")
            .and_then(|features| features.get(feature))
            .and_then(toml::Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns every optional dependency's name, target tables included.
    fn optional_dependencies(&self) -> Vec<String> {
        let tables = ["dependencies", "dev-dependencies", "build-dependencies"];
        let mut dependency_tables: Vec<&toml::Value> =
            tables.iter().filter_map(|name| self.0.get(*name)).collect();
        if let Some(targets) = self.0.get("target").and_then(toml::Value::as_table) {
            dependency_tables.extend(
                targets
                    .values()
                    .flat_map(|platform| tables.iter().filter_map(move |name| platform.get(*name))),
            );
        }
        dependency_tables
            .iter()
            .filter_map(|table| table.as_table())
            .flat_map(|table| table.iter())
            .filter(|(_, spec)| spec.get("optional").and_then(toml::Value::as_bool) == Some(true))
            .map(|(name, _)| name.clone())
            .collect()
    }
}
