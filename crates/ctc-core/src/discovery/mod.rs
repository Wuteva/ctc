use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::{
    diagnostic::{Diagnostic, DiagnosticCategory, TextRange},
    language::LanguageRegistry,
    matcher::{MatchCount, MatchMode, SearchScope},
    semantic::SemanticRule,
};

mod config;
mod coverage;
mod files;
mod partners;
mod rules;
mod semantic_rules;
#[cfg(test)]
mod tests;

pub(crate) use config::resolve_config_path;
use config::{ignorable_rule_ids, read_config, validate_rule_ids};
pub use coverage::uncovered_files;
#[cfg(test)]
use coverage::{fixed_directory, is_inside_directory};
use files::{
    ExplicitPath, display_path, matches_explicit_filter, normalize_existing,
    normalize_explicit_paths,
};
pub(crate) use files::{collect_source_files, lenient_globs};
use partners::resolve_partner;
#[cfg(test)]
use partners::{check_partner_pattern, expand_partner_pattern};
pub use rules::validate_template_filename;
use rules::{PreparedRule, prepare_rules};
use semantic_rules::{PreparedSemanticRule, prepare_semantic_rules};

const DEFAULT_CONFIG_NAME: &str = ".ctc.json";

#[derive(Clone, Debug)]
pub struct RuleApplication {
    pub id: String,
    pub language_id: &'static str,
    pub source_path: PathBuf,
    pub source_display_path: PathBuf,
    pub template_path: PathBuf,
    pub template_display_path: PathBuf,
    pub template_source: String,
    /// The other templates of a rule that lists several. The rule accepts a
    /// file that any one of its templates accepts.
    pub alternatives: Vec<AlternativeTemplate>,
    pub mode: MatchMode,
    pub scope: SearchScope,
    pub message: Option<String>,
    pub kinds: Vec<String>,
    /// The node kinds of an `inside` scope. Empty for the other scopes.
    pub inside: Vec<String>,
    pub stop_at_functions: bool,
    pub count: Option<MatchCount>,
}

#[derive(Clone, Debug)]
pub struct AlternativeTemplate {
    pub path: PathBuf,
    pub display_path: PathBuf,
    pub source: String,
}

