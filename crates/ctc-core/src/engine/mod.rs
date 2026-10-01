use std::path::{Path, PathBuf};

use crate::{
    canonical::{CanonicalNode, SemanticFacts},
    diagnostic::{Diagnostic, DiagnosticCategory, RunReport, TextRange},
    discovery::{self, DiscoveryResult, RuleApplication},
    language::LanguageRegistry,
    matcher::{MatchMode, MatchResult, SearchOptions, SearchScope, match_template_with_options},
    template::{CompiledTemplate, parse_placeholders},
};

mod runner;
#[cfg(test)]
mod tests;

#[cfg(test)]
use runner::combine_alternatives;
use runner::{Suppression, collect_supported_files, template_source_path};
pub(crate) use runner::{canonicalize_input, display_path, read_utf8, with_rule_message};

#[derive(Clone, Debug)]
pub struct ParsedSource {
    pub path: PathBuf,
    pub language_id: &'static str,
    pub source: String,
    pub root: CanonicalNode,
    pub semantic_facts: SemanticFacts,
    /// Byte ranges of the comments in the raw syntax tree, used to read
    /// suppression comments.
    pub comments: Vec<std::ops::Range<usize>>,
}

#[derive(Clone, Debug, Default)]
pub struct CheckOptions {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub paths: Vec<PathBuf>,
    pub rule_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct AdHocCheckOptions {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub template_path: PathBuf,
    pub mode_override: Option<MatchMode>,
    pub scope: SearchScope,
    pub message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CoverageOptions {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct DiscoverRulesOptions {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub paths: Vec<PathBuf>,
    pub rule_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CompileTemplateOptions {
    pub template_text: String,
    pub template_path: PathBuf,
    pub mode_override: Option<MatchMode>,
}

#[derive(Clone, Debug)]
pub struct ParseSourceOptions {
    pub source_text: String,
    pub source_path: PathBuf,
}

pub struct MatchTemplateOptions<'a> {
    pub rule_id: &'a str,
    pub template: &'a CompiledTemplate,
    pub source: &'a ParsedSource,
    pub scope: SearchScope,
    pub kinds: &'a [String],
    pub search: SearchOptions<'a>,
}

struct AdHocTemplate {
    path: PathBuf,
    display: PathBuf,
    source: String,
    language_id: &'static str,
    mode: MatchMode,
}

pub struct Engine {
    pub languages: LanguageRegistry,
}

impl Engine {
    pub fn new(languages: LanguageRegistry) -> Self {
        Self { languages }
    }

    pub fn discover_rules(
        &self,
        options: &DiscoverRulesOptions,
    ) -> Result<Vec<RuleApplication>, Vec<Diagnostic>> {
        discovery::discover(
            &options.root,
            options.config_path.as_deref(),
            &options.paths,
            &options.rule_ids,
            &self.languages,
        )
    }

    pub fn discover_all_rules(
        &self,
        options: &DiscoverRulesOptions,
    ) -> Result<DiscoveryResult, Vec<Diagnostic>> {
        discovery::discover_all(
            &options.root,
            options.config_path.as_deref(),
            &options.paths,
            &options.rule_ids,
            &self.languages,
        )
    }

    pub fn compile_template(
        &self,
        options: &CompileTemplateOptions,
    ) -> Result<CompiledTemplate, Vec<Diagnostic>> {
        let directives =
            crate::template::parse_directives(&options.template_text, &options.template_path)?;
        if directives.excluded {
            return Err(vec![
                Diagnostic::new(
                    "CTC1005",
                    DiagnosticCategory::InvalidTemplateDirective,
                    "An exclusion is not a match template.",
                    2,
                )
                .with_template(TextRange::from_offsets(
                    &options.template_path,
                    &options.template_text,
                    0,
                    0,
                )),
            ]);
        }
        let source_path = template_source_path(&options.template_path);
        let Some(adapter) = self.languages.for_path(&source_path) else {
            return Err(vec![
                Diagnostic::new(
                    "CTC1008",
                    DiagnosticCategory::UnsupportedSourceType,
                    "The template filename does not contain a supported source suffix.",
                    2,
                )
                .with_template(TextRange::from_offsets(
                    &options.template_path,
                    &options.template_text,
                    0,
                    0,
                )),
            ]);
        };
        let ranges = adapter.scan_placeholders(&options.template_text, &options.template_path)?;
        let placeholders = parse_placeholders(
            &options.template_text,
            &options.template_path,
            &ranges,
            adapter.as_ref(),
        )?;
        adapter.compile_template(
            &options.template_text,
            &options.template_path,
            &placeholders,
            options.mode_override.unwrap_or(directives.mode),
        )
    }

    pub fn parse_source(
        &self,
        options: &ParseSourceOptions,
    ) -> Result<ParsedSource, Vec<Diagnostic>> {
        let Some(adapter) = self.languages.for_path(&options.source_path) else {
            return Err(vec![
                Diagnostic::new(
                    "CTC1008",
                    DiagnosticCategory::UnsupportedSourceType,
                    "The source does not use a supported source type.",
                    2,
                )
                .with_source(TextRange::from_offsets(
                    &options.source_path,
                    &options.source_text,
                    0,
                    0,
                )),
            ]);
        };
        adapter.parse(&options.source_text, &options.source_path)
    }

    pub fn match_template(&self, options: &MatchTemplateOptions<'_>) -> MatchResult {
        if options.template.language_id != options.source.language_id {
            return MatchResult {
                matches: false,
                diagnostics: vec![
                    Diagnostic::new(
                        "CTC1008",
                        DiagnosticCategory::UnsupportedSourceType,
                        "The template and source use different language adapters.",
                        2,
                    )
                    .with_rule(options.rule_id)
                    .with_source(options.source.root.range.clone())
                    .with_template(options.template.root.range().clone()),
                ],
            };
        }
        let Some(adapter) = self.languages.for_path(&options.source.path) else {
            return MatchResult {
                matches: false,
                diagnostics: vec![
                    Diagnostic::new(
                        "CTC9001",
                        DiagnosticCategory::InternalToolError,
                        "The parsed source language adapter is not registered.",
                        2,
                    )
                    .with_rule(options.rule_id),
                ],
            };
        };
        match_template_with_options(
            options.rule_id,
            options.template,
            &options.source.root,
            options.scope,
            options.kinds,
            &options.search,
            &|value| adapter.validate_identifier(value),
        )
    }

    pub fn check(&self, options: &CheckOptions) -> RunReport {
        let discovered = match self.discover_all_rules(&DiscoverRulesOptions {
            root: options.root.clone(),
            config_path: options.config_path.clone(),
            paths: options.paths.clone(),
            rule_ids: options.rule_ids.clone(),
        }) {
            Ok(discovered) => discovered,
            Err(diagnostics) => return RunReport::new(diagnostics),
        };
        self.run_applications(
            discovered.template_rules,
            discovered.semantic_rules,
            Some(Suppression {
                configured_rule_ids: &discovered.configured_rule_ids,
                ignorable_rule_ids: &discovered.ignorable_rule_ids,
            }),
        )
    }

    /// Reports source files in the watched area that no rule selects.
    pub fn coverage(&self, options: &CoverageOptions) -> RunReport {
        match discovery::uncovered_files(
            &options.root,
            options.config_path.as_deref(),
            &self.languages,
        ) {
            Ok(diagnostics) | Err(diagnostics) => RunReport::new(diagnostics),
        }
    }

    pub fn check_ad_hoc(&self, options: &AdHocCheckOptions) -> RunReport {
        if options.paths.is_empty() {
            return RunReport::new(vec![Diagnostic::new(
                "CTC0001",
                DiagnosticCategory::InvalidCommandLine,
                "An ad hoc template requires one or more explicit source paths.",
                2,
            )]);
        }
        if discovery::rule_message_error(options.message.as_deref()).is_some() {
            return RunReport::new(vec![Diagnostic::new(
                "CTC0001",
                DiagnosticCategory::InvalidCommandLine,
                "`--message` must be one line of text that is not empty.",
                2,
            )]);
        }
        let root = match canonical_root(&options.root) {
            Ok(root) => root,
            Err(report) => return report,
        };
        let template = match self.prepare_ad_hoc_template(&root, options) {
            Ok(template) => template,
            Err(report) => return report,
        };
        let selected_sources = match self.select_ad_hoc_sources(&root, &options.paths) {
            Ok(selected_sources) => selected_sources,
            Err(report) => return report,
        };
        let applications =
            match self.build_ad_hoc_applications(&root, options, &template, selected_sources) {
                Ok(applications) => applications,
                Err(report) => return report,
            };
        self.run_applications(applications, Vec::new(), None)
    }

    fn prepare_ad_hoc_template(
        &self,
        root: &Path,
        options: &AdHocCheckOptions,
    ) -> Result<AdHocTemplate, RunReport> {
        let template_path = match canonicalize_input(root, &options.template_path) {
            Ok(path) => path,
            Err(diagnostic) => return Err(RunReport::new(vec![diagnostic])),
        };
        let template_display = display_path(root, &template_path);
        if let Err(diagnostic) =
            crate::discovery::validate_template_filename(&template_display, &self.languages)
        {
            return Err(RunReport::new(vec![diagnostic]));
        }
        let Some(template_adapter) = self
            .languages
            .for_path(&template_source_path(&template_path))
        else {
            return Err(RunReport::new(vec![Diagnostic::new(
                "CTC1008",
                DiagnosticCategory::UnsupportedSourceType,
                "The template filename does not contain a supported source suffix.",
                2,
            )]));
        };
        let template_source = match read_utf8(&template_path, &template_display, true) {
            Ok(source) => source,
            Err(diagnostic) => return Err(RunReport::new(vec![diagnostic])),
        };
        let directives =
            match crate::template::parse_directives(&template_source, &template_display) {
                Ok(directives) if !directives.excluded => directives,
                Ok(_) => {
                    return Err(RunReport::new(vec![Diagnostic::new(
                        "CTC1005",
                        DiagnosticCategory::InvalidTemplateDirective,
                        "An ad hoc template cannot be an exclusion.",
                        2,
                    )]));
                }
                Err(diagnostics) => return Err(RunReport::new(diagnostics)),
            };
        Ok(AdHocTemplate {
            path: template_path,
            display: template_display,
            source: template_source,
            language_id: template_adapter.id(),
            mode: options.mode_override.unwrap_or(directives.mode),
        })
    }

    fn select_ad_hoc_sources(
        &self,
        root: &Path,
        paths: &[PathBuf],
    ) -> Result<std::collections::BTreeSet<PathBuf>, RunReport> {
        let mut selected_sources = std::collections::BTreeSet::new();
        for path in paths {
            let source_path = match canonicalize_input(root, path) {
                Ok(path) => path,
                Err(diagnostic) => return Err(RunReport::new(vec![diagnostic])),
            };
            if !source_path.starts_with(root) {
                return Err(RunReport::new(vec![Diagnostic::new(
                    "CTC0001",
                    DiagnosticCategory::InvalidCommandLine,
                    format!(
                        "Explicit path `{}` is outside the scan root.",
                        path.display()
                    ),
                    2,
                )]));
            }
            if source_path.is_dir() {
                let mut selected = Vec::new();
                collect_supported_files(&source_path, &self.languages, &mut selected);
                for source_path in selected {
                    selected_sources.insert(source_path);
                }
            } else {
                selected_sources.insert(source_path);
            }
        }
        Ok(selected_sources)
    }

    fn build_ad_hoc_applications(
        &self,
        root: &Path,
        options: &AdHocCheckOptions,
        template: &AdHocTemplate,
        selected_sources: std::collections::BTreeSet<PathBuf>,
    ) -> Result<Vec<RuleApplication>, RunReport> {
        if selected_sources.is_empty() {
            return Err(RunReport::new(vec![Diagnostic::new(
                "CTC1006",
                DiagnosticCategory::NoSelectedSourceFiles,
                "No source files were selected.",
                2,
            )]));
        }
        let mut applications = Vec::new();
        for source_path in selected_sources {
            let Some(source_adapter) = self.languages.for_path(&source_path) else {
                return Err(RunReport::new(vec![Diagnostic::new(
                    "CTC1008",
                    DiagnosticCategory::UnsupportedSourceType,
                    format!(
                        "Source `{}` does not use a supported source type.",
                        display_path(root, &source_path).display()
                    ),
                    2,
                )]));
            };
            if source_adapter.id() != template.language_id {
                return Err(RunReport::new(vec![Diagnostic::new(
                    "CTC1008",
                    DiagnosticCategory::UnsupportedSourceType,
                    "The template and source use different language adapters.",
                    2,
                )]));
            }
            applications.push(RuleApplication {
                id: "command-line".to_string(),
                language_id: template.language_id,
                source_display_path: display_path(root, &source_path),
                source_path,
                template_path: template.path.clone(),
                template_display_path: template.display.clone(),
                template_source: template.source.clone(),
                alternatives: Vec::new(),
                mode: template.mode,
                scope: options.scope,
                message: options.message.clone(),
                kinds: Vec::new(),
                inside: Vec::new(),
                stop_at_functions: false,
                count: None,
            });
        }
        Ok(applications)
    }

    pub fn validate_template(&self, path: &Path) -> RunReport {
        let path = path.to_path_buf();
        let display = path.clone();
        if let Err(diagnostic) =
            crate::discovery::validate_template_filename(&display, &self.languages)
        {
            return RunReport::new(vec![diagnostic]);
        }
        let source = match read_utf8(&path, &display, true) {
            Ok(source) => source,
            Err(diagnostic) => return RunReport::new(vec![diagnostic]),
        };
        let directives = match crate::template::parse_directives(&source, &display) {
            Ok(directives) => directives,
            Err(diagnostics) => return RunReport::new(diagnostics),
        };
        if directives.excluded {
            return RunReport::new(Vec::new());
        }
        let Some(adapter) = self.languages.for_path(&template_source_path(&path)) else {
            return RunReport::new(vec![
                Diagnostic::new(
                    "CTC1008",
                    DiagnosticCategory::UnsupportedSourceType,
                    "The template filename does not contain a supported source suffix.",
                    2,
                )
                .with_template(TextRange::from_offsets(&display, &source, 0, 0)),
            ]);
        };
        let ranges = match adapter.scan_placeholders(&source, &display) {
            Ok(ranges) => ranges,
            Err(diagnostics) => return RunReport::new(diagnostics),
        };
        let placeholders = match parse_placeholders(&source, &display, &ranges, adapter.as_ref()) {
            Ok(placeholders) => placeholders,
            Err(diagnostics) => return RunReport::new(diagnostics),
        };
        match adapter.compile_template(&source, &display, &placeholders, directives.mode) {
            Ok(_) => RunReport::new(Vec::new()),
            Err(diagnostics) => RunReport::new(diagnostics),
        }
    }
}

fn canonical_root(root: &Path) -> Result<PathBuf, RunReport> {
    std::fs::canonicalize(root).map_err(|error| {
        RunReport::new(vec![Diagnostic::new(
            "CTC0001",
            DiagnosticCategory::InvalidCommandLine,
            format!("Cannot resolve the scan root: {error}"),
            2,
        )])
    })
}

pub use ParsedSource as Source;
