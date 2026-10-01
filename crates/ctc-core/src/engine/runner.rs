use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    diagnostic::{Diagnostic, DiagnosticCategory, RunReport, TextRange},
    discovery::{RuleApplication, SemanticRuleApplication},
    language::{LanguageAdapter, LanguageRegistry},
    matcher::{MatchMode, MatchResult, SearchOptions},
    semantic::{
        MissingPartner, SemanticRule, evaluate_header_source_pairing, evaluate_semantic_rule,
        file_too_long, header_needs_definitions, missing_companion_file, missing_source_file,
    },
    suppression::collect_suppressions,
    template::{CompiledTemplate, parse_placeholders},
};

use super::{Engine, MatchTemplateOptions, ParsedSource};

pub(super) struct Suppression<'a> {
    pub(super) configured_rule_ids: &'a BTreeSet<String>,
    pub(super) ignorable_rule_ids: &'a BTreeSet<String>,
}

struct RunState {
    diagnostics: Vec<Diagnostic>,
    template_cache: BTreeMap<PathBuf, Result<CompiledTemplate, Vec<Diagnostic>>>,
    source_cache: BTreeMap<PathBuf, Result<ParsedSource, Vec<Diagnostic>>>,
}

impl RunState {
    fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
            template_cache: BTreeMap::new(),
            source_cache: BTreeMap::new(),
        }
    }
}

impl Engine {
    /// Runs the rule applications. Suppression comments apply only when
    /// `suppression` is present, because ad hoc runs have no configured rules
    /// to name.
    pub(super) fn run_applications(
        &self,
        applications: Vec<RuleApplication>,
        semantic_applications: Vec<SemanticRuleApplication>,
        suppression: Option<Suppression<'_>>,
    ) -> RunReport {
        let mut state = RunState::new();
        for application in applications {
            self.run_template_application(application, &mut state);
        }
        for application in semantic_applications {
            self.run_semantic_application(application, &mut state);
        }
        if let Some(suppression) = suppression {
            apply_suppressions(&mut state.diagnostics, &state.source_cache, suppression);
        }
        RunReport::new(state.diagnostics)
    }

    fn run_template_application(&self, application: RuleApplication, state: &mut RunState) {
        let Some((adapter, source_adapter)) = self.template_adapters(&application, state) else {
            return;
        };
        let template_inputs = template_inputs(&application);
        if !compile_templates(
            &mut state.template_cache,
            &mut state.diagnostics,
            &application,
            adapter.as_ref(),
            &template_inputs,
        ) {
            return;
        }
        let source = match cached_source(&mut state.source_cache, &application.source_path, || {
            let source = read_utf8(
                &application.source_path,
                &application.source_display_path,
                false,
            )
            .map_err(|diagnostic| vec![diagnostic])?;
            source_adapter.parse(&source, &application.source_display_path)
        }) {
            Ok(source) => source,
            Err(errors) => {
                extend_unique_diagnostics(&mut state.diagnostics, errors);
                return;
            }
        };
        let results = template_inputs
            .iter()
            .filter_map(|(path, _, _)| match state.template_cache.get(*path) {
                Some(Ok(template)) => Some(template),
                _ => None,
            })
            .map(|template| {
                self.match_template(&MatchTemplateOptions {
                    rule_id: &application.id,
                    template,
                    source,
                    scope: application.scope,
                    kinds: &application.kinds,
                    search: SearchOptions {
                        inside: &application.inside,
                        stop_at_functions: application.stop_at_functions,
                        count: application.count,
                    },
                })
            })
            .collect::<Vec<_>>();
        let result = combine_alternatives(application.mode, results);
        state.diagnostics.extend(with_rule_message(
            result.diagnostics,
            application.message.as_deref(),
        ));
    }

    fn template_adapters(
        &self,
        application: &RuleApplication,
        state: &mut RunState,
    ) -> Option<(Arc<dyn LanguageAdapter>, Arc<dyn LanguageAdapter>)> {
        let Some(adapter) = self.languages.by_id(application.language_id) else {
            state.diagnostics.push(unregistered_language_diagnostic(
                application.language_id,
                &application.source_display_path,
            ));
            return None;
        };
        let Some(source_adapter) = self.languages.for_path(&application.source_path) else {
            state.diagnostics.push(unsupported_source_diagnostic(
                &application.source_display_path,
            ));
            return None;
        };
        if source_adapter.id() != application.language_id {
            state.diagnostics.push(different_language_diagnostic(
                &application.source_display_path,
            ));
            return None;
        }
        Some((adapter, source_adapter))
    }

