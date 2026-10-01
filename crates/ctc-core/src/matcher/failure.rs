use crate::{
    canonical::{CanonicalNode, CanonicalScalar},
    diagnostic::{Diagnostic, DiagnosticCategory, TextRange},
    template::{Placeholder, TemplateNode},
};

use super::trace::{FailureTrace, TraceFailure, TraceLink, collect_steps};

#[derive(Clone, Debug)]
pub(super) struct MatchFailure {
    pub(super) code: &'static str,
    pub(super) category: DiagnosticCategory,
    pub(super) message: String,
    pub(super) template: TextRange,
    pub(super) source: Option<TextRange>,
    pub(super) expected: Option<String>,
    pub(super) actual: Option<String>,
    pub(super) matched_nodes: usize,
    pub(super) source_offset: u64,
    pub(super) exit_class: u8,
    pub(super) candidate_start: usize,
    pub(super) trace: Option<Box<FailureTrace>>,
}

impl MatchFailure {
    pub(super) fn with_trace(mut self, steps: Option<std::sync::Arc<TraceLink>>) -> Self {
        if self.trace.is_none() {
            self.trace = Some(Box::new(FailureTrace {
                steps,
                reason: None,
            }));
        }
        self
    }

    pub(super) fn traced(&self, rule_id: &str) -> TraceFailure {
        TraceFailure {
            diagnostic: self.clone().diagnostic(rule_id),
            reason: self.trace.as_ref().and_then(|trace| trace.reason.clone()),
        }
    }

    pub(super) fn trace_steps(&self) -> Vec<super::TraceStep> {
        collect_steps(self.trace.as_ref().and_then(|trace| trace.steps.as_ref()))
    }

    pub(super) fn diagnostic(self, rule_id: &str) -> Diagnostic {
        let mut diagnostic =
            Diagnostic::new(self.code, self.category, self.message, self.exit_class)
                .with_rule(rule_id)
                .with_template(self.template);
        if let Some(source) = self.source {
            diagnostic = diagnostic.with_source(source);
        }
        if let (Some(expected), Some(actual)) = (self.expected, self.actual) {
            diagnostic = diagnostic.with_values(expected, actual);
        }
        diagnostic
    }
}

pub(super) fn literal_failure(
    template: &TemplateNode,
    source: &CanonicalNode,
    matched_nodes: usize,
    candidate_start: usize,
) -> MatchFailure {
    let expected = match template {
        TemplateNode::Literal { kind, value, .. } => format_node(kind, value.as_ref()),
        _ => "a matching syntax node".to_string(),
    };
    MatchFailure {
        code: "CTC3002",
        category: DiagnosticCategory::MissingSyntaxNode,
        message: "The source node does not match the required template node.".to_string(),
        template: template.range().clone(),
        source: Some(source.range.clone()),
        expected: Some(expected),
        actual: Some(format_node(&source.kind, source.value.as_ref())),
        matched_nodes,
        source_offset: source.range.start.offset,
        exit_class: 1,
        candidate_start,
        trace: None,
    }
}

pub(super) fn missing_failure(
    template: Option<&TemplateNode>,
    source: Option<&CanonicalNode>,
    template_fallback: &TextRange,
    source_fallback: &TextRange,
    matched_nodes: usize,
    candidate_start: usize,
) -> MatchFailure {
    MatchFailure {
        code: "CTC3002",
        category: DiagnosticCategory::MissingSyntaxNode,
        message: "The source is missing a required syntax node.".to_string(),
        template: template.map_or_else(|| template_fallback.clone(), |node| node.range().clone()),
        source: Some(source.map_or_else(|| source_fallback.clone(), |node| node.range.clone())),
        expected: template.map(describe_template_node),
        actual: Some(
            source
                .map(|node| format_node(&node.kind, node.value.as_ref()))
                .unwrap_or_else(|| "end of syntax list".to_string()),
        ),
        matched_nodes,
        source_offset: source.map_or(source_fallback.end.offset, |node| node.range.start.offset),
        exit_class: 1,
        candidate_start,
        trace: None,
    }
}

