use crate::{
    canonical::CanonicalNode,
    diagnostic::{Diagnostic, DiagnosticCategory, TextRange},
    template::TemplateNode,
};

use super::{
    MatchResult,
    capture::{CaptureEnvironment, DerivedHints, MatchContext, MatchSuccess},
    failure::{MatchFailure, keep_better_failure},
    list::match_list,
    scope::{MatchCount, MatchMode, SearchOptions, SearchScope, collect_candidate_lists},
    trace::{CandidateTrace, candidate_trace},
};

pub(super) fn match_search(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    template_children: &[TemplateNode],
    source: &CanonicalNode,
    scope: SearchScope,
    options: &SearchOptions<'_>,
    validate_identifier: &dyn Fn(&str) -> bool,
    sink: Option<&mut Vec<CandidateTrace>>,
) -> MatchResult {
    let count = options
        .count
        .filter(|_| template.mode == MatchMode::Contains);
    let full_count = count.is_some_and(MatchCount::needs_full_count);
    let scan = scan_search_candidates(
        rule_id,
        template,
        template_children,
        source,
        scope,
        options,
        validate_identifier,
        sink,
        full_count,
    );
    if let Some(failure) = scan.terminal_failure {
        return MatchResult {
            matches: false,
            diagnostics: vec![failure.diagnostic(rule_id)],
        };
    }
    if scan.short_circuit_match {
        return MatchResult {
            matches: true,
            diagnostics: Vec::new(),
        };
    }
    if full_count && let Some(count) = count {
        return match_count_result(rule_id, template, source, count, scan.successes);
    }
    finish_search(
        rule_id,
        template,
        template_children,
        source,
        scan,
        template.mode,
    )
}

struct SearchScan {
    successes: Vec<(TextRange, MatchSuccess)>,
    best_failure: Option<MatchFailure>,
    terminal_failure: Option<MatchFailure>,
    short_circuit_match: bool,
}

fn scan_search_candidates(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    template_children: &[TemplateNode],
    source: &CanonicalNode,
    scope: SearchScope,
    options: &SearchOptions<'_>,
    validate_identifier: &dyn Fn(&str) -> bool,
    mut sink: Option<&mut Vec<CandidateTrace>>,
    full_count: bool,
) -> SearchScan {
    let derived_hints = DerivedHints::from_trees(&template.root, source, validate_identifier);
    let tracing = sink.is_some();
    let mut successes = Vec::new();
    let mut best_failure = None;
    let mut candidate_ranges = Vec::new();
    let mut pair_states = 0;
    let mut candidate_order = 0;

    for candidate_list in collect_candidate_lists(source, scope, options) {
        for start in 0..candidate_list.nodes.len() {
            let (result, states) = match_search_candidate(
                template,
                template_children,
                validate_identifier,
                &derived_hints,
                candidate_list.nodes,
                candidate_list.fallback,
                start,
                pair_states,
                candidate_order,
                tracing,
            );
            pair_states = states;
            candidate_order += 1;
            record_search_candidate(
                rule_id,
                &mut sink,
                &mut candidate_ranges,
                candidate_list.nodes,
                start,
                &result,
            );
            match result {
                Ok(success)
                    if success.consumed_nodes > 0
                        && template.mode == MatchMode::Contains
                        && !full_count =>
                {
                    return SearchScan {
                        successes,
                        best_failure,
                        terminal_failure: None,
                        short_circuit_match: true,
                    };
                }
                Ok(success) if success.consumed_nodes > 0 => {
                    successes.push((candidate_list.nodes[start].range.clone(), success));
                }
                Ok(_) => {}
                Err(failure) if failure.exit_class == 2 => {
                    return SearchScan {
                        successes: Vec::new(),
                        best_failure: None,
                        terminal_failure: Some(failure),
                        short_circuit_match: false,
                    };
                }
                Err(failure) => keep_better_failure(&mut best_failure, failure),
            }
        }
    }

    if let (Some(sink), Some(failure)) = (sink, &best_failure)
        && successes.is_empty()
        && let Some(candidate_range) = candidate_ranges.get(failure.candidate_start)
    {
        sink.push(candidate_trace(
            candidate_range,
            &Err(failure.clone()),
            rule_id,
        ));
    }
    SearchScan {
        successes,
        best_failure,
        terminal_failure: None,
        short_circuit_match: false,
    }
}

type SearchCandidateResult = (Result<MatchSuccess, MatchFailure>, usize);

