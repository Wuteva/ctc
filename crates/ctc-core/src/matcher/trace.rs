use std::sync::Arc;

use crate::{
    canonical::{CanonicalNode, CanonicalScalar},
    diagnostic::{Diagnostic, TextRange},
    template::{Placeholder, TemplateNode},
};

use super::{capture::MatchSuccess, failure::MatchFailure};

/// The result of a traced match. `result` is the same as the result of an
/// untraced match for the same inputs.
#[derive(Clone, Debug)]
pub struct MatchExplanation {
    pub result: super::MatchResult,
    pub candidates: Vec<CandidateTrace>,
}

#[derive(Clone, Debug)]
pub struct CandidateTrace {
    pub source: TextRange,
    pub template_matched: bool,
    pub steps: Vec<TraceStep>,
    pub failure: Option<TraceFailure>,
}

#[derive(Clone, Debug)]
pub struct TraceFailure {
    pub diagnostic: Diagnostic,
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceStepKind {
    Literal,
    Capture,
    Sequence,
    Derived,
    OptionalAbsent,
}

#[derive(Clone, Debug)]
pub struct TraceStep {
    pub depth: usize,
    pub kind: TraceStepKind,
    pub name: Option<String>,
    pub template_kind: Option<String>,
    pub template: TextRange,
    pub nodes: Vec<TraceNode>,
}

#[derive(Clone, Debug)]
pub struct TraceNode {
    pub kind: String,
    pub value: Option<String>,
    pub range: TextRange,
}

impl TraceNode {
    pub(super) fn from_node(node: &CanonicalNode) -> Self {
        Self {
            kind: node.kind.to_string(),
            value: node.value.as_ref().map(CanonicalScalar::display),
            range: node.range.clone(),
        }
    }
}

/// One link of a persistent list of trace steps. The newest step is first.
#[derive(Debug)]
pub(super) struct TraceLink {
    pub(super) step: TraceStep,
    pub(super) parent: Option<Arc<TraceLink>>,
}

impl Drop for TraceLink {
    // Drops long chains in a loop instead of by recursion.
    fn drop(&mut self) {
        let mut next = self.parent.take();
        while let Some(link) = next {
            match Arc::try_unwrap(link) {
                Ok(mut link) => next = link.parent.take(),
                Err(_) => break,
            }
        }
    }
}

pub(super) fn collect_steps(mut link: Option<&Arc<TraceLink>>) -> Vec<TraceStep> {
    let mut steps = Vec::new();
    while let Some(current) = link {
        steps.push(current.step.clone());
        link = current.parent.as_ref();
    }
    steps.reverse();
    steps
}

#[derive(Clone, Debug)]
pub(super) struct FailureTrace {
    pub(super) steps: Option<Arc<TraceLink>>,
    pub(super) reason: Option<String>,
}

pub(super) fn candidate_trace(
    source: &TextRange,
    result: &Result<MatchSuccess, MatchFailure>,
    rule_id: &str,
) -> CandidateTrace {
    match result {
        Ok(success) => CandidateTrace {
            source: source.clone(),
            template_matched: true,
            steps: collect_steps(success.environment.trace.as_ref()),
            failure: None,
        },
        Err(failure) => CandidateTrace {
            source: source.clone(),
            template_matched: false,
            steps: failure.trace_steps(),
            failure: Some(failure.traced(rule_id)),
        },
    }
}

pub(super) fn template_name(node: &TemplateNode) -> Option<String> {
    match node {
        TemplateNode::Capture { placeholder }
        | TemplateNode::Sequence { placeholder }
        | TemplateNode::Derived { placeholder } => Some(placeholder.name.clone()),
        TemplateNode::Optional { node, .. } => template_name(node),
        TemplateNode::Literal { .. } => None,
    }
}

pub(super) fn template_kind(node: &TemplateNode) -> Option<String> {
    match node {
        TemplateNode::Literal { kind, .. } => Some(kind.to_string()),
        TemplateNode::Optional { node, .. } => template_kind(node),
        TemplateNode::Capture { .. }
        | TemplateNode::Sequence { .. }
        | TemplateNode::Derived { .. } => None,
    }
}

pub(super) fn sequence_step(
    placeholder: &Placeholder,
    nodes: &[CanonicalNode],
    depth: usize,
) -> TraceStep {
    TraceStep {
        depth,
        kind: TraceStepKind::Sequence,
        name: Some(placeholder.name.clone()),
        template_kind: None,
        template: placeholder.range.clone(),
        nodes: nodes.iter().map(TraceNode::from_node).collect(),
    }
}

pub(super) fn placeholder_step(
    kind: TraceStepKind,
    placeholder: &Placeholder,
    source: &CanonicalNode,
    depth: usize,
) -> TraceStep {
    TraceStep {
        depth,
        kind,
        name: Some(placeholder.name.clone()),
        template_kind: None,
        template: placeholder.range.clone(),
        nodes: vec![TraceNode::from_node(source)],
    }
}
