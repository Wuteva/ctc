use std::collections::HashSet;

use crate::{
    canonical::CanonicalNode,
    diagnostic::{DiagnosticCategory, TextRange},
    template::{Placeholder, TemplateNode},
};

use super::{
    capture::{CaptureEnvironment, CaptureValue, MatchContext, MatchSuccess},
    failure::{
        MatchFailure, capture_failure, category_failure, keep_better_failure, missing_failure,
        unexpected_failure,
    },
    filters::{constraints_match, rejected_filter},
    node::match_node,
    trace::{FailureTrace, TraceStep, TraceStepKind, sequence_step, template_kind, template_name},
};

const MAX_SEQUENCE_STATES: usize = 100_000;

pub(super) fn match_list(
    template: &[TemplateNode],
    source: &[CanonicalNode],
    environment: CaptureEnvironment,
    require_source_end: bool,
    template_fallback: &TextRange,
    source_fallback: &TextRange,
    validate_identifier: &dyn Fn(&str) -> bool,
    context: &mut MatchContext,
) -> Result<MatchSuccess, MatchFailure> {
    let mut memo = HashSet::new();
    ListMatcher {
        template,
        source,
        require_source_end,
        template_fallback,
        source_fallback,
        validate_identifier,
        context,
        memo: &mut memo,
    }
    .match_from(0, 0, environment)
}

struct ListMatcher<'a, 'b, 'c> {
    template: &'a [TemplateNode],
    source: &'a [CanonicalNode],
    require_source_end: bool,
    template_fallback: &'a TextRange,
    source_fallback: &'a TextRange,
    validate_identifier: &'b dyn Fn(&str) -> bool,
    context: &'c mut MatchContext,
    memo: &'c mut HashSet<(usize, usize, u64)>,
}

impl<'a, 'b, 'c> ListMatcher<'a, 'b, 'c> {
    fn match_from(
        &mut self,
        template_index: usize,
        source_index: usize,
        environment: CaptureEnvironment,
    ) -> Result<MatchSuccess, MatchFailure> {
        let trace = self
            .context
            .tracing
            .then(|| environment.trace.clone())
            .flatten();
        self.match_from_inner(template_index, source_index, environment)
            .map_err(|failure| {
                if self.context.tracing {
                    failure.with_trace(trace)
                } else {
                    failure
                }
            })
    }

    fn match_from_inner(
        &mut self,
        template_index: usize,
        source_index: usize,
        environment: CaptureEnvironment,
    ) -> Result<MatchSuccess, MatchFailure> {
        self.context.states += 1;
        self.check_state_limit(template_index, source_index)?;

        let memo_key = (template_index, source_index, environment.fingerprint());
        if self.memo.contains(&memo_key) {
            return Err(self.memo_failure(template_index, source_index));
        }
        if template_index == self.template.len() {
            return self.finish_template(source_index, environment);
        }
        match &self.template[template_index] {
            TemplateNode::Optional { node, range } => self.match_optional(
                template_index,
                source_index,
                environment,
                node,
                range,
                memo_key,
            ),
            TemplateNode::Sequence { placeholder } => self.match_sequence(
                template_index,
                source_index,
                environment,
                placeholder,
                memo_key,
            ),
            _ => self.match_required(template_index, source_index, environment, memo_key),
        }
    }

    fn check_state_limit(
        &self,
        template_index: usize,
        source_index: usize,
    ) -> Result<(), MatchFailure> {
        if self.context.states <= MAX_SEQUENCE_STATES {
            return Ok(());
        }
        Err(MatchFailure {
            code: "CTC2009",
            category: DiagnosticCategory::TemplateComplexity,
            message: "The sequence matcher reached its 100,000-state limit.".to_string(),
            template: self.template.get(template_index).map_or_else(
                || self.template_fallback.clone(),
                |node| node.range().clone(),
            ),
            source: self
                .source
                .get(source_index)
                .map(|node| node.range.clone())
                .or_else(|| Some(self.source_fallback.clone())),
            expected: None,
            actual: None,
            matched_nodes: 0,
            source_offset: self
                .source
                .get(source_index)
                .map_or(self.source_fallback.start.offset, |node| {
                    node.range.start.offset
                }),
            exit_class: 2,
            candidate_start: self.context.candidate_start,
            trace: None,
        })
    }

    fn memo_failure(&self, template_index: usize, source_index: usize) -> MatchFailure {
        missing_failure(
            self.template.get(template_index),
            self.source.get(source_index),
            self.template_fallback,
            self.source_fallback,
            template_index,
            self.context.candidate_start,
        )
    }

    fn finish_template(
        &self,
        source_index: usize,
        environment: CaptureEnvironment,
    ) -> Result<MatchSuccess, MatchFailure> {
        if self.require_source_end && source_index < self.source.len() {
            let mut failure = unexpected_failure(
                &self.source[source_index],
                self.template_fallback,
                self.template.len(),
                self.context.candidate_start,
            );
            if self.context.tracing {
                failure.trace = Some(Box::new(FailureTrace {
                    reason: group_reason(self.template, &self.source[source_index], &environment),
                    steps: environment.trace,
                }));
            }
            return Err(failure);
        }
        Ok(MatchSuccess {
            environment,
            matched_nodes: 0,
            consumed_nodes: source_index,
        })
    }

