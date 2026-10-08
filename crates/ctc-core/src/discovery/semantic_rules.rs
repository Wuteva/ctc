use std::{collections::BTreeSet, path::Path};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::{
    diagnostic::Diagnostic,
    language::LanguageRegistry,
    semantic::{DEFAULT_RESTRICTED_GLOBALS, SemanticRule},
};

use super::{
    config::{ProjectConfig, SemanticRuleConfig},
    config_error,
    partners::validate_partner_patterns,
    rule_message_error,
    rules::compile_globs,
};

pub(super) struct PreparedSemanticRule {
    pub(super) id: String,
    pub(super) language_id: Option<&'static str>,
    pub(super) include: GlobSet,
    pub(super) exclude: GlobSet,
    pub(super) rule: SemanticRule,
    pub(super) message: Option<String>,
    pub(super) partner_patterns: Vec<String>,
}

pub(super) fn prepare_semantic_rules(
    config: &ProjectConfig,
    requested_rule_ids: &[String],
    languages: &LanguageRegistry,
    config_display_path: &Path,
) -> Result<Vec<PreparedSemanticRule>, Vec<Diagnostic>> {
    if config.semantic_rules.is_empty() {
        return Ok(Vec::new());
    }
    let requested = requested_rule_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let mut prepared = Vec::new();
    let mut diagnostics = Vec::new();
    for config_rule in &config.semantic_rules {
        if !requested.is_empty() && !requested.contains(config_rule.id()) {
            continue;
        }
        match prepare_semantic_rule(config_rule, languages, config_display_path) {
            Ok(rule) => prepared.push(rule),
            Err(mut errors) => diagnostics.append(&mut errors),
        }
    }
    if diagnostics.is_empty() {
        Ok(prepared)
    } else {
        Err(diagnostics)
    }
}

fn prepare_semantic_rule(
    config_rule: &SemanticRuleConfig,
    languages: &LanguageRegistry,
    config_display_path: &Path,
) -> Result<PreparedSemanticRule, Vec<Diagnostic>> {
    let language_id = resolved_language_id(config_rule, languages, config_display_path)?;
    if config_rule.include().is_empty() {
        return Err(vec![config_error(
            config_display_path,
            format!(
                "Semantic rule `{}` must contain at least one include pattern.",
                config_rule.id()
            ),
        )]);
    }
    if let Some(error) = rule_message_error(config_rule.message().map(String::as_str)) {
        return Err(vec![config_error(
            config_display_path,
            format!("Semantic rule `{}` {error}", config_rule.id()),
        )]);
    }
    let include = compile_globs(
        config_rule.include(),
        config_rule.id(),
        "include",
        config_display_path,
    )?;
    let exclude = compile_globs(
        config_rule.exclude(),
        config_rule.id(),
        "exclude",
        config_display_path,
    )?;
    let rule = build_semantic_rule(config_rule, config_display_path)?;
    let partner_patterns = partner_patterns(config_rule, config_display_path)?;
    Ok(PreparedSemanticRule {
        id: config_rule.id().to_string(),
        language_id,
        include,
        exclude,
        rule,
        message: config_rule.message().cloned(),
        partner_patterns,
    })
}

fn resolved_language_id(
    config_rule: &SemanticRuleConfig,
    languages: &LanguageRegistry,
    config_display_path: &Path,
) -> Result<Option<&'static str>, Vec<Diagnostic>> {
    match config_rule.language_id() {
        Some(language) => match languages.by_id(language) {
            Some(adapter) => Ok(Some(adapter.id())),
            None => Err(vec![config_error(
                config_display_path,
                format!(
                    "Semantic rule `{}` needs the `{language}` language adapter, which is not registered.",
                    config_rule.id()
                ),
            )]),
        },
        None => Ok(None),
    }
}