pub(super) fn unexpected_failure(
    source: &CanonicalNode,
    template_fallback: &TextRange,
    matched_nodes: usize,
    candidate_start: usize,
) -> MatchFailure {
    MatchFailure {
        code: "CTC3003",
        category: DiagnosticCategory::UnexpectedSyntaxNode,
        message: "The source contains an unexpected syntax node.".to_string(),
        template: template_fallback.clone(),
        source: Some(source.range.clone()),
        expected: Some("end of syntax list".to_string()),
        actual: Some(format_node(&source.kind, source.value.as_ref())),
        matched_nodes,
        source_offset: source.range.start.offset,
        exit_class: 1,
        candidate_start,
        trace: None,
    }
}

pub(super) fn capture_failure(
    placeholder: &Placeholder,
    source: &CanonicalNode,
    matched_nodes: usize,
    candidate_start: usize,
) -> MatchFailure {
    MatchFailure {
        code: "CTC3004",
        category: DiagnosticCategory::CapturedValueMismatch,
        message: format!(
            "Capture `{}` does not match its required value.",
            placeholder.name
        ),
        template: placeholder.range.clone(),
        source: Some(source.range.clone()),
        expected: Some(format!("capture `{}`", placeholder.name)),
        actual: Some(format_node(&source.kind, source.value.as_ref())),
        matched_nodes,
        source_offset: source.range.start.offset,
        exit_class: 1,
        candidate_start,
        trace: None,
    }
}

pub(super) fn category_failure(
    placeholder: &Placeholder,
    source: Option<&CanonicalNode>,
    matched_nodes: usize,
    candidate_start: usize,
) -> MatchFailure {
    MatchFailure {
        code: "CTC2006",
        category: DiagnosticCategory::PlaceholderCategoryMismatch,
        message: format!(
            "Capture `{}` does not match the required placeholder category.",
            placeholder.name
        ),
        template: placeholder.range.clone(),
        source: source.map(|node| node.range.clone()),
        expected: Some(format!("{:?}", placeholder.category)),
        actual: source.map(|node| format_node(&node.kind, node.value.as_ref())),
        matched_nodes,
        source_offset: source.map_or(0, |node| node.range.start.offset),
        exit_class: 1,
        candidate_start,
        trace: None,
    }
}

pub(super) fn derived_failure(
    placeholder: &Placeholder,
    source: &CanonicalNode,
    expected: String,
    actual: String,
    candidate_start: usize,
) -> MatchFailure {
    MatchFailure {
        code: "CTC3005",
        category: DiagnosticCategory::DerivedIdentifierMismatch,
        message: format!(
            "Derived identifier for capture `{}` does not match.",
            placeholder.name
        ),
        template: placeholder.range.clone(),
        source: Some(source.range.clone()),
        expected: Some(expected),
        actual: Some(actual),
        matched_nodes: 0,
        source_offset: source.range.start.offset,
        exit_class: 1,
        candidate_start,
        trace: None,
    }
}

fn describe_template_node(node: &TemplateNode) -> String {
    match node {
        TemplateNode::Literal { kind, value, .. } => format_node(kind, value.as_ref()),
        TemplateNode::Capture { placeholder }
        | TemplateNode::Sequence { placeholder }
        | TemplateNode::Derived { placeholder } => format!("capture `{}`", placeholder.name),
        TemplateNode::Optional { node, .. } => {
            format!("optional {}", describe_template_node(node))
        }
    }
}

fn format_node(kind: &str, value: Option<&CanonicalScalar>) -> String {
    value.map_or_else(
        || kind.to_string(),
        |value| format!("{kind} `{}`", value.display()),
    )
}

pub(super) fn keep_better_failure(best: &mut Option<MatchFailure>, candidate: MatchFailure) {
    let replace = best.as_ref().is_none_or(|current| {
        (
            candidate.matched_nodes,
            candidate.source_offset,
            std::cmp::Reverse(candidate.candidate_start),
        ) > (
            current.matched_nodes,
            current.source_offset,
            std::cmp::Reverse(current.candidate_start),
        )
    });
    if replace {
        *best = Some(candidate);
    }
}