    fn match_optional(
        &mut self,
        template_index: usize,
        source_index: usize,
        environment: CaptureEnvironment,
        node: &TemplateNode,
        range: &TextRange,
        memo_key: (usize, usize, u64),
    ) -> Result<MatchSuccess, MatchFailure> {
        let mut best_failure = None;
        let mut skipped_environment = environment.clone();
        if self.context.tracing {
            skipped_environment.push_step(TraceStep {
                depth: self.context.depth,
                kind: TraceStepKind::OptionalAbsent,
                name: template_name(node),
                template_kind: template_kind(node),
                template: range.clone(),
                nodes: Vec::new(),
            });
        }
        if let Some(success) = self.try_optional_skip(
            template_index,
            source_index,
            skipped_environment,
            &mut best_failure,
        )? {
            return Ok(success);
        }
        if let Some(success) = self.try_optional_match(
            template_index,
            source_index,
            environment,
            node,
            &mut best_failure,
        )? {
            return Ok(success);
        }
        self.memo.insert(memo_key);
        Err(best_failure.unwrap_or_else(|| {
            missing_failure(
                Some(&self.template[template_index]),
                self.source.get(source_index),
                self.template_fallback,
                self.source_fallback,
                template_index,
                self.context.candidate_start,
            )
        }))
    }

    fn try_optional_skip(
        &mut self,
        template_index: usize,
        source_index: usize,
        environment: CaptureEnvironment,
        best_failure: &mut Option<MatchFailure>,
    ) -> Result<Option<MatchSuccess>, MatchFailure> {
        match self.match_from(template_index + 1, source_index, environment) {
            Ok(success) => Ok(Some(success)),
            Err(failure) if failure.exit_class == 2 => Err(failure),
            Err(failure) => {
                keep_better_failure(best_failure, failure);
                Ok(None)
            }
        }
    }

    fn try_optional_match(
        &mut self,
        template_index: usize,
        source_index: usize,
        environment: CaptureEnvironment,
        node: &TemplateNode,
        best_failure: &mut Option<MatchFailure>,
    ) -> Result<Option<MatchSuccess>, MatchFailure> {
        let Some(source_node) = self.source.get(source_index) else {
            return Ok(None);
        };
        match match_node(
            node,
            source_node,
            environment,
            self.validate_identifier,
            self.context,
        ) {
            Ok(matched) => {
                match self.match_from(template_index + 1, source_index + 1, matched.environment) {
                    Ok(mut success) => {
                        success.matched_nodes += matched.matched_nodes;
                        Ok(Some(success))
                    }
                    Err(failure) if failure.exit_class == 2 => Err(failure),
                    Err(mut failure) => {
                        failure.matched_nodes += matched.matched_nodes;
                        keep_better_failure(best_failure, failure);
                        Ok(None)
                    }
                }
            }
            Err(failure) if failure.exit_class == 2 => Err(failure),
            Err(failure) => {
                keep_better_failure(best_failure, failure);
                Ok(None)
            }
        }
    }

