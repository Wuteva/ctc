use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{
    diagnostic::{Diagnostic, DiagnosticCategory, TextRange},
    matcher::{MatchMode, SearchScope},
    semantic::MissingPartner,
};

use super::{DEFAULT_CONFIG_NAME, config_error, empty_range, normalize_existing};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ProjectConfig {
    pub(super) schema_version: u32,
    #[serde(default)]
    pub(super) rules: Vec<RuleConfig>,
    #[serde(default)]
    pub(super) semantic_rules: Vec<SemanticRuleConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RuleConfig {
    pub(super) id: String,
    pub(super) template: TemplateConfig,
    pub(super) include: Vec<String>,
    #[serde(default)]
    pub(super) exclude: Vec<String>,
    #[serde(default)]
    pub(super) mode: MatchMode,
    #[serde(default)]
    pub(super) scope: ScopeConfig,
    #[serde(default)]
    pub(super) message: Option<String>,
    pub(super) kinds: Option<Vec<String>>,
    #[serde(default)]
    pub(super) count: Option<CountConfig>,
    #[serde(default)]
    pub(super) allow_ignore: bool,
}

/// A rule lists one template, or several that are alternatives.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum TemplateConfig {
    One(PathBuf),
    Many(Vec<PathBuf>),
}

impl TemplateConfig {
    pub(super) fn paths(&self) -> Vec<&PathBuf> {
        match self {
            Self::One(path) => vec![path],
            Self::Many(paths) => paths.iter().collect(),
        }
    }
}

/// A rule scope is a name or an object that lists node kinds.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum ScopeConfig {
    Named(SearchScope),
    Inside(InsideScopeConfig),
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self::Named(SearchScope::default())
    }
}

