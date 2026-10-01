use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde_json::{Map, Value};

use crate::{
    diagnostic::{Diagnostic, DiagnosticCategory, RunReport, TextRange},
    discovery::{collect_source_files, lenient_globs, resolve_config_path},
    engine::{Engine, display_path},
    suppression::{SuppressionDirective, collect_suppressions},
};

/// Rule keys that never count as a change of the rule definition. The include,
/// exclude, and `allowIgnore` settings have their own findings.
const NON_DEFINITION_KEYS: &[&str] = &["id", "include", "exclude", "message", "allowIgnore"];

#[derive(Clone, Debug)]
pub struct GuardOptions {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub base_ref: String,
    pub allow_rule_changes: bool,
}

impl Engine {
    /// Compares the working tree with the configuration and sources at a Git
    /// reference and reports changes that make the rules weaker.
    pub fn guard(&self, options: &GuardOptions) -> RunReport {
        match self.run_guard(options) {
            Ok(diagnostics) | Err(diagnostics) => RunReport::new(diagnostics),
        }
    }

    fn run_guard(&self, options: &GuardOptions) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
        let root = fs::canonicalize(&options.root).map_err(|error| {
            vec![command_error(format!(
                "Cannot resolve the scan root: {error}"
            ))]
        })?;
        let git = Git {
            root: root.clone(),
            base_ref: options.base_ref.clone(),
        };
        git.verify_base()?;

        let config_path = resolve_config_path(&root, options.config_path.as_deref())?;
        let config_rel = display_path(&root, &config_path)
            .to_string_lossy()
            .into_owned();
        let current_config = read_json(&config_path)?;
        let base_config = git
            .show(&config_rel)
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());

        let mut diagnostics = Vec::new();
        let config_at = TextRange::from_offsets(Path::new(&config_rel), "", 0, 0);
        let mut known_rule_ids = rule_map(&current_config)
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();

        let mut source_files = Vec::new();
        collect_source_files(&root, &self.languages, &mut source_files).map_err(|message| {
            vec![Diagnostic::new(
                "CTC9001",
                DiagnosticCategory::InternalToolError,
                message,
                2,
            )]
        })?;
        source_files.sort();

        if let Some(base_config) = &base_config {
            let base_rules = rule_map(base_config);
            known_rule_ids.extend(base_rules.keys().cloned());
            let current_rules = rule_map(&current_config);
            for (id, base_rule) in &base_rules {
                let Some(current_rule) = current_rules.get(id) else {
                    diagnostics.push(finding(
                        "CTC5203",
                        DiagnosticCategory::RuleRemovedOrWeakened,
                        format!("Rule `{id}` was removed from the configuration."),
                        id,
                        &config_at,
                    ));
                    continue;
                };
                if !allows_ignore(base_rule) && allows_ignore(current_rule) {
                    diagnostics.push(finding(
                        "CTC5203",
                        DiagnosticCategory::RuleRemovedOrWeakened,
                        format!("Rule `{id}` now allows suppression comments."),
                        id,
                        &config_at,
                    ));
                }
                if let Some(message) =
                    reduced_selection(&root, &source_files, id, base_rule, current_rule)
                {
                    diagnostics.push(finding(
                        "CTC5202",
                        DiagnosticCategory::RuleSelectionReduced,
                        message,
                        id,
                        &config_at,
                    ));
                }
                if !options.allow_rule_changes
                    && let Some(message) =
                        self.changed_definition(&git, &root, id, base_rule, current_rule)
                {
                    diagnostics.push(finding(
                        "CTC5204",
                        DiagnosticCategory::RuleDefinitionChanged,
                        message,
                        id,
                        &config_at,
                    ));
                }
            }
        }

        diagnostics.extend(self.added_suppressions(&git, &root, &source_files, &known_rule_ids));
        Ok(diagnostics)
    }

    fn changed_definition(
        &self,
        git: &Git,
        root: &Path,
        id: &str,
        base_rule: &Value,
        current_rule: &Value,
    ) -> Option<String> {
        let mut changed = changed_keys(base_rule, current_rule);
        let base_templates = template_paths(base_rule);
        for template in template_paths(current_rule) {
            if base_templates.contains(&template)
                && let Some(reason) = git.template_change(root, template)
            {
                changed.push(reason);
            }
        }
        (!changed.is_empty()).then(|| format!("Rule `{id}` changed: {}.", changed.join(", ")))
    }

    fn added_suppressions(
        &self,
        git: &Git,
        root: &Path,
        source_files: &[PathBuf],
        known_rule_ids: &BTreeSet<String>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for path in source_files {
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            if !text.contains("ctc-ignore") {
                continue;
            }
            let relative = display_path(root, path);
            let current = self.suppression_directives(&text, &relative, known_rule_ids);
            let base = git
                .show(&relative.to_string_lossy())
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .map(|text| self.suppression_directives(&text, &relative, known_rule_ids))
                .unwrap_or_default();
            for (rule_id, directives) in current {
                let before = base.get(&rule_id).map_or(0, Vec::len);
                let added = directives.len().saturating_sub(before);
                for directive in directives.iter().rev().take(added).rev() {
                    diagnostics.push(
                        Diagnostic::new(
                            "CTC5201",
                            DiagnosticCategory::SuppressionAdded,
                            format!("A suppression comment for rule `{rule_id}` was added."),
                            1,
                        )
                        .with_rule(&rule_id)
                        .with_source(directive.range.clone()),
                    );
                }
            }
        }
        diagnostics
    }

    /// Suppression comments of one source text, grouped by rule.
    fn suppression_directives(
        &self,
        text: &str,
        relative: &Path,
        known_rule_ids: &BTreeSet<String>,
    ) -> BTreeMap<String, Vec<SuppressionDirective>> {
        let mut grouped = BTreeMap::<String, Vec<SuppressionDirective>>::new();
        let Some(adapter) = self.languages.for_path(relative) else {
            return grouped;
        };
        let Ok(parsed) = adapter.parse(text, relative) else {
            return grouped;
        };
        let suppressions = collect_suppressions(
            relative,
            &parsed.source,
            &parsed.comments,
            known_rule_ids,
            known_rule_ids,
        );
        for directive in suppressions.directives {
            grouped
                .entry(directive.rule_id.clone())
                .or_default()
                .push(directive);
        }
        grouped
    }
}