fn match_search_candidate(
    template: &crate::template::CompiledTemplate,
    template_children: &[TemplateNode],
    validate_identifier: &dyn Fn(&str) -> bool,
    derived_hints: &DerivedHints,
    candidate_nodes: &[CanonicalNode],
    candidate_fallback: &TextRange,
    start: usize,
    pair_states: usize,
    candidate_order: usize,
    tracing: bool,
) -> SearchCandidateResult {
    let mut context = MatchContext {
        states: pair_states,
        candidate_start: candidate_order,
        derived_hints: derived_hints.clone(),
        tracing,
        depth: 0,
    };
    let result = match_list(
        template_children,
        &candidate_nodes[start..],
        CaptureEnvironment::default(),
        false,
        template.root.range(),
        candidate_fallback,
        validate_identifier,
        &mut context,
    );
    (result, context.states)
}

fn record_search_candidate(
    rule_id: &str,
    sink: &mut Option<&mut Vec<CandidateTrace>>,
    candidate_ranges: &mut Vec<TextRange>,
    nodes: &[CanonicalNode],
    start: usize,
    result: &Result<MatchSuccess, MatchFailure>,
) {
    let Some(sink) = sink.as_deref_mut() else {
        return;
    };
    let candidate_range = &nodes[start].range;
    candidate_ranges.push(candidate_range.clone());
    let reported = match result {
        Ok(success) => success.consumed_nodes > 0,
        Err(failure) => failure.exit_class == 2,
    };
    if reported {
        sink.push(candidate_trace(candidate_range, result, rule_id));
    }
}

fn finish_search(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    template_children: &[TemplateNode],
    source: &CanonicalNode,
    scan: SearchScan,
    mode: MatchMode,
) -> MatchResult {
    match mode {
        MatchMode::Contains if !scan.successes.is_empty() => MatchResult {
            matches: true,
            diagnostics: Vec::new(),
        },
        MatchMode::Contains => MatchResult {
            matches: false,
            diagnostics: scan
                .best_failure
                .map(|failure| vec![failure.diagnostic(rule_id)])
                .unwrap_or_else(|| {
                    vec![
                        Diagnostic::new(
                            "CTC3002",
                            DiagnosticCategory::MissingSyntaxNode,
                            "The source does not contain the required structure.",
                            1,
                        )
                        .with_rule(rule_id)
                        .with_source(source.range.clone())
                        .with_template(template.root.range().clone()),
                    ]
                }),
        },
        MatchMode::Forbid if scan.successes.is_empty() => MatchResult {
            matches: true,
            diagnostics: Vec::new(),
        },
        MatchMode::Forbid => MatchResult {
            matches: false,
            diagnostics: scan
                .successes
                .into_iter()
                .map(|(source_range, _)| {
                    Diagnostic::new(
                        "CTC3006",
                        DiagnosticCategory::ForbiddenStructure,
                        "The source contains a forbidden structure.",
                        1,
                    )
                    .with_rule(rule_id)
                    .with_source(source_range)
                    .with_template(
                        template_children
                            .first()
                            .map_or_else(|| template.root.range(), TemplateNode::range)
                            .clone(),
                    )
                })
                .collect(),
        },
        MatchMode::Exact | MatchMode::Every => MatchResult {
            matches: false,
            diagnostics: vec![
                Diagnostic::new(
                    "CTC9001",
                    DiagnosticCategory::InternalToolError,
                    "Exact mode entered the search matcher.",
                    2,
                )
                .with_rule(rule_id)
                .with_template(template.root.range().clone()),
            ],
        },
    }
}

/// Turns the matches of a contains rule with a `count` into diagnostics.
fn match_count_result(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    source: &CanonicalNode,
    count: MatchCount,
    successes: Vec<(TextRange, MatchSuccess)>,
) -> MatchResult {
    let found = successes.len() as u64;
    let mut diagnostics = Vec::new();
    if found < u64::from(count.min) {
        diagnostics.push(
            Diagnostic::new(
                "CTC3008",
                DiagnosticCategory::TooFewMatches,
                "The source contains fewer matches of the template than the rule requires.",
                1,
            )
            .with_rule(rule_id)
            .with_source(source.range.clone())
            .with_template(template.root.range().clone())
            .with_values(format!("at least {}", count.min), found.to_string()),
        );
    }
    if let Some(max) = count.max {
        for (range, _) in successes.into_iter().skip(max as usize) {
            diagnostics.push(
                Diagnostic::new(
                    "CTC3009",
                    DiagnosticCategory::TooManyMatches,
                    "The source contains more matches of the template than the rule allows.",
                    1,
                )
                .with_rule(rule_id)
                .with_source(range)
                .with_template(template.root.range().clone())
                .with_values(format!("at most {max}"), found.to_string()),
            );
        }
    }
    MatchResult {
        matches: diagnostics.is_empty(),
        diagnostics,
    }
}
