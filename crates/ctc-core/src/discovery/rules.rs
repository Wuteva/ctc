use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::{
    diagnostic::{Diagnostic, DiagnosticCategory},
    language::LanguageRegistry,
    matcher::{MatchCount, MatchMode, SearchScope},
};

use super::{
    config::{ProjectConfig, RuleConfig, ScopeConfig},
    config_error, display_path, empty_range, normalize_existing, rule_message_error,
};

pub(super) struct PreparedTemplate {
    pub(super) path: PathBuf,
    pub(super) display_path: PathBuf,
    pub(super) source: String,
}

pub(super) struct PreparedRule {
    pub(super) id: String,
    pub(super) language_id: &'static str,
    pub(super) templates: Vec<PreparedTemplate>,
    pub(super) include: GlobSet,
    pub(super) exclude: GlobSet,
    pub(super) mode: MatchMode,
    pub(super) scope: SearchScope,
    pub(super) message: Option<String>,
    pub(super) kinds: Vec<String>,
    pub(super) inside: Vec<String>,
    pub(super) stop_at_functions: bool,
    pub(super) count: Option<MatchCount>,
}

type PreparedTemplatesResult = (
    Vec<PreparedTemplate>,
    Vec<std::sync::Arc<dyn crate::language::LanguageAdapter>>,
);

pub(super) fn prepare_rules(
    root: &Path,
    config: &ProjectConfig,
    requested_rule_ids: &[String],
    languages: &LanguageRegistry,
    config_display_path: &Path,
) -> Result<Vec<PreparedRule>, Vec<Diagnostic>> {
    let requested = requested_rule_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let mut prepared = Vec::new();
    let mut diagnostics = Vec::new();
    for rule in &config.rules {
        if !requested.is_empty() && !requested.contains(rule.id.as_str()) {
            continue;
        }
        match prepare_rule(root, rule, languages, config_display_path) {
            Ok(prepared_rule) => prepared.push(prepared_rule),
            Err(mut errors) => diagnostics.append(&mut errors),
        }
    }
    if diagnostics.is_empty() {
        Ok(prepared)
    } else {
        Err(diagnostics)
    }
}

fn prepare_rule(
    root: &Path,
    rule: &RuleConfig,
    languages: &LanguageRegistry,
    config_display_path: &Path,
) -> Result<PreparedRule, Vec<Diagnostic>> {
    validate_rule_configuration(rule, config_display_path)?;
    let scope = rule.scope.search_scope();
    let (inside, stop_at_functions) = inside_scope(rule, config_display_path)?;
    let count =
        rule_count(rule).map_err(|message| vec![config_error(config_display_path, message)])?;
    let template_paths = validated_template_paths(rule, config_display_path)?;
    let include = compile_globs(&rule.include, &rule.id, "include", config_display_path)?;
    let exclude = compile_globs(&rule.exclude, &rule.id, "exclude", config_display_path)?;
    let (templates, adapters) =
        prepared_templates(root, rule, &template_paths, languages, config_display_path)?;
    let adapter = shared_adapter(rule, &adapters, config_display_path)?;
    let kinds = validated_kinds(rule, &adapter, config_display_path)?;
    validate_inside_kinds(rule, &inside, &adapter, config_display_path)?;
    Ok(PreparedRule {
        id: rule.id.clone(),
        language_id: adapter.id(),
        templates,
        include,
        exclude,
        mode: rule.mode,
        scope,
        message: rule.message.clone(),
        kinds,
        inside,
        stop_at_functions,
        count,
    })
}

fn validate_rule_configuration(
    rule: &RuleConfig,
    config_display_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    if rule.include.is_empty() {
        return Err(vec![config_error(
            config_display_path,
            format!(
                "Rule `{}` must contain at least one include pattern.",
                rule.id
            ),
        )]);
    }
    let scope = rule.scope.search_scope();
    if rule.mode == MatchMode::Exact && scope != SearchScope::TopLevel {
        return Err(vec![config_error(
            config_display_path,
            format!(
                "Rule `{}` uses exact mode with a non-topLevel scope.",
                rule.id
            ),
        )]);
    }
    if let Some(error) = rule_message_error(rule.message.as_deref()) {
        return Err(vec![config_error(
            config_display_path,
            format!("Rule `{}` {error}", rule.id),
        )]);
    }
    Ok(())
}