struct Git {
    root: PathBuf,
    base_ref: String,
}

impl Git {
    fn command(&self) -> Command {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.root)
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    fn verify_base(&self) -> Result<(), Vec<Diagnostic>> {
        if self.base_ref.is_empty() || self.base_ref.starts_with('-') {
            return Err(vec![command_error(format!(
                "`{}` is not a valid Git reference.",
                self.base_ref
            ))]);
        }
        let output = self
            .command()
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("{}^{{commit}}", self.base_ref))
            .output()
            .map_err(|error| vec![command_error(format!("Cannot run git: {error}"))])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(vec![command_error(format!(
                "Cannot resolve Git reference `{}` in `{}`.",
                self.base_ref,
                self.root.display()
            ))])
        }
    }

    /// The content of a file at the base reference. The path is relative to
    /// the scan root.
    fn show(&self, relative: &str) -> Option<Vec<u8>> {
        let output = self
            .command()
            .arg("show")
            .arg(format!("{}:./{relative}", self.base_ref))
            .output()
            .ok()?;
        output.status.success().then_some(output.stdout)
    }

    /// Describes how a template file differs from the base reference, or
    /// returns `None` when it is the same or when the base has no such file.
    fn template_change(&self, root: &Path, relative: &str) -> Option<String> {
        let current = fs::read(root.join(relative)).ok()?;
        match self.show(relative) {
            Some(base) => (normalized(&base) != normalized(&current))
                .then(|| format!("content of template `{relative}`")),
            None => {
                let directory = self.changed_submodule(relative)?;
                Some(format!(
                    "shared template directory `{directory}` points to a different commit"
                ))
            }
        }
    }

    /// Returns the submodule directory above `relative` whose commit differs
    /// from the base reference.
    fn changed_submodule(&self, relative: &str) -> Option<String> {
        let segments = relative.split('/').collect::<Vec<_>>();
        for length in 1..segments.len() {
            let directory = segments[..length].join("/");
            let output = self
                .command()
                .args(["ls-tree", &self.base_ref, "--", &directory])
                .output()
                .ok()?;
            let listing = String::from_utf8_lossy(&output.stdout).into_owned();
            let Some(base_commit) = listing
                .lines()
                .find(|line| line.starts_with("160000"))
                .and_then(|line| line.split_whitespace().nth(2))
            else {
                continue;
            };
            return match self.current_commit(&directory) {
                Some(current) if current != base_commit => Some(directory),
                _ => None,
            };
        }
        None
    }

    fn current_commit(&self, directory: &str) -> Option<String> {
        let checked_out = Command::new("git")
            .arg("-C")
            .arg(self.root.join(directory))
            .args(["rev-parse", "HEAD"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|commit| !commit.is_empty());
        checked_out.or_else(|| {
            let output = self
                .command()
                .args(["ls-files", "-s", "--", directory])
                .output()
                .ok()?;
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .find(|line| line.starts_with("160000"))
                .and_then(|line| line.split_whitespace().nth(1))
                .map(str::to_string)
        })
    }
}

