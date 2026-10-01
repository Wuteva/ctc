use crate::{
    canonical::CanonicalNode,
    diagnostic::{Diagnostic, TextRange},
    template::{Filter, TemplateNode},
};

use super::{
    MatchResult,
    capture::{CaptureEnvironment, DerivedHints, MatchContext},
    failure::{MatchFailure, keep_better_failure, literal_failure, missing_failure},
    filters::fields_match,
    list::match_list,
    node::match_node,
    scope::{SearchOptions, SearchScope, collect_candidate_lists},
    trace::{CandidateTrace, candidate_trace},
};

const CLASS_BODY: &str = "ClassBody";
const KEYWORD_TOKEN: &str = "Token";

pub(super) fn match_every(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    template_children: &[TemplateNode],
    source: &CanonicalNode,
    scope: SearchScope,
    kinds: &[String],
    options: &SearchOptions<'_>,
    validate_identifier: &dyn Fn(&str) -> bool,
    mut sink: Option<&mut Vec<CandidateTrace>>,
) -> MatchResult {
    let [pattern @ TemplateNode::Literal { kind, .. }] = template_children else {
        return MatchResult {
            matches: false,
            diagnostics: vec![
                every_shape_diagnostic(template.root.range().clone()).with_rule(rule_id),
            ],
        };
    };
    let class_template = split_class_template(pattern).is_some();
    let accepts_kind = |candidate: &str| {
        if kinds.is_empty() {
            candidate == kind.as_ref()
        } else {
            kinds.iter().any(|listed| listed == candidate)
        }
    };
    let derived_hints = DerivedHints::from_trees(&template.root, source, validate_identifier);
    let mut diagnostics = Vec::new();
    let mut candidate_order = 0;

    for candidate_list in collect_candidate_lists(source, scope, options) {
        for candidate in candidate_list.nodes.iter().filter(|node| {
            accepts_kind(&node.kind) && (!class_template || class_body_index(node).is_some())
        }) {
            let mut context = MatchContext {
                states: 0,
                candidate_start: candidate_order,
                derived_hints: derived_hints.clone(),
                tracing: sink.is_some(),
                depth: 0,
            };
            candidate_order += 1;
            let result =
                match_every_candidate(pattern, candidate, validate_identifier, &mut context);
            if let Some(sink) = sink.as_deref_mut() {
                sink.push(candidate_trace(&candidate.range, &result, rule_id));
            }
            match result {
                Ok(_) => {}
                Err(failure) if failure.exit_class == 2 => {
                    return MatchResult {
                        matches: false,
                        diagnostics: vec![failure.diagnostic(rule_id)],
                    };
                }
                Err(failure) => diagnostics.push(failure.diagnostic(rule_id)),
            }
        }
    }
    diagnostics.sort_by_key(|diagnostic| {
        diagnostic
            .source
            .as_ref()
            .map_or(0, |range| range.start.offset)
    });
    MatchResult {
        matches: diagnostics.is_empty(),
        diagnostics,
    }
}

/// Returns the header items and the body when the every template is a class
/// template, which means its last child is a class body.
fn split_class_template(pattern: &TemplateNode) -> Option<(&[TemplateNode], &TemplateNode)> {
    let TemplateNode::Literal { children, .. } = pattern else {
        return None;
    };
    let (body, header) = children.split_last()?;
    matches!(body, TemplateNode::Literal { kind, .. } if kind.as_ref() == CLASS_BODY)
        .then_some((header, body))
}

fn class_body_index(node: &CanonicalNode) -> Option<usize> {
    node.children
        .iter()
        .rposition(|child| child.kind.as_ref() == CLASS_BODY)
}

fn is_keyword_token(node: &TemplateNode) -> bool {
    match node {
        TemplateNode::Literal { kind, .. } => kind.as_ref() == KEYWORD_TOKEN,
        TemplateNode::Optional { node, .. } => is_keyword_token(node),
        _ => false,
    }
}

/// Matches one every candidate. The root kind is not compared, because the rule
/// already selected the candidate by kind. For a class template, keyword tokens
/// in the header are ignored and the other header items must appear in order,
/// while the class body is compared in full.
fn match_every_candidate(
    pattern: &TemplateNode,
    candidate: &CanonicalNode,
    validate_identifier: &dyn Fn(&str) -> bool,
    context: &mut MatchContext,
) -> Result<super::capture::MatchSuccess, MatchFailure> {
    let TemplateNode::Literal {
        value,
        fields,
        children,
        range,
        ..
    } = pattern
    else {
        return match_node(
            pattern,
            candidate,
            CaptureEnvironment::default(),
            validate_identifier,
            context,
        );
    };
    if candidate.value != *value || !fields_match(fields, &candidate.fields) {
        return Err(literal_failure(
            pattern,
            candidate,
            0,
            context.candidate_start,
        ));
    }
    let result = match (split_class_template(pattern), class_body_index(candidate)) {
        (Some((header, body)), Some(body_index)) => {
            let template_header = header
                .iter()
                .filter(|node| !is_keyword_token(node))
                .collect::<Vec<_>>();
            let source_header = candidate.children[..body_index]
                .iter()
                .filter(|node| node.kind.as_ref() != KEYWORD_TOKEN)
                .collect::<Vec<_>>();
            let class = ClassParts {
                template_header: &template_header,
                source_header: &source_header,
                body,
                source_body: &candidate.children[body_index],
                template_range: range,
                source_range: &candidate.range,
            };
            match_class_header(
                &class,
                0,
                0,
                CaptureEnvironment::default(),
                validate_identifier,
                context,
            )
        }
        _ => match_list(
            children,
            &candidate.children,
            CaptureEnvironment::default(),
            true,
            range,
            &candidate.range,
            validate_identifier,
            context,
        ),
    };
    result
        .map(|mut success| {
            success.matched_nodes += 1;
            success
        })
        .map_err(|mut failure| {
            failure.matched_nodes += 1;
            failure
        })
}