    fn match_sequence(
        &mut self,
        template_index: usize,
        source_index: usize,
        environment: CaptureEnvironment,
        placeholder: &Placeholder,
        memo_key: (usize, usize, u64),
    ) -> Result<MatchSuccess, MatchFailure> {
        let earlier_groups = self.template[..template_index]
            .iter()
            .rev()
            .map_while(|node| match node {
                TemplateNode::Sequence { placeholder } => Some(placeholder),
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some(capture) = environment.values.get(&placeholder.name).cloned() {
            return self.match_bound_sequence(
                template_index,
                source_index,
                environment,
                placeholder,
                &earlier_groups,
                capture,
            );
        }
        self.match_unbound_sequence(
            template_index,
            source_index,
            environment,
            placeholder,
            &earlier_groups,
            memo_key,
        )
    }

    fn match_bound_sequence(
        &mut self,
        template_index: usize,
        source_index: usize,
        mut environment: CaptureEnvironment,
        placeholder: &Placeholder,
        earlier_groups: &[&Placeholder],
        capture: CaptureValue,
    ) -> Result<MatchSuccess, MatchFailure> {
        let CaptureValue::Sequence(expected) = capture else {
            return Err(category_failure(
                placeholder,
                self.source.get(source_index),
                template_index,
                self.context.candidate_start,
            ));
        };
        if source_index + expected.len() > self.source.len() {
            return Err(missing_failure(
                Some(&self.template[template_index]),
                None,
                self.template_fallback,
                self.source_fallback,
                template_index,
                self.context.candidate_start,
            ));
        }
        for (expected_node, actual_node) in expected
            .iter()
            .zip(&self.source[source_index..source_index + expected.len()])
        {
            if !expected_node.structurally_eq(actual_node)
                || !sequence_fits(actual_node, placeholder, earlier_groups)
            {
                return Err(capture_failure(
                    placeholder,
                    actual_node,
                    template_index,
                    self.context.candidate_start,
                ));
            }
        }
        if self.context.tracing {
            environment.push_step(sequence_step(
                placeholder,
                &self.source[source_index..source_index + expected.len()],
                self.context.depth,
            ));
        }
        self.match_from(
            template_index + 1,
            source_index + expected.len(),
            environment,
        )
        .map(|mut success| {
            success.matched_nodes += expected.len();
            success
        })
        .map_err(|mut failure| {
            failure.matched_nodes += expected.len();
            failure
        })
    }

    fn match_unbound_sequence(
        &mut self,
        template_index: usize,
        source_index: usize,
        environment: CaptureEnvironment,
        placeholder: &Placeholder,
        earlier_groups: &[&Placeholder],
        memo_key: (usize, usize, u64),
    ) -> Result<MatchSuccess, MatchFailure> {
        let mut best_failure = None;
        for length in 0..=self.source.len() - source_index {
            let candidate = &self.source[source_index..source_index + length];
            if !candidate
                .iter()
                .all(|node| sequence_fits(node, placeholder, earlier_groups))
            {
                break;
            }
            let mut next_environment = environment.clone();
            next_environment.values.insert(
                placeholder.name.clone(),
                CaptureValue::Sequence(candidate.to_vec()),
            );
            if self.context.tracing {
                next_environment.push_step(sequence_step(
                    placeholder,
                    candidate,
                    self.context.depth,
                ));
            }
            match self.match_from(template_index + 1, source_index + length, next_environment) {
                Ok(mut success) => {
                    success.matched_nodes += length;
                    return Ok(success);
                }
                Err(failure) if failure.exit_class == 2 => return Err(failure),
                Err(mut failure) => {
                    failure.matched_nodes += length;
                    keep_better_failure(&mut best_failure, failure);
                }
            }
        }
        self.memo.insert(memo_key);
        Err(best_failure.unwrap_or_else(|| {
            missing_failure(
                Some(&self.template[template_index]),
                self.source.get(source_index),
                self.template_fallback,
                self.source_fallback,
                template_index,
                self.context.candidate_start,
            )
        }))
    }

    fn match_required(
        &mut self,
        template_index: usize,
        source_index: usize,
        environment: CaptureEnvironment,
        memo_key: (usize, usize, u64),
    ) -> Result<MatchSuccess, MatchFailure> {
        let Some(source_node) = self.source.get(source_index) else {
            self.memo.insert(memo_key);
            return Err(missing_failure(
                Some(&self.template[template_index]),
                None,
                self.template_fallback,
                self.source_fallback,
                template_index,
                self.context.candidate_start,
            ));
        };
        let group_environment = self.context.tracing.then(|| environment.clone());
        let matched = match_node(
            &self.template[template_index],
            source_node,
            environment,
            self.validate_identifier,
            self.context,
        )
        .map_err(|mut failure| {
            if let (Some(group_environment), Some(trace)) = (&group_environment, &mut failure.trace)
                && failure.template == *self.template[template_index].range()
                && failure.source.as_ref() == Some(&source_node.range)
            {
                trace.reason = group_reason(
                    &self.template[..template_index],
                    source_node,
                    group_environment,
                );
            }
            failure
        })?;
        match self.match_from(template_index + 1, source_index + 1, matched.environment) {
            Ok(mut success) => {
                success.matched_nodes += matched.matched_nodes;
                Ok(success)
            }
            Err(mut failure) => {
                failure.matched_nodes += matched.matched_nodes;
                self.memo.insert(memo_key);
                Err(failure)
            }
        }
    }
}

fn sequence_fits(
    node: &CanonicalNode,
    placeholder: &Placeholder,
    earlier_groups: &[&Placeholder],
) -> bool {
    constraints_match(node, placeholder)
        && !earlier_groups
            .iter()
            .any(|earlier| constraints_match(node, earlier))
}

/// Explains why `node` could not join one of the sequence groups in `template`.
fn group_reason(
    template: &[TemplateNode],
    node: &CanonicalNode,
    environment: &CaptureEnvironment,
) -> Option<String> {
    let groups = template
        .iter()
        .filter_map(|template_node| match template_node {
            TemplateNode::Sequence { placeholder } => Some(placeholder),
            _ => None,
        })
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return None;
    }
    let label = format!("The node at line {} ({})", node.range.start.line, node.kind);
    if let Some(position) = groups
        .iter()
        .position(|group| constraints_match(node, group))
    {
        let (later, first) = groups[position + 1..].iter().find_map(|group| {
            match environment.values.get(&group.name) {
                Some(CaptureValue::Sequence(nodes)) => nodes.first().map(|first| (group, first)),
                _ => None,
            }
        })?;
        return Some(format!(
            "{label} passes group `{}`, but the later group `{}` already started at line {}.",
            groups[position].name, later.name, first.range.start.line
        ));
    }
    let mut reason = format!("{label} passes no group.");
    for group in groups {
        if let Some(rejection) = rejected_filter(node, group) {
            reason.push_str(&format!(" `{}`: {rejection}.", group.name));
        }
    }
    Some(reason)
}