    fn run_semantic_application(&self, application: SemanticRuleApplication, state: &mut RunState) {
        if handle_companion_file(&application, &mut state.diagnostics) {
            return;
        }
        if handle_file_length(&application, &mut state.diagnostics) {
            return;
        }
        let Some(adapter) = self.semantic_adapter(&application, state) else {
            return;
        };
        let source = match cached_source(&mut state.source_cache, &application.source_path, || {
            let source = read_utf8(
                &application.source_path,
                &application.source_display_path,
                false,
            )
            .map_err(|diagnostic| vec![diagnostic])?;
            adapter.parse(&source, &application.source_display_path)
        }) {
            Ok(source) => source.clone(),
            Err(errors) => {
                extend_unique_diagnostics(&mut state.diagnostics, errors);
                return;
            }
        };
        if self.handle_header_source_pairing(&application, state, adapter.as_ref(), &source) {
            return;
        }
        state.diagnostics.extend(with_rule_message(
            evaluate_semantic_rule(&application.id, &application.rule, &source.semantic_facts),
            application.message.as_deref(),
        ));
    }

    fn semantic_adapter(
        &self,
        application: &SemanticRuleApplication,
        state: &mut RunState,
    ) -> Option<Arc<dyn LanguageAdapter>> {
        let Some(adapter) = self.languages.by_id(application.language_id) else {
            state.diagnostics.push(unregistered_language_diagnostic(
                application.language_id,
                &application.source_display_path,
            ));
            return None;
        };
        Some(adapter)
    }

    fn handle_header_source_pairing(
        &self,
        application: &SemanticRuleApplication,
        state: &mut RunState,
        adapter: &dyn LanguageAdapter,
        source: &ParsedSource,
    ) -> bool {
        let SemanticRule::HeaderSourcePairing {
            missing_source,
            check_order,
        } = application.rule
        else {
            return false;
        };
        let Some(partner) = &application.partner else {
            if missing_source == MissingPartner::Report
                && header_needs_definitions(&source.semantic_facts)
            {
                state.diagnostics.extend(with_rule_message(
                    vec![missing_source_file(
                        &application.id,
                        &application.source_display_path,
                        &application.partner_candidates,
                    )],
                    application.message.as_deref(),
                ));
            }
            return true;
        };
        let header_facts = source.semantic_facts.clone();
        let Some(partner_adapter) = self
            .languages
            .for_path(&partner.path)
            .filter(|partner_adapter| partner_adapter.id() == adapter.id())
        else {
            state
                .diagnostics
                .push(partner_language_diagnostic(application, partner));
            return true;
        };
        let partner_source = match cached_source(&mut state.source_cache, &partner.path, || {
            let source = read_utf8(&partner.path, &partner.display_path, false)
                .map_err(|diagnostic| vec![diagnostic])?;
            partner_adapter.parse(&source, &partner.display_path)
        }) {
            Ok(partner_source) => partner_source,
            Err(errors) => {
                extend_unique_diagnostics(&mut state.diagnostics, errors);
                return true;
            }
        };
        state.diagnostics.extend(with_rule_message(
            evaluate_header_source_pairing(
                &application.id,
                &header_facts,
                &partner.display_path,
                &partner_source.semantic_facts,
                check_order,
            ),
            application.message.as_deref(),
        ));
        true
    }
}

fn handle_companion_file(
    application: &SemanticRuleApplication,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if matches!(application.rule, SemanticRule::CompanionFile) {
        if application.partner.is_none() {
            diagnostics.extend(with_rule_message(
                vec![missing_companion_file(
                    &application.id,
                    &application.source_display_path,
                    &application.partner_candidates,
                )],
                application.message.as_deref(),
            ));
        }
        true
    } else {
        false
    }
}

fn handle_file_length(
    application: &SemanticRuleApplication,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let SemanticRule::FileLength { max_lines } = application.rule else {
        return false;
    };
    match read_utf8(
        &application.source_path,
        &application.source_display_path,
        false,
    ) {
        Ok(text) => diagnostics.extend(with_rule_message(
            file_too_long(
                &application.id,
                &application.source_display_path,
                &text,
                max_lines,
            )
            .into_iter()
            .collect(),
            application.message.as_deref(),
        )),
        Err(diagnostic) => diagnostics.push(diagnostic),
    }
    true
}

fn template_inputs(application: &RuleApplication) -> Vec<(&PathBuf, &PathBuf, &String)> {
    std::iter::once((
        &application.template_path,
        &application.template_display_path,
        &application.template_source,
    ))
    .chain(
        application
            .alternatives
            .iter()
            .map(|template| (&template.path, &template.display_path, &template.source)),
    )
    .collect()
}