fn build_semantic_rule(
    config_rule: &SemanticRuleConfig,
    config_display_path: &Path,
) -> Result<SemanticRule, Vec<Diagnostic>> {
    match config_rule {
        SemanticRuleConfig::ReturnPaths { type_name, .. } => {
            if type_name.is_empty() {
                return Err(vec![config_error(
                    config_display_path,
                    format!(
                        "Semantic rule `{}` has an empty typeName.",
                        config_rule.id()
                    ),
                )]);
            }
            Ok(SemanticRule::ReturnPaths {
                type_name: type_name.clone(),
            })
        }
        SemanticRuleConfig::ExceptionPolicy {
            forbid_try,
            forbid_throw,
            forbid_promise_reject,
            exception_sources,
            ..
        } => Ok(SemanticRule::ExceptionPolicy {
            forbid_try: *forbid_try,
            forbid_throw: *forbid_throw,
            forbid_promise_reject: *forbid_promise_reject,
            exception_sources: compile_call_patterns(
                exception_sources,
                config_rule.id(),
                config_display_path,
            )?,
        }),
        SemanticRuleConfig::CompanionFile { .. } => Ok(SemanticRule::CompanionFile),
        SemanticRuleConfig::FileLength { max_lines, .. } => {
            if *max_lines == 0 {
                return Err(vec![config_error(
                    config_display_path,
                    format!("Semantic rule `{}` has a maxLines of 0.", config_rule.id()),
                )]);
            }
            Ok(SemanticRule::FileLength {
                max_lines: *max_lines,
            })
        }
        SemanticRuleConfig::HeaderSourcePairing {
            missing_source,
            check_order,
            ..
        } => Ok(SemanticRule::HeaderSourcePairing {
            missing_source: *missing_source,
            check_order: *check_order,
        }),
        SemanticRuleConfig::AccidentalGlobals {
            allow, allow_write, ..
        } => Ok(SemanticRule::AccidentalGlobals {
            allow: lua_names(allow, "allow", config_rule.id(), config_display_path)?,
            allow_write: lua_names(
                allow_write,
                "allowWrite",
                config_rule.id(),
                config_display_path,
            )?,
        }),
        SemanticRuleConfig::RestrictedGlobals {
            forbid,
            forbid_dynamic_require,
            ..
        } => {
            let forbid = match forbid {
                Some(paths) => lua_paths(paths, config_rule.id(), config_display_path)?,
                None => DEFAULT_RESTRICTED_GLOBALS
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            };
            Ok(SemanticRule::RestrictedGlobals {
                forbid,
                forbid_dynamic_require: *forbid_dynamic_require,
            })
        }
    }
}

/// A Lua name: ASCII letters, digits, and `_`, not starting with a digit.
fn is_lua_name(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn lua_names(
    names: &[String],
    field: &str,
    rule_id: &str,
    config_display_path: &Path,
) -> Result<BTreeSet<String>, Vec<Diagnostic>> {
    let invalid = names
        .iter()
        .filter(|name| !is_lua_name(name))
        .map(|name| {
            config_error(
                config_display_path,
                format!(
                    "Semantic rule `{rule_id}` has the invalid Lua name `{name}` in `{field}`."
                ),
            )
        })
        .collect::<Vec<_>>();
    if invalid.is_empty() {
        Ok(names.iter().cloned().collect())
    } else {
        Err(invalid)
    }
}

/// Names that are joined with dots, such as `string.dump`.
fn lua_paths(
    paths: &[String],
    rule_id: &str,
    config_display_path: &Path,
) -> Result<Vec<String>, Vec<Diagnostic>> {
    let invalid = paths
        .iter()
        .filter(|path| !path.split('.').all(is_lua_name))
        .map(|path| {
            config_error(
                config_display_path,
                format!(
                    "Semantic rule `{rule_id}` has the invalid Lua name path `{path}` in `forbid`."
                ),
            )
        })
        .collect::<Vec<_>>();
    if invalid.is_empty() {
        Ok(paths.to_vec())
    } else {
        Err(invalid)
    }
}

fn partner_patterns(
    config_rule: &SemanticRuleConfig,
    config_display_path: &Path,
) -> Result<Vec<String>, Vec<Diagnostic>> {
    match config_rule.partner_patterns() {
        Some((field, patterns)) => {
            validate_partner_patterns(patterns, config_rule.id(), field, config_display_path)?;
            Ok(patterns.to_vec())
        }
        None => Ok(Vec::new()),
    }
}

fn compile_call_patterns(
    patterns: &[String],
    rule_id: &str,
    config_display_path: &Path,
) -> Result<GlobSet, Vec<Diagnostic>> {
    let mut builder = GlobSetBuilder::new();
    let mut diagnostics = Vec::new();
    for pattern in patterns {
        if pattern.is_empty() {
            diagnostics.push(config_error(
                config_display_path,
                format!("Semantic rule `{rule_id}` has an empty exception source."),
            ));
            continue;
        }
        match GlobBuilder::new(pattern)
            .literal_separator(false)
            .backslash_escape(false)
            .case_insensitive(false)
            .build()
        {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(error) => diagnostics.push(config_error(
                config_display_path,
                format!(
                    "Semantic rule `{rule_id}` has invalid exception source `{pattern}`: {error}."
                ),
            )),
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    builder.build().map_err(|error| {
        vec![config_error(
            config_display_path,
            format!("Cannot compile exception sources for semantic rule `{rule_id}`: {error}."),
        )]
    })
}
