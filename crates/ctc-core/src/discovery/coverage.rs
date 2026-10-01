use std::path::Path;

use crate::{
    diagnostic::{Diagnostic, DiagnosticCategory, TextRange},
    language::LanguageRegistry,
};

use super::{
    config::{read_config, validate_rule_ids},
    files::{collect_source_files, display_path, normalize_existing},
    resolve_config_path,
    rules::prepare_rules,
    semantic_rules::prepare_semantic_rules,
};

/// Lists the source files that sit in the watched area but that no rule
/// includes. The watched area is the fixed directory at the start of every
/// `include` pattern. A file that a rule includes and then excludes counts as
/// covered, because the exclusion is a visible decision in the configuration.
pub fn uncovered_files(
    root: &Path,
    config_path: Option<&Path>,
    languages: &LanguageRegistry,
) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let root = normalize_existing(root).map_err(|message| {
        vec![Diagnostic::new(
            "CTC0001",
            DiagnosticCategory::InvalidCommandLine,
            message,
            2,
        )]
    })?;
    let config_path = resolve_config_path(&root, config_path)?;
    let config_display_path = std::path::PathBuf::from(display_path(&root, &config_path));
    let config = read_config(&config_path, &config_display_path)?;
    validate_rule_ids(&config, &[], &config_display_path)?;
    let rules = prepare_rules(&root, &config, &[], languages, &config_display_path)?;
    let semantic_rules = prepare_semantic_rules(&config, &[], languages, &config_display_path)?;
    let watched = watched_directories(&config);
    let source_files = sorted_source_files(&root, languages)?;
    Ok(uncovered_diagnostics(
        &root,
        &source_files,
        &watched,
        &rules,
        &semantic_rules,
        languages,
    ))
}

fn watched_directories(config: &super::config::ProjectConfig) -> Vec<String> {
    config
        .rules
        .iter()
        .flat_map(|rule| rule.include.iter())
        .chain(
            config
                .semantic_rules
                .iter()
                .flat_map(|rule| rule.include().iter()),
        )
        .map(|pattern| fixed_directory(pattern))
        .collect()
}

fn sorted_source_files(
    root: &Path,
    languages: &LanguageRegistry,
) -> Result<Vec<std::path::PathBuf>, Vec<Diagnostic>> {
    let mut source_files = Vec::new();
    collect_source_files(root, languages, &mut source_files).map_err(|message| {
        vec![Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            message,
            2,
        )]
    })?;
    source_files.sort();
    source_files.dedup();
    Ok(source_files)
}

fn uncovered_diagnostics(
    root: &Path,
    source_files: &[std::path::PathBuf],
    watched: &[String],
    rules: &[super::rules::PreparedRule],
    semantic_rules: &[super::semantic_rules::PreparedSemanticRule],
    languages: &LanguageRegistry,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for source_path in source_files {
        let relative = display_path(root, source_path);
        if !watched
            .iter()
            .any(|directory| is_inside_directory(&relative, directory))
        {
            continue;
        }
        let Some(adapter) = languages.for_path(source_path) else {
            continue;
        };
        let covered = rules
            .iter()
            .any(|rule| rule.language_id == adapter.id() && rule.include.is_match(&relative))
            || semantic_rules.iter().any(|rule| {
                rule.language_id
                    .is_none_or(|language| language == adapter.id())
                    && rule.include.is_match(&relative)
            });
        if !covered {
            diagnostics.push(
                Diagnostic::new(
                    "CTC5101",
                    DiagnosticCategory::UncoveredSourceFile,
                    "The file is not selected by any rule.",
                    1,
                )
                .with_source(TextRange::from_offsets(
                    Path::new(&relative),
                    "",
                    0,
                    0,
                )),
            );
        }
    }
    diagnostics
}

/// The directory that an include pattern names before its first wildcard.
/// A pattern without a wildcard names a file, so its directory is the parent.
pub(super) fn fixed_directory(pattern: &str) -> String {
    let segments = pattern.split('/').collect::<Vec<_>>();
    let wildcard = segments
        .iter()
        .position(|segment| segment.contains(['*', '?', '[', ']', '{', '}']));
    let fixed = match wildcard {
        Some(index) => &segments[..index],
        None => &segments[..segments.len().saturating_sub(1)],
    };
    fixed
        .iter()
        .filter(|segment| !segment.is_empty() && **segment != ".")
        .copied()
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn is_inside_directory(relative: &str, directory: &str) -> bool {
    if directory.is_empty() {
        return true;
    }
    let (relative, directory) = if cfg!(windows) {
        (
            relative.to_ascii_lowercase(),
            directory.to_ascii_lowercase(),
        )
    } else {
        (relative.to_string(), directory.to_string())
    };
    relative
        .strip_prefix(&directory)
        .is_some_and(|rest| rest.starts_with('/'))
}
