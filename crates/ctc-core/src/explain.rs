use std::{fs, path::PathBuf};

use serde::Serialize;

use crate::{
    diagnostic::{Diagnostic, DiagnosticCategory, RunReport, TextRange},
    discovery::validate_template_filename,
    engine::{
        CompileTemplateOptions, DiscoverRulesOptions, Engine, ParseSourceOptions, ParsedSource,
        canonicalize_input, display_path, read_utf8, with_rule_message,
    },
    matcher::{
        CandidateTrace, MatchCount, MatchMode, SearchOptions, SearchScope, TraceNode, TraceStep,
        TraceStepKind, explain_template_with_options,
    },
    template::CompiledTemplate,
};

const SNIPPET_LIMIT: usize = 60;

#[derive(Clone, Debug)]
pub struct ExplainOptions {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub source_path: PathBuf,
    pub target: ExplainTarget,
}

#[derive(Clone, Debug)]
pub enum ExplainTarget {
    Rule(String),
    Template {
        path: PathBuf,
        mode_override: Option<MatchMode>,
        scope: SearchScope,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainReport {
    pub schema_version: u32,
    pub rule_id: String,
    pub mode: MatchMode,
    pub scope: SearchScope,
    pub template: String,
    pub source: String,
    pub matches: bool,
    pub candidates: Vec<ExplainCandidate>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ExplainReport {
    pub fn exit_code(&self) -> u8 {
        self.diagnostics
            .iter()
            .map(|diagnostic| diagnostic.exit_class)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainCandidate {
    pub source: TextRange,
    pub text: String,
    pub template_matched: bool,
    pub steps: Vec<ExplainStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ExplainFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExplainStepKind {
    Literal,
    Capture,
    Sequence,
    Derived,
    OptionalAbsent,
}

impl From<TraceStepKind> for ExplainStepKind {
    fn from(value: TraceStepKind) -> Self {
        match value {
            TraceStepKind::Literal => Self::Literal,
            TraceStepKind::Capture => Self::Capture,
            TraceStepKind::Sequence => Self::Sequence,
            TraceStepKind::Derived => Self::Derived,
            TraceStepKind::OptionalAbsent => Self::OptionalAbsent,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainStep {
    pub depth: usize,
    pub kind: ExplainStepKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_kind: Option<String>,
    pub template: TextRange,
    pub nodes: Vec<ExplainNode>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainNode {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub range: TextRange,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainFailure {
    #[serde(flatten)]
    pub diagnostic: Diagnostic,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

struct PreparedExplain {
    rule_id: String,
    template: CompiledTemplate,
    source: ParsedSource,
    scope: SearchScope,
    kinds: Vec<String>,
    inside: Vec<String>,
    stop_at_functions: bool,
    count: Option<MatchCount>,
    message: Option<String>,
}

impl Engine {
    /// Explains how one rule or ad hoc template matches one source file.
    pub fn explain(&self, options: &ExplainOptions) -> Result<ExplainReport, RunReport> {
        let prepared = match &options.target {
            ExplainTarget::Rule(rule_id) => self.prepare_rule_explain(options, rule_id)?,
            ExplainTarget::Template {
                path,
                mode_override,
                scope,
            } => self.prepare_template_explain(options, path, *mode_override, *scope)?,
        };
        if prepared.template.language_id != prepared.source.language_id {
            return Err(error(
                "CTC1008",
                DiagnosticCategory::UnsupportedSourceType,
                "The template and source use different language adapters.",
            ));
        }
        let Some(adapter) = self.languages.by_id(prepared.source.language_id) else {
            return Err(error(
                "CTC9001",
                DiagnosticCategory::InternalToolError,
                "The parsed source language adapter is not registered.",
            ));
        };
        let explanation = explain_template_with_options(
            &prepared.rule_id,
            &prepared.template,
            &prepared.source.root,
            prepared.scope,
            &prepared.kinds,
            &SearchOptions {
                inside: &prepared.inside,
                stop_at_functions: prepared.stop_at_functions,
                count: prepared.count,
            },
            &|value| adapter.validate_identifier(value),
        );
        let text = &prepared.source.source;
        let run = RunReport::new(with_rule_message(
            explanation.result.diagnostics,
            prepared.message.as_deref(),
        ));
        Ok(ExplainReport {
            schema_version: 1,
            rule_id: prepared.rule_id,
            mode: prepared.template.mode,
            scope: prepared.scope,
            template: prepared.template.path.clone(),
            source: prepared.source.path.to_string_lossy().replace('\\', "/"),
            matches: run.matches,
            candidates: explanation
                .candidates
                .into_iter()
                .map(|candidate| explain_candidate(candidate, text))
                .collect(),
            diagnostics: run.diagnostics,
        })
    }

    fn prepare_rule_explain(
        &self,
        options: &ExplainOptions,
        rule_id: &str,
    ) -> Result<PreparedExplain, RunReport> {
        let discovered = self
            .discover_all_rules(&DiscoverRulesOptions {
                root: options.root.clone(),
                config_path: options.config_path.clone(),
                paths: vec![options.source_path.clone()],
                rule_ids: vec![rule_id.to_string()],
            })
            .map_err(RunReport::new)?;
        let mut applications = discovered.template_rules;
        if applications.is_empty() {
            return Err(command_error(&format!(
                "Rule `{rule_id}` is not a template rule. `explain` supports template rules only."
            )));
        }
        if applications.len() > 1 {
            return Err(command_error("`explain` needs exactly one source file."));
        }
        let application = applications.remove(0);
        if !application.alternatives.is_empty() {
            return Err(command_error(&format!(
                "Rule `{rule_id}` lists several templates. `explain` supports rules with one template only."
            )));
        }
        let template = self
            .compile_template(&CompileTemplateOptions {
                template_text: application.template_source,
                template_path: application.template_display_path,
                mode_override: Some(application.mode),
            })
            .map_err(RunReport::new)?;
        let source_text = read_utf8(
            &application.source_path,
            &application.source_display_path,
            false,
        )
        .map_err(|diagnostic| RunReport::new(vec![diagnostic]))?;
        let source = self
            .parse_source(&ParseSourceOptions {
                source_text,
                source_path: application.source_display_path,
            })
            .map_err(RunReport::new)?;
        Ok(PreparedExplain {
            rule_id: application.id,
            template,
            source,
            scope: application.scope,
            kinds: application.kinds,
            inside: application.inside,
            stop_at_functions: application.stop_at_functions,
            count: application.count,
            message: application.message,
        })
    }

    fn prepare_template_explain(
        &self,
        options: &ExplainOptions,
        template_path: &std::path::Path,
        mode_override: Option<MatchMode>,
        scope: SearchScope,
    ) -> Result<PreparedExplain, RunReport> {
        let one = |diagnostic: Diagnostic| RunReport::new(vec![diagnostic]);
        let root = fs::canonicalize(&options.root).map_err(|failure| {
            command_error(&format!("Cannot resolve the scan root: {failure}"))
        })?;
        let template_path = canonicalize_input(&root, template_path).map_err(one)?;
        let template_display = display_path(&root, &template_path);
        validate_template_filename(&template_display, &self.languages).map_err(one)?;
        let template_text = read_utf8(&template_path, &template_display, true).map_err(one)?;
        let template = self
            .compile_template(&CompileTemplateOptions {
                template_text,
                template_path: template_display,
                mode_override,
            })
            .map_err(RunReport::new)?;

        let source_path = canonicalize_input(&root, &options.source_path).map_err(one)?;
        if !source_path.starts_with(&root) {
            return Err(command_error(&format!(
                "Explicit path `{}` is outside the scan root.",
                options.source_path.display()
            )));
        }
        if !source_path.is_file() {
            return Err(command_error("`explain` needs exactly one source file."));
        }
        let source_display = display_path(&root, &source_path);
        let source_text = read_utf8(&source_path, &source_display, false).map_err(one)?;
        let source = self
            .parse_source(&ParseSourceOptions {
                source_text,
                source_path: source_display,
            })
            .map_err(RunReport::new)?;
        Ok(PreparedExplain {
            rule_id: "command-line".to_string(),
            template,
            source,
            scope,
            kinds: Vec::new(),
            inside: Vec::new(),
            stop_at_functions: false,
            count: None,
            message: None,
        })
    }
}

fn error(code: &str, category: DiagnosticCategory, message: &str) -> RunReport {
    RunReport::new(vec![Diagnostic::new(code, category, message, 2)])
}

fn command_error(message: &str) -> RunReport {
    error("CTC0001", DiagnosticCategory::InvalidCommandLine, message)
}

fn explain_candidate(candidate: CandidateTrace, text: &str) -> ExplainCandidate {
    ExplainCandidate {
        text: snippet(text, &candidate.source),
        source: candidate.source,
        template_matched: candidate.template_matched,
        steps: candidate
            .steps
            .into_iter()
            .map(|step| explain_step(step, text))
            .collect(),
        failure: candidate.failure.map(|failure| ExplainFailure {
            diagnostic: failure.diagnostic,
            reason: failure.reason,
        }),
    }
}

fn explain_step(step: TraceStep, text: &str) -> ExplainStep {
    ExplainStep {
        depth: step.depth,
        kind: step.kind.into(),
        name: step.name,
        template_kind: step.template_kind,
        template: step.template,
        nodes: step
            .nodes
            .into_iter()
            .map(|node| explain_node(node, text))
            .collect(),
    }
}

fn explain_node(node: TraceNode, text: &str) -> ExplainNode {
    ExplainNode {
        text: snippet(text, &node.range),
        kind: node.kind,
        value: node.value,
        range: node.range,
    }
}

/// Returns the first line of the source text in `range`, cut to a short length.
fn snippet(text: &str, range: &TextRange) -> String {
    let start = range.start.offset as usize;
    let end = (range.end.offset as usize).max(start);
    let Some(slice) = text.get(start..end.min(text.len())) else {
        return String::new();
    };
    let line = slice.lines().next().unwrap_or_default().trim();
    if line.chars().count() > SNIPPET_LIMIT {
        let cut = line.chars().take(SNIPPET_LIMIT - 3).collect::<String>();
        format!("{}...", cut.trim_end())
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests;