struct ClassParts<'a> {
    template_header: &'a [&'a TemplateNode],
    source_header: &'a [&'a CanonicalNode],
    body: &'a TemplateNode,
    source_body: &'a CanonicalNode,
    template_range: &'a TextRange,
    source_range: &'a TextRange,
}

fn match_class_header(
    class: &ClassParts<'_>,
    template_index: usize,
    source_index: usize,
    environment: CaptureEnvironment,
    validate_identifier: &dyn Fn(&str) -> bool,
    context: &mut MatchContext,
) -> Result<super::capture::MatchSuccess, MatchFailure> {
    let Some(item) = class.template_header.get(template_index) else {
        return match_node(
            class.body,
            class.source_body,
            environment,
            validate_identifier,
            context,
        )
        .map_err(|mut failure| {
            failure.matched_nodes += template_index;
            failure
        });
    };
    let (required, inner) = match item {
        TemplateNode::Optional { node, .. } => (false, node.as_ref()),
        other => (true, *other),
    };
    let mut best_failure = None;
    for (offset, source) in class.source_header[source_index..].iter().enumerate() {
        let attempt = match_node(
            inner,
            source,
            environment.clone(),
            validate_identifier,
            context,
        )
        .map_err(|mut failure| {
            failure.matched_nodes += template_index;
            failure
        })
        .and_then(|success| {
            match_class_header(
                class,
                template_index + 1,
                source_index + offset + 1,
                success.environment,
                validate_identifier,
                context,
            )
        });
        match attempt {
            Ok(success) => return Ok(success),
            Err(failure) if failure.exit_class == 2 => return Err(failure),
            Err(failure) => keep_better_failure(&mut best_failure, failure),
        }
    }
    if !required {
        match match_class_header(
            class,
            template_index + 1,
            source_index,
            environment,
            validate_identifier,
            context,
        ) {
            Ok(success) => return Ok(success),
            Err(failure) if failure.exit_class == 2 => return Err(failure),
            Err(failure) => keep_better_failure(&mut best_failure, failure),
        }
    }
    Err(best_failure.unwrap_or_else(|| {
        missing_failure(
            Some(item),
            class.source_header.get(source_index).copied(),
            class.template_range,
            class.source_range,
            template_index,
            context.candidate_start,
        )
    }))
}

fn every_shape_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic::new(
        "CTC2011",
        crate::diagnostic::DiagnosticCategory::InvalidEveryTemplate,
        "An every template must contain exactly one top-level syntax node without placeholders around it.",
        2,
    )
    .with_template(range)
}

pub fn validate_every_template(root: &TemplateNode) -> Result<(), Vec<Diagnostic>> {
    match root {
        TemplateNode::Literal { children, .. }
            if matches!(children.as_slice(), [TemplateNode::Literal { .. }]) =>
        {
            Ok(())
        }
        _ => Err(vec![every_shape_diagnostic(root.range().clone())]),
    }
}

pub fn validate_adjacent_sequences(node: &TemplateNode) -> Result<(), Vec<Diagnostic>> {
    if let TemplateNode::Optional { node, .. } = node {
        return validate_adjacent_sequences(node);
    }
    let TemplateNode::Literal { children, .. } = node else {
        return Ok(());
    };
    for pair in children.windows(2) {
        if let (TemplateNode::Sequence { placeholder }, TemplateNode::Sequence { .. }) =
            (&pair[0], &pair[1])
            && !placeholder
                .filters
                .iter()
                .any(|filter| matches!(filter, Filter::Kind { .. } | Filter::Field { .. }))
        {
            return Err(vec![
                Diagnostic::new(
                    "CTC2007",
                    crate::diagnostic::DiagnosticCategory::AmbiguousSequence,
                    "A sequence placeholder followed by another sequence placeholder needs a `kind` or `field` filter.",
                    2,
                )
                .with_template(pair[1].range().clone()),
            ]);
        }
    }
    for child in children {
        validate_adjacent_sequences(child)?;
    }
    Ok(())
}