fn normalized(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            index += 1;
            continue;
        }
        result.push(bytes[index]);
        index += 1;
    }
    result
}

fn command_error(message: String) -> Diagnostic {
    Diagnostic::new(
        "CTC0001",
        DiagnosticCategory::InvalidCommandLine,
        message,
        2,
    )
}

fn finding(
    code: &str,
    category: DiagnosticCategory,
    message: String,
    rule_id: &str,
    at: &TextRange,
) -> Diagnostic {
    Diagnostic::new(code, category, message, 1)
        .with_rule(rule_id)
        .with_source(at.clone())
}

fn read_json(path: &Path) -> Result<Value, Vec<Diagnostic>> {
    let invalid = |message: String| {
        vec![Diagnostic::new(
            "CTC1010",
            DiagnosticCategory::InvalidConfiguration,
            message,
            2,
        )]
    };
    let bytes =
        fs::read(path).map_err(|error| invalid(format!("Cannot read configuration: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("Invalid JSON configuration: {error}.")))
}

/// The template and semantic rules of a configuration by identifier.
fn rule_map(config: &Value) -> BTreeMap<String, &Value> {
    ["rules", "semanticRules"]
        .into_iter()
        .filter_map(|key| config.get(key).and_then(Value::as_array))
        .flatten()
        .filter_map(|rule| Some((rule.get("id")?.as_str()?.to_string(), rule)))
        .collect()
}

fn allows_ignore(rule: &Value) -> bool {
    rule.get("allowIgnore").and_then(Value::as_bool) == Some(true)
}

/// The template paths of a rule. `template` is a string or a list of strings.
fn template_paths(rule: &Value) -> Vec<&str> {
    match rule.get("template") {
        Some(Value::String(path)) => vec![path.as_str()],
        Some(Value::Array(paths)) => paths.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn string_list(rule: &Value, key: &str) -> Vec<String> {
    rule.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Reports the current source files that a rule selected at the base
/// reference and no longer selects.
fn reduced_selection(
    root: &Path,
    source_files: &[PathBuf],
    id: &str,
    base_rule: &Value,
    current_rule: &Value,
) -> Option<String> {
    let base_include = lenient_globs(&string_list(base_rule, "include"));
    let base_exclude = lenient_globs(&string_list(base_rule, "exclude"));
    let current_include = lenient_globs(&string_list(current_rule, "include"));
    let current_exclude = lenient_globs(&string_list(current_rule, "exclude"));
    let mut dropped = source_files
        .iter()
        .map(|path| display_path(root, path).to_string_lossy().into_owned())
        .filter(|relative| {
            base_include.is_match(relative)
                && !base_exclude.is_match(relative)
                && !(current_include.is_match(relative) && !current_exclude.is_match(relative))
        })
        .peekable();
    let example = dropped.peek()?.clone();
    let count = dropped.count();
    Some(format!(
        "Rule `{id}` no longer selects {count} file(s) that it selected at the base reference, for example `{example}`."
    ))
}

/// The definition keys that differ between two versions of a rule.
fn changed_keys(base_rule: &Value, current_rule: &Value) -> Vec<String> {
    let empty = Map::new();
    let base = base_rule.as_object().unwrap_or(&empty);
    let current = current_rule.as_object().unwrap_or(&empty);
    let keys = base
        .keys()
        .chain(current.keys())
        .filter(|key| !NON_DEFINITION_KEYS.contains(&key.as_str()))
        .collect::<BTreeSet<_>>();
    keys.into_iter()
        .filter(|key| base.get(*key) != current.get(*key))
        .map(|key| format!("`{key}`"))
        .collect()
}