fn inside_scope(
    rule: &RuleConfig,
    config_display_path: &Path,
) -> Result<(Vec<String>, bool), Vec<Diagnostic>> {
    match &rule.scope {
        ScopeConfig::Inside(config) => {
            if config.inside.is_empty() {
                return Err(vec![config_error(
                    config_display_path,
                    format!(
                        "Rule `{}` must list at least one kind in its `inside` scope.",
                        rule.id
                    ),
                )]);
            }
            Ok((config.inside.clone(), config.stop_at_functions))
        }
        ScopeConfig::Named(_) => Ok((Vec::new(), false)),
    }
}

fn validated_template_paths<'a>(
    rule: &'a RuleConfig,
    config_display_path: &Path,
) -> Result<Vec<&'a PathBuf>, Vec<Diagnostic>> {
    let template_paths = rule.template.paths();
    if template_paths.is_empty() {
        return Err(vec![config_error(
            config_display_path,
            format!("Rule `{}` must list at least one template.", rule.id),
        )]);
    }
    if template_paths.len() > 1 {
        let problem = if rule.count.is_some() {
            Some("uses `count` with several templates")
        } else if rule.mode == MatchMode::Every && rule.kinds.is_none() {
            Some("uses every mode with several templates but no `kinds`")
        } else {
            None
        };
        if let Some(problem) = problem {
            return Err(vec![config_error(
                config_display_path,
                format!("Rule `{}` {problem}.", rule.id),
            )]);
        }
    }
    if let Some(kinds) = &rule.kinds {
        if rule.mode != MatchMode::Every {
            return Err(vec![config_error(
                config_display_path,
                format!("Rule `{}` uses `kinds` without every mode.", rule.id),
            )]);
        }
        if kinds.is_empty() {
            return Err(vec![config_error(
                config_display_path,
                format!("Rule `{}` must list at least one kind.", rule.id),
            )]);
        }
    }
    Ok(template_paths)
}

fn prepared_templates(
    root: &Path,
    rule: &RuleConfig,
    template_paths: &[&PathBuf],
    languages: &LanguageRegistry,
    config_display_path: &Path,
) -> Result<PreparedTemplatesResult, Vec<Diagnostic>> {
    let mut templates = Vec::new();
    let mut adapters = Vec::new();
    let mut diagnostics = Vec::new();
    for template_path in template_paths {
        match prepare_template(
            root,
            &rule.id,
            template_path,
            languages,
            config_display_path,
        ) {
            Ok((template, adapter)) => {
                templates.push(template);
                adapters.push(adapter);
            }
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    if diagnostics.is_empty() {
        Ok((templates, adapters))
    } else {
        Err(diagnostics)
    }
}

fn shared_adapter(
    rule: &RuleConfig,
    adapters: &[std::sync::Arc<dyn crate::language::LanguageAdapter>],
    config_display_path: &Path,
) -> Result<std::sync::Arc<dyn crate::language::LanguageAdapter>, Vec<Diagnostic>> {
    let adapter = adapters[0].clone();
    if adapters.iter().any(|other| other.id() != adapter.id()) {
        return Err(vec![config_error(
            config_display_path,
            format!(
                "Rule `{}` lists templates for different languages.",
                rule.id
            ),
        )]);
    }
    Ok(adapter)
}

fn validated_kinds(
    rule: &RuleConfig,
    adapter: &std::sync::Arc<dyn crate::language::LanguageAdapter>,
    config_display_path: &Path,
) -> Result<Vec<String>, Vec<Diagnostic>> {
    let kinds = rule.kinds.clone().unwrap_or_default();
    if let Some(unknown) = kinds.iter().find(|kind| !adapter.known_kind(kind)) {
        return Err(vec![config_error(
            config_display_path,
            format!("Rule `{}` lists unknown kind `{unknown}`.", rule.id),
        )]);
    }
    Ok(kinds)
}

fn validate_inside_kinds(
    rule: &RuleConfig,
    inside: &[String],
    adapter: &std::sync::Arc<dyn crate::language::LanguageAdapter>,
    config_display_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    if let Some(unknown) = inside.iter().find(|kind| !adapter.known_kind(kind)) {
        return Err(vec![config_error(
            config_display_path,
            format!(
                "Rule `{}` lists unknown kind `{unknown}` in its `inside` scope.",
                rule.id
            ),
        )]);
    }
    Ok(())
}

/// Resolves and reads one template of a rule.
fn prepare_template(
    root: &Path,
    rule_id: &str,
    template: &Path,
    languages: &LanguageRegistry,
    config_display_path: &Path,
) -> Result<
    (
        PreparedTemplate,
        std::sync::Arc<dyn crate::language::LanguageAdapter>,
    ),
    Diagnostic,
> {
    let path = match normalize_existing(&root.join(template)) {
        Ok(path) if path.starts_with(root) && path.is_file() => path,
        _ => {
            return Err(config_error(
                config_display_path,
                format!(
                    "Rule `{rule_id}` cannot resolve template `{}`.",
                    template.display()
                ),
            ));
        }
    };
    let display_path = PathBuf::from(display_path(root, &path));
    validate_template_filename(&display_path, languages)?;
    let Some(adapter) = languages.for_path(&template_source_path(&path)) else {
        return Err(config_error(
            config_display_path,
            format!("Rule `{rule_id}` uses an unsupported template type."),
        ));
    };
    let source = match fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
            Diagnostic::new(
                "CTC1009",
                DiagnosticCategory::InvalidUtf8,
                "The template is not valid UTF-8.",
                2,
            )
            .with_template(empty_range(&display_path))
        })?,
        Err(error) => {
            return Err(config_error(
                config_display_path,
                format!("Cannot read template for rule `{rule_id}`: {error}"),
            ));
        }
    };
    Ok((
        PreparedTemplate {
            path,
            display_path,
            source,
        },
        adapter,
    ))
}