pub struct SemanticRuleApplication {
    pub id: String,
    pub language_id: &'static str,
    pub source_path: PathBuf,
    pub source_display_path: PathBuf,
    pub rule: SemanticRule,
    pub message: Option<String>,
    /// The first existing partner file, for rules that use partner patterns.
    pub partner: Option<PartnerFile>,
    /// The partner paths that were tried, in pattern order.
    pub partner_candidates: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartnerFile {
    pub path: PathBuf,
    pub display_path: PathBuf,
}

pub struct DiscoveryResult {
    pub template_rules: Vec<RuleApplication>,
    pub semantic_rules: Vec<SemanticRuleApplication>,
    /// Every rule identifier in the configuration, including rules that the
    /// command line did not select.
    pub configured_rule_ids: BTreeSet<String>,
    /// The configured rules that set `allowIgnore` to true, including rules
    /// that the command line did not select.
    pub ignorable_rule_ids: BTreeSet<String>,
}

struct DiscoveryContext {
    root: PathBuf,
    config_display_path: PathBuf,
    configured_rule_ids: BTreeSet<String>,
    ignorable_rule_ids: BTreeSet<String>,
    rules: Vec<PreparedRule>,
    semantic_rules: Vec<PreparedSemanticRule>,
    source_files: Vec<PathBuf>,
}

struct DiscoveryState {
    explicit: Vec<ExplicitPath>,
    matched_explicit_files: BTreeSet<PathBuf>,
    applications: Vec<RuleApplication>,
    semantic_applications: Vec<SemanticRuleApplication>,
    diagnostics: Vec<Diagnostic>,
}

pub fn discover(
    root: &Path,
    config_path: Option<&Path>,
    explicit_paths: &[PathBuf],
    rule_ids: &[String],
    languages: &LanguageRegistry,
) -> Result<Vec<RuleApplication>, Vec<Diagnostic>> {
    discover_all(root, config_path, explicit_paths, rule_ids, languages)
        .map(|result| result.template_rules)
}

pub fn discover_all(
    root: &Path,
    config_path: Option<&Path>,
    explicit_paths: &[PathBuf],
    rule_ids: &[String],
    languages: &LanguageRegistry,
) -> Result<DiscoveryResult, Vec<Diagnostic>> {
    let context = prepare_discovery_context(root, config_path, rule_ids, languages)?;
    let mut state = DiscoveryState::new(normalize_explicit_paths(&context.root, explicit_paths)?);
    apply_template_rules(&context, &mut state);
    apply_semantic_rules(&context, languages, &mut state);
    finish_discovery(context, state)
}

impl DiscoveryState {
    fn new(explicit: Vec<ExplicitPath>) -> Self {
        Self {
            explicit,
            matched_explicit_files: BTreeSet::new(),
            applications: Vec::new(),
            semantic_applications: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

fn prepare_discovery_context(
    root: &Path,
    config_path: Option<&Path>,
    rule_ids: &[String],
    languages: &LanguageRegistry,
) -> Result<DiscoveryContext, Vec<Diagnostic>> {
    let root = normalize_existing(root).map_err(invalid_root_diagnostics)?;
    let config_path = resolve_config_path(&root, config_path)?;
    let config_display_path = PathBuf::from(display_path(&root, &config_path));
    let config = read_config(&config_path, &config_display_path)?;
    let configured_rule_ids = validate_rule_ids(&config, rule_ids, &config_display_path)?;
    let ignorable_rule_ids = ignorable_rule_ids(&config);
    let rules = prepare_rules(&root, &config, rule_ids, languages, &config_display_path)?;
    let semantic_rules =
        prepare_semantic_rules(&config, rule_ids, languages, &config_display_path)?;
    let source_files = sorted_source_files(&root, languages)?;
    Ok(DiscoveryContext {
        root,
        config_display_path,
        configured_rule_ids,
        ignorable_rule_ids,
        rules,
        semantic_rules,
        source_files,
    })
}

fn invalid_root_diagnostics(message: String) -> Vec<Diagnostic> {
    vec![Diagnostic::new(
        "CTC0001",
        DiagnosticCategory::InvalidCommandLine,
        message,
        2,
    )]
}

fn sorted_source_files(
    root: &Path,
    languages: &LanguageRegistry,
) -> Result<Vec<PathBuf>, Vec<Diagnostic>> {
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

fn apply_template_rules(context: &DiscoveryContext, state: &mut DiscoveryState) {
    for rule in &context.rules {
        let selected = selected_template_files(context, rule);
        if selected.is_empty() {
            state.diagnostics.push(
                Diagnostic::new(
                    "CTC1003",
                    DiagnosticCategory::ConfiguredRuleSelectsNoFiles,
                    format!("Rule `{}` includes no source files.", rule.id),
                    2,
                )
                .with_template(empty_range(&context.config_display_path)),
            );
            continue;
        }
        for source_path in selected {
            if !matches_explicit_filter(&source_path, &state.explicit) {
                continue;
            }
            maybe_record_explicit_match(
                &state.explicit,
                &source_path,
                &mut state.matched_explicit_files,
            );
            state
                .applications
                .push(build_template_application(context, rule, source_path));
        }
    }
}

fn selected_template_files(context: &DiscoveryContext, rule: &PreparedRule) -> Vec<PathBuf> {
    context
        .source_files
        .iter()
        .filter(|source_path| {
            let relative = display_path(&context.root, source_path);
            rule.include.is_match(&relative) && !rule.exclude.is_match(&relative)
        })
        .cloned()
        .collect()
}

fn build_template_application(
    context: &DiscoveryContext,
    rule: &PreparedRule,
    source_path: PathBuf,
) -> RuleApplication {
    RuleApplication {
        id: rule.id.clone(),
        language_id: rule.language_id,
        source_display_path: PathBuf::from(display_path(&context.root, &source_path)),
        source_path,
        template_path: rule.templates[0].path.clone(),
        template_display_path: rule.templates[0].display_path.clone(),
        template_source: rule.templates[0].source.clone(),
        alternatives: rule.templates[1..]
            .iter()
            .map(|template| AlternativeTemplate {
                path: template.path.clone(),
                display_path: template.display_path.clone(),
                source: template.source.clone(),
            })
            .collect(),
        mode: rule.mode,
        scope: rule.scope,
        message: rule.message.clone(),
        kinds: rule.kinds.clone(),
        inside: rule.inside.clone(),
        stop_at_functions: rule.stop_at_functions,
        count: rule.count,
    }
}

fn apply_semantic_rules(
    context: &DiscoveryContext,
    languages: &LanguageRegistry,
    state: &mut DiscoveryState,
) {
    for rule in &context.semantic_rules {
        let selected = selected_semantic_files(context, rule, languages);
        if selected.is_empty() {
            state.diagnostics.push(
                Diagnostic::new(
                    "CTC1003",
                    DiagnosticCategory::ConfiguredRuleSelectsNoFiles,
                    format!("Semantic rule `{}` includes no source files.", rule.id),
                    2,
                )
                .with_template(empty_range(&context.config_display_path)),
            );
            continue;
        }
        for source_path in selected {
            if !matches_explicit_filter(&source_path, &state.explicit) {
                continue;
            }
            maybe_record_explicit_match(
                &state.explicit,
                &source_path,
                &mut state.matched_explicit_files,
            );
            let Some(adapter) = languages.for_path(&source_path) else {
                continue;
            };
            state.semantic_applications.push(build_semantic_application(
                context,
                rule,
                source_path,
                adapter.id(),
            ));
        }
    }
}

fn selected_semantic_files(
    context: &DiscoveryContext,
    rule: &PreparedSemanticRule,
    languages: &LanguageRegistry,
) -> Vec<PathBuf> {
    context
        .source_files
        .iter()
        .filter(|source_path| {
            let relative = display_path(&context.root, source_path);
            languages.for_path(source_path).is_some_and(|adapter| {
                rule.language_id
                    .is_none_or(|language| adapter.id() == language)
            }) && rule.include.is_match(&relative)
                && !rule.exclude.is_match(&relative)
        })
        .cloned()
        .collect()
}

fn build_semantic_application(
    context: &DiscoveryContext,
    rule: &PreparedSemanticRule,
    source_path: PathBuf,
    language_id: &'static str,
) -> SemanticRuleApplication {
    let source_display_path = display_path(&context.root, &source_path);
    let (partner, partner_candidates) = if rule.partner_patterns.is_empty() {
        (None, Vec::new())
    } else {
        resolve_partner(&context.root, &source_display_path, &rule.partner_patterns)
    };
    SemanticRuleApplication {
        id: rule.id.clone(),
        language_id,
        source_display_path: PathBuf::from(source_display_path),
        source_path,
        rule: rule.rule.clone(),
        message: rule.message.clone(),
        partner,
        partner_candidates,
    }
}

fn maybe_record_explicit_match(
    explicit: &[ExplicitPath],
    source_path: &Path,
    matched_explicit_files: &mut BTreeSet<PathBuf>,
) {
    if explicit
        .iter()
        .any(|path| path.is_file && path.path == source_path)
    {
        matched_explicit_files.insert(source_path.to_path_buf());
    }
}

fn finish_discovery(
    mut context: DiscoveryContext,
    mut state: DiscoveryState,
) -> Result<DiscoveryResult, Vec<Diagnostic>> {
    push_unmatched_explicit_diagnostics(
        &context.root,
        &state.explicit,
        &state.matched_explicit_files,
        &mut state.diagnostics,
    );
    if state.applications.is_empty()
        && state.semantic_applications.is_empty()
        && state.diagnostics.is_empty()
    {
        state.diagnostics.push(Diagnostic::new(
            "CTC1006",
            DiagnosticCategory::NoSelectedSourceFiles,
            "No source files were selected.",
            2,
        ));
    }
    if !state.diagnostics.is_empty() {
        return Err(state.diagnostics);
    }
    sort_applications(&mut state.applications, &mut state.semantic_applications);
    Ok(DiscoveryResult {
        template_rules: state.applications,
        semantic_rules: state.semantic_applications,
        configured_rule_ids: std::mem::take(&mut context.configured_rule_ids),
        ignorable_rule_ids: std::mem::take(&mut context.ignorable_rule_ids),
    })
}

fn push_unmatched_explicit_diagnostics(
    root: &Path,
    explicit: &[ExplicitPath],
    matched_explicit_files: &BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for path in explicit.iter().filter(|path| path.is_file) {
        if !matched_explicit_files.contains(&path.path) {
            diagnostics.push(Diagnostic::new(
                "CTC1007",
                DiagnosticCategory::NoApplicableRule,
                format!(
                    "No configured rule applies to `{}`.",
                    display_path(root, &path.path)
                ),
                2,
            ));
        }
    }
}

fn sort_applications(
    applications: &mut [RuleApplication],
    semantic_applications: &mut [SemanticRuleApplication],
) {
    applications.sort_by(|left, right| {
        left.source_display_path
            .cmp(&right.source_display_path)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.template_display_path.cmp(&right.template_display_path))
    });
    semantic_applications.sort_by(|left, right| {
        left.source_display_path
            .cmp(&right.source_display_path)
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn empty_range(path: &Path) -> TextRange {
    TextRange::from_offsets(path, "", 0, 0)
}

fn config_error(path: &Path, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(
        "CTC1010",
        DiagnosticCategory::InvalidConfiguration,
        message,
        2,
    )
    .with_template(empty_range(path))
}

/// Returns the end of an error sentence when a configured rule message is not valid.
pub fn rule_message_error(message: Option<&str>) -> Option<&'static str> {
    let message = message?;
    if message.trim().is_empty() {
        Some("has an empty message.")
    } else if message.contains(['\n', '\r']) {
        Some("has a message with a line break. Use one line.")
    } else {
        None
    }
}
