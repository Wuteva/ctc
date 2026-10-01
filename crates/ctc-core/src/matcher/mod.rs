use crate::{
    canonical::CanonicalNode,
    diagnostic::{Diagnostic, DiagnosticCategory},
    template::TemplateNode,
};

mod capture;
mod every;
mod failure;
mod filters;
mod list;
mod node;
mod scope;
mod search;
mod trace;

#[cfg(test)]
mod tests;

pub use every::{validate_adjacent_sequences, validate_every_template};
pub use scope::{MatchCount, MatchMode, SearchOptions, SearchScope};
pub use trace::{
    CandidateTrace, MatchExplanation, TraceFailure, TraceNode, TraceStep, TraceStepKind,
};

use capture::{CaptureEnvironment, DerivedHints, MatchContext};
use failure::literal_failure;
use filters::fields_match;
use list::match_list;
use scope::MatchMode::{Contains, Every, Exact, Forbid};

#[derive(Clone, Debug)]
pub struct MatchResult {
    pub matches: bool,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn match_template(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    source: &CanonicalNode,
    validate_identifier: &dyn Fn(&str) -> bool,
) -> MatchResult {
    match_template_in_scope(
        rule_id,
        template,
        source,
        SearchScope::TopLevel,
        validate_identifier,
    )
}

pub fn match_template_in_scope(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    source: &CanonicalNode,
    scope: SearchScope,
    validate_identifier: &dyn Fn(&str) -> bool,
) -> MatchResult {
    run_match(
        rule_id,
        template,
        source,
        scope,
        &[],
        &SearchOptions::default(),
        validate_identifier,
        None,
    )
}

/// Like `match_template_in_scope`, but in every mode `kinds` lists the candidate
/// node kinds to check. An empty list means the kind of the template node.
pub fn match_template_in_scope_with_kinds(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    source: &CanonicalNode,
    scope: SearchScope,
    kinds: &[String],
    validate_identifier: &dyn Fn(&str) -> bool,
) -> MatchResult {
    match_template_with_options(
        rule_id,
        template,
        source,
        scope,
        kinds,
        &SearchOptions::default(),
        validate_identifier,
    )
}

/// Like `match_template_in_scope_with_kinds`, with the rule settings in
/// `options`.
pub fn match_template_with_options(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    source: &CanonicalNode,
    scope: SearchScope,
    kinds: &[String],
    options: &SearchOptions<'_>,
    validate_identifier: &dyn Fn(&str) -> bool,
) -> MatchResult {
    run_match(
        rule_id,
        template,
        source,
        scope,
        kinds,
        options,
        validate_identifier,
        None,
    )
}

/// Runs the same match as `match_template_in_scope_with_kinds` and also
/// records, for each reported candidate, the bindings of the final successful
/// path or of the best failed attempt.
pub fn explain_template_in_scope(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    source: &CanonicalNode,
    scope: SearchScope,
    kinds: &[String],
    validate_identifier: &dyn Fn(&str) -> bool,
) -> MatchExplanation {
    explain_template_with_options(
        rule_id,
        template,
        source,
        scope,
        kinds,
        &SearchOptions::default(),
        validate_identifier,
    )
}

/// Like `explain_template_in_scope`, with the rule settings in `options`.
pub fn explain_template_with_options(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    source: &CanonicalNode,
    scope: SearchScope,
    kinds: &[String],
    options: &SearchOptions<'_>,
    validate_identifier: &dyn Fn(&str) -> bool,
) -> MatchExplanation {
    let mut candidates = Vec::new();
    let result = run_match(
        rule_id,
        template,
        source,
        scope,
        kinds,
        options,
        validate_identifier,
        Some(&mut candidates),
    );
    candidates.sort_by_key(|candidate| candidate.source.start.offset);
    MatchExplanation { result, candidates }
}

fn run_match(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    source: &CanonicalNode,
    scope: SearchScope,
    kinds: &[String],
    options: &SearchOptions<'_>,
    validate_identifier: &dyn Fn(&str) -> bool,
    sink: Option<&mut Vec<CandidateTrace>>,
) -> MatchResult {
    let TemplateNode::Literal {
        kind,
        value,
        fields,
        children,
        range,
    } = &template.root
    else {
        return invalid_root_result(rule_id, &template.root);
    };

    if source.kind != *kind || source.value != *value || !fields_match(fields, &source.fields) {
        let failure = literal_failure(&template.root, source, 0, 0);
        return MatchResult {
            matches: false,
            diagnostics: vec![failure.diagnostic(rule_id)],
        };
    }

    match template.mode {
        Exact => run_exact_match(
            rule_id,
            template,
            source,
            scope,
            children,
            range,
            validate_identifier,
            sink,
        ),
        Contains | Forbid => search::match_search(
            rule_id,
            template,
            children,
            source,
            scope,
            options,
            validate_identifier,
            sink,
        ),
        Every => every::match_every(
            rule_id,
            template,
            children,
            source,
            scope,
            kinds,
            options,
            validate_identifier,
            sink,
        ),
    }
}

fn invalid_root_result(rule_id: &str, root: &TemplateNode) -> MatchResult {
    MatchResult {
        matches: false,
        diagnostics: vec![
            Diagnostic::new(
                "CTC9001",
                DiagnosticCategory::InternalToolError,
                "The compiled template root is not a literal source-file node.",
                2,
            )
            .with_rule(rule_id)
            .with_template(root.range().clone()),
        ],
    }
}

fn run_exact_match(
    rule_id: &str,
    template: &crate::template::CompiledTemplate,
    source: &CanonicalNode,
    scope: SearchScope,
    children: &[TemplateNode],
    range: &crate::diagnostic::TextRange,
    validate_identifier: &dyn Fn(&str) -> bool,
    sink: Option<&mut Vec<CandidateTrace>>,
) -> MatchResult {
    if scope != SearchScope::TopLevel {
        return MatchResult {
            matches: false,
            diagnostics: vec![
                Diagnostic::new(
                    "CTC1010",
                    DiagnosticCategory::InvalidConfiguration,
                    "Exact mode supports only the topLevel search scope.",
                    2,
                )
                .with_rule(rule_id)
                .with_template(template.root.range().clone()),
            ],
        };
    }

    let derived_hints = DerivedHints::from_trees(&template.root, source, validate_identifier);
    let mut context = MatchContext {
        states: 0,
        candidate_start: 0,
        derived_hints,
        tracing: sink.is_some(),
        depth: 0,
    };
    let result = match_list(
        children,
        &source.children,
        CaptureEnvironment::default(),
        true,
        range,
        &source.range,
        validate_identifier,
        &mut context,
    );
    if let Some(sink) = sink {
        sink.push(trace::candidate_trace(&source.range, &result, rule_id));
    }
    match result {
        Ok(_) => MatchResult {
            matches: true,
            diagnostics: Vec::new(),
        },
        Err(failure) => MatchResult {
            matches: false,
            diagnostics: vec![failure.diagnostic(rule_id)],
        },
    }
}