fn compile_templates(
    template_cache: &mut BTreeMap<PathBuf, Result<CompiledTemplate, Vec<Diagnostic>>>,
    diagnostics: &mut Vec<Diagnostic>,
    application: &RuleApplication,
    adapter: &dyn LanguageAdapter,
    template_inputs: &[(&PathBuf, &PathBuf, &String)],
) -> bool {
    let mut templates_compiled = true;
    for (path, display_path, template_source) in template_inputs {
        let template_result = template_cache.entry((*path).clone()).or_insert_with(|| {
            let ranges = adapter.scan_placeholders(template_source, display_path)?;
            let placeholders = parse_placeholders(template_source, display_path, &ranges, adapter)?;
            adapter.compile_template(
                template_source,
                display_path,
                &placeholders,
                application.mode,
            )
        });
        if let Err(errors) = template_result {
            templates_compiled = false;
            extend_unique_diagnostics(diagnostics, errors);
        }
    }
    templates_compiled
}

fn cached_source<'a, F>(
    source_cache: &'a mut BTreeMap<PathBuf, Result<ParsedSource, Vec<Diagnostic>>>,
    path: &Path,
    load: F,
) -> &'a Result<ParsedSource, Vec<Diagnostic>>
where
    F: FnOnce() -> Result<ParsedSource, Vec<Diagnostic>>,
{
    source_cache.entry(path.to_path_buf()).or_insert_with(load)
}

fn extend_unique_diagnostics(diagnostics: &mut Vec<Diagnostic>, errors: &[Diagnostic]) {
    if !diagnostics
        .iter()
        .any(|diagnostic| diagnostics_equal_set(diagnostic, errors))
    {
        diagnostics.extend(errors.iter().cloned());
    }
}

/// Combines the results of the templates of one rule for one source file.
///
/// - `exact` and `contains` pass when any template passes. Otherwise the
///   result of the template that failed the furthest into the source is kept.
/// - `forbid` reports the matches of every template.
/// - `every` fails a candidate only when every template fails it.
#[cfg(test)]
pub(super) fn combine_alternatives(mode: MatchMode, mut results: Vec<MatchResult>) -> MatchResult {
    combine_alternatives_impl(mode, &mut results)
}

#[cfg(not(test))]
fn combine_alternatives(mode: MatchMode, mut results: Vec<MatchResult>) -> MatchResult {
    combine_alternatives_impl(mode, &mut results)
}

fn combine_alternatives_impl(mode: MatchMode, results: &mut Vec<MatchResult>) -> MatchResult {
    if results.len() == 1 {
        return results.remove(0);
    }
    if let Some(index) = results.iter().position(|result| {
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.exit_class == 2)
    }) {
        return results.swap_remove(index);
    }
    match mode {
        MatchMode::Exact | MatchMode::Contains => {
            if results.iter().any(|result| result.matches) {
                return MatchResult {
                    matches: true,
                    diagnostics: Vec::new(),
                };
            }
            let reach = |result: &MatchResult| {
                result
                    .diagnostics
                    .iter()
                    .filter_map(|diagnostic| diagnostic.source.as_ref())
                    .map(|range| range.start.offset)
                    .max()
                    .unwrap_or(0)
            };
            let mut best = 0;
            for (index, result) in results.iter().enumerate() {
                if reach(result) > reach(&results[best]) {
                    best = index;
                }
            }
            results.swap_remove(best)
        }
        MatchMode::Forbid => {
            let diagnostics = results
                .drain(..)
                .flat_map(|result| result.diagnostics)
                .collect::<Vec<_>>();
            MatchResult {
                matches: diagnostics.is_empty(),
                diagnostics,
            }
        }
        MatchMode::Every => {
            let offset = |diagnostic: &Diagnostic| {
                diagnostic.source.as_ref().map(|range| range.start.offset)
            };
            let failed_everywhere = results[0]
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    results[1..].iter().all(|other| {
                        other
                            .diagnostics
                            .iter()
                            .any(|candidate| offset(candidate) == offset(diagnostic))
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            MatchResult {
                matches: failed_everywhere.is_empty(),
                diagnostics: failed_everywhere,
            }
        }
    }
}

pub(crate) fn with_rule_message(
    diagnostics: Vec<Diagnostic>,
    message: Option<&str>,
) -> Vec<Diagnostic> {
    let Some(message) = message else {
        return diagnostics;
    };
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            if diagnostic.is_rule_failure() {
                diagnostic.with_rule_message(message)
            } else {
                diagnostic
            }
        })
        .collect()
}