impl ScopeConfig {
    pub(super) fn search_scope(&self) -> SearchScope {
        match self {
            Self::Named(scope) => *scope,
            Self::Inside(_) => SearchScope::Inside,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct InsideScopeConfig {
    pub(super) inside: Vec<String>,
    #[serde(default = "default_true")]
    pub(super) stop_at_functions: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CountConfig {
    #[serde(default = "default_count_min")]
    pub(super) min: u32,
    pub(super) max: Option<u32>,
}

fn default_count_min() -> u32 {
    1
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum SemanticRuleConfig {
    ReturnPaths {
        id: String,
        include: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        type_name: String,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        allow_ignore: bool,
    },
    ExceptionPolicy {
        id: String,
        include: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        #[serde(default)]
        forbid_try: bool,
        #[serde(default)]
        forbid_throw: bool,
        #[serde(default)]
        forbid_promise_reject: bool,
        #[serde(default)]
        exception_sources: Vec<String>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        allow_ignore: bool,
    },
    CompanionFile {
        id: String,
        include: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        companions: Vec<String>,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        allow_ignore: bool,
    },
    FileLength {
        id: String,
        include: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        max_lines: u32,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        allow_ignore: bool,
    },
    HeaderSourcePairing {
        id: String,
        include: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        sources: Vec<String>,
        #[serde(default)]
        missing_source: MissingPartner,
        #[serde(default = "default_true")]
        check_order: bool,
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        allow_ignore: bool,
    },
}

impl SemanticRuleConfig {
    pub(super) fn id(&self) -> &str {
        match self {
            Self::ReturnPaths { id, .. }
            | Self::ExceptionPolicy { id, .. }
            | Self::CompanionFile { id, .. }
            | Self::FileLength { id, .. }
            | Self::HeaderSourcePairing { id, .. } => id,
        }
    }

    pub(super) fn include(&self) -> &[String] {
        match self {
            Self::ReturnPaths { include, .. }
            | Self::ExceptionPolicy { include, .. }
            | Self::CompanionFile { include, .. }
            | Self::FileLength { include, .. }
            | Self::HeaderSourcePairing { include, .. } => include,
        }
    }

    pub(super) fn exclude(&self) -> &[String] {
        match self {
            Self::ReturnPaths { exclude, .. }
            | Self::ExceptionPolicy { exclude, .. }
            | Self::CompanionFile { exclude, .. }
            | Self::FileLength { exclude, .. }
            | Self::HeaderSourcePairing { exclude, .. } => exclude,
        }
    }

    /// The language adapter a rule selects, or `None` for any language.
    pub(super) fn language_id(&self) -> Option<&'static str> {
        match self {
            Self::ReturnPaths { .. } | Self::ExceptionPolicy { .. } => Some("typescript"),
            Self::HeaderSourcePairing { .. } => Some("cpp"),
            Self::CompanionFile { .. } | Self::FileLength { .. } => None,
        }
    }

    pub(super) fn partner_patterns(&self) -> Option<(&'static str, &[String])> {
        match self {
            Self::CompanionFile { companions, .. } => Some(("companions", companions)),
            Self::HeaderSourcePairing { sources, .. } => Some(("sources", sources)),
            Self::ReturnPaths { .. } | Self::ExceptionPolicy { .. } | Self::FileLength { .. } => {
                None
            }
        }
    }

    pub(super) fn message(&self) -> Option<&String> {
        match self {
            Self::ReturnPaths { message, .. }
            | Self::ExceptionPolicy { message, .. }
            | Self::CompanionFile { message, .. }
            | Self::FileLength { message, .. }
            | Self::HeaderSourcePairing { message, .. } => message.as_ref(),
        }
    }

    pub(super) fn allow_ignore(&self) -> bool {
        match self {
            Self::ReturnPaths { allow_ignore, .. }
            | Self::ExceptionPolicy { allow_ignore, .. }
            | Self::CompanionFile { allow_ignore, .. }
            | Self::FileLength { allow_ignore, .. }
            | Self::HeaderSourcePairing { allow_ignore, .. } => *allow_ignore,
        }
    }
}

fn default_true() -> bool {
    true
}

pub(super) fn ignorable_rule_ids(config: &ProjectConfig) -> BTreeSet<String> {
    config
        .rules
        .iter()
        .filter(|rule| rule.allow_ignore)
        .map(|rule| rule.id.clone())
        .chain(
            config
                .semantic_rules
                .iter()
                .filter(|rule| rule.allow_ignore())
                .map(|rule| rule.id().to_string()),
        )
        .collect()
}

pub(crate) fn resolve_config_path(
    root: &Path,
    config_path: Option<&Path>,
) -> Result<PathBuf, Vec<Diagnostic>> {
    let configured = config_path
        .map(|path| {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join(path)
            }
        })
        .unwrap_or_else(|| root.join(DEFAULT_CONFIG_NAME));
    let normalized = normalize_existing(&configured).map_err(|_| {
        vec![Diagnostic::new(
            "CTC1010",
            DiagnosticCategory::InvalidConfiguration,
            format!("Cannot find configuration `{}`.", configured.display()),
            2,
        )]
    })?;
    if !normalized.starts_with(root) {
        return Err(vec![Diagnostic::new(
            "CTC1010",
            DiagnosticCategory::InvalidConfiguration,
            "The configuration file must be inside the scan root.",
            2,
        )]);
    }
    Ok(normalized)
}

pub(super) fn read_config(
    path: &Path,
    display_path: &Path,
) -> Result<ProjectConfig, Vec<Diagnostic>> {
    let bytes = fs::read(path).map_err(|error| {
        vec![
            Diagnostic::new(
                "CTC1010",
                DiagnosticCategory::InvalidConfiguration,
                format!("Cannot read configuration: {error}"),
                2,
            )
            .with_template(empty_range(display_path)),
        ]
    })?;
    let source = String::from_utf8(bytes).map_err(|_| {
        vec![
            Diagnostic::new(
                "CTC1009",
                DiagnosticCategory::InvalidUtf8,
                "The configuration is not valid UTF-8.",
                2,
            )
            .with_template(empty_range(display_path)),
        ]
    })?;
    let config = serde_json::from_str::<ProjectConfig>(&source).map_err(|error| {
        vec![
            Diagnostic::new(
                "CTC1010",
                DiagnosticCategory::InvalidConfiguration,
                format!("Invalid JSON configuration: {error}."),
                2,
            )
            .with_template(TextRange::from_offsets(
                display_path,
                &source,
                0,
                source.len(),
            )),
        ]
    })?;
    validate_schema_version(&config, display_path, &source)?;
    validate_non_empty_config(&config, display_path)?;
    Ok(config)
}

fn validate_schema_version(
    config: &ProjectConfig,
    display_path: &Path,
    source: &str,
) -> Result<(), Vec<Diagnostic>> {
    if config.schema_version == 1 {
        return Ok(());
    }
    Err(vec![
        Diagnostic::new(
            "CTC1010",
            DiagnosticCategory::InvalidConfiguration,
            format!(
                "Unsupported configuration schema version `{}`.",
                config.schema_version
            ),
            2,
        )
        .with_template(TextRange::from_offsets(
            display_path,
            source,
            0,
            source.len(),
        )),
    ])
}

fn validate_non_empty_config(
    config: &ProjectConfig,
    display_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    if !config.rules.is_empty() || !config.semantic_rules.is_empty() {
        return Ok(());
    }
    Err(vec![
        Diagnostic::new(
            "CTC1010",
            DiagnosticCategory::InvalidConfiguration,
            "The configuration must contain at least one template or semantic rule.",
            2,
        )
        .with_template(empty_range(display_path)),
    ])
}

pub(super) fn validate_rule_ids(
    config: &ProjectConfig,
    requested_rule_ids: &[String],
    config_display_path: &Path,
) -> Result<BTreeSet<String>, Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let mut configured_ids = BTreeSet::new();
    for id in config
        .rules
        .iter()
        .map(|rule| rule.id.as_str())
        .chain(config.semantic_rules.iter().map(SemanticRuleConfig::id))
    {
        validate_configured_rule_id(
            id,
            config_display_path,
            &mut diagnostics,
            &mut configured_ids,
        );
    }
    for requested in requested_rule_ids {
        if !configured_ids.contains(requested) {
            diagnostics.push(Diagnostic::new(
                "CTC1007",
                DiagnosticCategory::NoApplicableRule,
                format!("Unknown rule identifier `{requested}`."),
                2,
            ));
        }
    }
    if diagnostics.is_empty() {
        Ok(configured_ids)
    } else {
        Err(diagnostics)
    }
}

fn validate_configured_rule_id(
    id: &str,
    config_display_path: &Path,
    diagnostics: &mut Vec<Diagnostic>,
    configured_ids: &mut BTreeSet<String>,
) {
    if !valid_rule_id(id) {
        diagnostics.push(config_error(
            config_display_path,
            format!("Invalid rule identifier `{id}`."),
        ));
    } else if !configured_ids.insert(id.to_string()) {
        diagnostics.push(config_error(
            config_display_path,
            format!("Duplicate rule identifier `{id}`."),
        ));
    }
}

fn valid_rule_id(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