/// Checks the `count` setting of a rule and converts it.
fn rule_count(rule: &RuleConfig) -> Result<Option<MatchCount>, String> {
    let Some(count) = &rule.count else {
        return Ok(None);
    };
    if rule.mode != MatchMode::Contains {
        return Err(format!(
            "Rule `{}` uses `count` without contains mode.",
            rule.id
        ));
    }
    if count.min == 0 {
        return Err(format!(
            "Rule `{}` has a `count` minimum of 0. Use forbid mode to require no matches.",
            rule.id
        ));
    }
    if count.max.is_some_and(|max| max < count.min) {
        return Err(format!(
            "Rule `{}` has a `count` maximum below its minimum.",
            rule.id
        ));
    }
    Ok(Some(MatchCount {
        min: count.min,
        max: count.max,
    }))
}

pub(super) fn compile_globs(
    patterns: &[String],
    rule_id: &str,
    field: &str,
    config_display_path: &Path,
) -> Result<GlobSet, Vec<Diagnostic>> {
    let mut builder = GlobSetBuilder::new();
    let mut diagnostics = Vec::new();
    for pattern in patterns {
        if pattern.contains('\\')
            || pattern.starts_with('/')
            || Path::new(pattern)
                .components()
                .any(|component| component == Component::ParentDir)
        {
            diagnostics.push(config_error(
                config_display_path,
                format!(
                    "Rule `{rule_id}` has invalid {field} pattern `{pattern}`. Use relative paths with `/` separators."
                ),
            ));
            continue;
        }
        match GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(false)
            .case_insensitive(cfg!(windows))
            .build()
        {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(error) => diagnostics.push(config_error(
                config_display_path,
                format!("Rule `{rule_id}` has invalid {field} pattern `{pattern}`: {error}."),
            )),
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    builder.build().map_err(|error| {
        vec![config_error(
            config_display_path,
            format!("Cannot compile {field} patterns for rule `{rule_id}`: {error}."),
        )]
    })
}

pub fn validate_template_filename(
    path: &Path,
    languages: &LanguageRegistry,
) -> Result<(), Diagnostic> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let Some(source_name) = filename.strip_suffix(".ctmpl") else {
        return Err(Diagnostic::new(
            "CTC1001",
            DiagnosticCategory::InvalidTemplateFilename,
            "A template filename must end in `.ctmpl`.",
            2,
        )
        .with_template(empty_range(path)));
    };
    if source_name.contains('{') || source_name.contains('}') {
        return Err(Diagnostic::new(
            "CTC1001",
            DiagnosticCategory::InvalidTemplateFilename,
            "A template filename cannot contain selector tokens.",
            2,
        )
        .with_template(empty_range(path)));
    }
    if languages.for_path(Path::new(source_name)).is_none() {
        return Err(Diagnostic::new(
            "CTC1008",
            DiagnosticCategory::UnsupportedSourceType,
            format!("Template suffix `{source_name}` does not select a supported source type."),
            2,
        )
        .with_template(empty_range(path)));
    }
    Ok(())
}

fn template_source_path(template_path: &Path) -> PathBuf {
    let value = template_path.to_string_lossy();
    value
        .strip_suffix(".ctmpl")
        .map(PathBuf::from)
        .unwrap_or_else(|| template_path.to_path_buf())
}