fn apply_suppressions(
    diagnostics: &mut Vec<Diagnostic>,
    sources: &BTreeMap<PathBuf, Result<ParsedSource, Vec<Diagnostic>>>,
    suppression: Suppression<'_>,
) {
    for source in sources.values().filter_map(|source| source.as_ref().ok()) {
        let suppressions = collect_suppressions(
            &source.path,
            &source.source,
            &source.comments,
            suppression.configured_rule_ids,
            suppression.ignorable_rule_ids,
        );
        let path = TextRange::from_offsets(&source.path, "", 0, 0).path;
        diagnostics.retain(|diagnostic| {
            !(diagnostic
                .source
                .as_ref()
                .is_some_and(|range| range.path == path)
                && suppressions.suppresses(diagnostic))
        });
        diagnostics.extend(suppressions.diagnostics);
    }
}

fn diagnostics_equal_set(diagnostic: &Diagnostic, set: &[Diagnostic]) -> bool {
    set.iter().any(|candidate| {
        diagnostic.code == candidate.code
            && diagnostic.message == candidate.message
            && diagnostic.source == candidate.source
            && diagnostic.template == candidate.template
    })
}

pub(crate) fn read_utf8(path: &Path, display: &Path, template: bool) -> Result<String, Diagnostic> {
    let bytes = fs::read(path).map_err(|error| {
        Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            format!("Cannot read `{}`: {error}", display.display()),
            2,
        )
    })?;
    let source = String::from_utf8(bytes).map_err(|_| {
        let diagnostic = Diagnostic::new(
            "CTC1009",
            DiagnosticCategory::InvalidUtf8,
            format!("`{}` is not valid UTF-8.", display.display()),
            2,
        );
        if template {
            diagnostic.with_template(TextRange::from_offsets(display, "", 0, 0))
        } else {
            diagnostic.with_source(TextRange::from_offsets(display, "", 0, 0))
        }
    })?;
    Ok(source)
}

pub(crate) fn canonicalize_input(root: &Path, path: &Path) -> Result<PathBuf, Diagnostic> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    fs::canonicalize(&path).map_err(|error| {
        Diagnostic::new(
            "CTC0001",
            DiagnosticCategory::InvalidCommandLine,
            format!("Cannot resolve input path `{}`: {error}", path.display()),
            2,
        )
    })
}

pub(crate) fn display_path(root: &Path, path: &Path) -> PathBuf {
    let value = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let value = if let Some(value) = value.strip_prefix("//?/UNC/") {
        format!("//{value}")
    } else {
        value.strip_prefix("//?/").unwrap_or(&value).to_string()
    };
    PathBuf::from(value)
}

pub(super) fn template_source_path(template_path: &Path) -> PathBuf {
    let value = template_path.to_string_lossy();
    value
        .strip_suffix(".ctmpl")
        .map(PathBuf::from)
        .unwrap_or_else(|| template_path.to_path_buf())
}

pub(super) fn collect_supported_files(
    root: &Path,
    languages: &LanguageRegistry,
    files: &mut Vec<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if matches!(
                entry.file_name().to_str(),
                Some(".git" | ".ctmpl" | "node_modules" | "target")
            ) {
                continue;
            }
            collect_supported_files(&entry.path(), languages, files);
        } else if file_type.is_file() && languages.for_path(&entry.path()).is_some() {
            files.push(entry.path());
        }
    }
}

fn unregistered_language_diagnostic(language_id: &str, display_path: &Path) -> Diagnostic {
    Diagnostic::new(
        "CTC1008",
        DiagnosticCategory::UnsupportedSourceType,
        format!("Language adapter `{language_id}` is not registered."),
        2,
    )
    .with_source(TextRange::from_offsets(display_path, "", 0, 0))
}

fn unsupported_source_diagnostic(display_path: &Path) -> Diagnostic {
    Diagnostic::new(
        "CTC1008",
        DiagnosticCategory::UnsupportedSourceType,
        format!(
            "Source `{}` does not use a supported source type.",
            display_path.display()
        ),
        2,
    )
    .with_source(TextRange::from_offsets(display_path, "", 0, 0))
}

fn different_language_diagnostic(display_path: &Path) -> Diagnostic {
    Diagnostic::new(
        "CTC1008",
        DiagnosticCategory::UnsupportedSourceType,
        "The template and source use different language adapters.",
        2,
    )
    .with_source(TextRange::from_offsets(display_path, "", 0, 0))
}

fn partner_language_diagnostic(
    application: &SemanticRuleApplication,
    partner: &crate::discovery::PartnerFile,
) -> Diagnostic {
    Diagnostic::new(
        "CTC1008",
        DiagnosticCategory::UnsupportedSourceType,
        format!(
            "Partner file `{}` does not use the same language as `{}`.",
            partner.display_path.display(),
            application.source_display_path.display()
        ),
        2,
    )
    .with_rule(&application.id)
    .with_source(TextRange::from_offsets(
        &application.source_display_path,
        "",
        0,
        0,
    ))
}
