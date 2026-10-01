use std::path::Path;

use globset::GlobSet;
use serde::Deserialize;

use crate::{
    canonical::{ExceptionKind, MemberFunctionFact, MemberFunctionRole, SemanticFacts},
    diagnostic::{Diagnostic, DiagnosticCategory, TextRange},
};

#[derive(Clone)]
pub enum SemanticRule {
    ReturnPaths {
        type_name: String,
    },
    ExceptionPolicy {
        forbid_try: bool,
        forbid_throw: bool,
        forbid_promise_reject: bool,
        exception_sources: GlobSet,
    },
    CompanionFile,
    FileLength {
        max_lines: u32,
    },
    HeaderSourcePairing {
        missing_source: MissingPartner,
        check_order: bool,
    },
}

/// What a partner-file rule does when no candidate partner file exists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MissingPartner {
    #[default]
    Skip,
    Report,
}

pub fn evaluate_semantic_rule(
    rule_id: &str,
    rule: &SemanticRule,
    facts: &SemanticFacts,
) -> Vec<Diagnostic> {
    match rule {
        SemanticRule::ReturnPaths { type_name } => evaluate_return_paths(rule_id, type_name, facts),
        SemanticRule::ExceptionPolicy {
            forbid_try,
            forbid_throw,
            forbid_promise_reject,
            exception_sources,
        } => evaluate_exception_policy(
            rule_id,
            *forbid_try,
            *forbid_throw,
            *forbid_promise_reject,
            exception_sources,
            facts,
        ),
        SemanticRule::CompanionFile
        | SemanticRule::FileLength { .. }
        | SemanticRule::HeaderSourcePairing { .. } => Vec::new(),
    }
}

fn evaluate_return_paths(rule_id: &str, type_name: &str, facts: &SemanticFacts) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for function in &facts.functions {
        if !function
            .return_type_names
            .iter()
            .any(|name| name == type_name)
        {
            continue;
        }
        if !function.all_paths_return_value {
            diagnostics.push(
                Diagnostic::new(
                    "CTC4001",
                    DiagnosticCategory::ReturnPathFallthrough,
                    format!(
                        "Function `{}` can complete without returning `{type_name}`.",
                        function.name
                    ),
                    1,
                )
                .with_rule(rule_id)
                .with_source(function.range.clone())
                .with_values(
                    format!("all code paths return `{type_name}`"),
                    "a path can complete without a value",
                ),
            );
        }
        for range in &function.bare_returns {
            diagnostics.push(
                Diagnostic::new(
                    "CTC4002",
                    DiagnosticCategory::ReturnPathBareReturn,
                    format!(
                        "Function `{}` contains a return without a value.",
                        function.name
                    ),
                    1,
                )
                .with_rule(rule_id)
                .with_source(range.clone())
                .with_values(format!("a `{type_name}` value"), "bare return"),
            );
        }
    }
    diagnostics
}

fn evaluate_exception_policy(
    rule_id: &str,
    forbid_try: bool,
    forbid_throw: bool,
    forbid_promise_reject: bool,
    exception_sources: &GlobSet,
    facts: &SemanticFacts,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for exception in &facts.exceptions {
        let (enabled, code, category, message) = match exception.kind {
            ExceptionKind::Try => (
                forbid_try,
                "CTC4101",
                DiagnosticCategory::ForbiddenTry,
                "A try statement is forbidden by this exception policy.",
            ),
            ExceptionKind::Throw => (
                forbid_throw,
                "CTC4102",
                DiagnosticCategory::ForbiddenThrow,
                "A throw statement is forbidden by this exception policy.",
            ),
            ExceptionKind::PromiseReject => (
                forbid_promise_reject,
                "CTC4103",
                DiagnosticCategory::PromiseRejection,
                "Promise rejection is forbidden by this exception policy.",
            ),
        };
        if enabled {
            diagnostics.push(
                Diagnostic::new(code, category, message, 1)
                    .with_rule(rule_id)
                    .with_source(exception.range.clone()),
            );
        }
    }
    for call in &facts.calls {
        if exception_sources.is_match(&call.callee)
            && !(forbid_promise_reject && call.callee == "Promise.reject")
        {
            diagnostics.push(
                Diagnostic::new(
                    "CTC4104",
                    DiagnosticCategory::ExceptionSourceCall,
                    format!(
                        "Call to configured exception source `{}` is forbidden.",
                        call.callee
                    ),
                    1,
                )
                .with_rule(rule_id)
                .with_source(call.range.clone()),
            );
        }
    }
    diagnostics
}

/// Reports a file with more than max_lines lines. The diagnostic points at the
/// first line over the limit.
pub fn file_too_long(rule_id: &str, path: &Path, text: &str, max_lines: u32) -> Option<Diagnostic> {
    let lines = text.lines().count();
    if lines <= max_lines as usize {
        return None;
    }
    let offset = text
        .split_inclusive('\n')
        .take(max_lines as usize)
        .map(str::len)
        .sum();
    Some(
        Diagnostic::new(
            "CTC4301",
            DiagnosticCategory::FileTooLong,
            "The file has more lines than the rule allows.",
            1,
        )
        .with_rule(rule_id)
        .with_source(TextRange::from_offsets(path, text, offset, offset))
        .with_values(
            format!("at most {max_lines} lines"),
            format!("{lines} lines"),
        ),
    )
}

/// Reports a selected file that has no companion file.
pub fn missing_companion_file(rule_id: &str, anchor: &Path, candidates: &[String]) -> Diagnostic {
    missing_partner(
        rule_id,
        anchor,
        format!(
            "No companion file found for `{}`. Expected one of: {}.",
            display(anchor),
            candidate_list(candidates)
        ),
    )
}

/// Reports a header that declares member functions but has no source file.
pub fn missing_source_file(rule_id: &str, anchor: &Path, candidates: &[String]) -> Diagnostic {
    missing_partner(
        rule_id,
        anchor,
        format!(
            "No source file found for header `{}`, which declares member functions that need definitions. Expected one of: {}.",
            display(anchor),
            candidate_list(candidates)
        ),
    )
}

fn missing_partner(rule_id: &str, anchor: &Path, message: String) -> Diagnostic {
    Diagnostic::new(
        "CTC4201",
        DiagnosticCategory::MissingPartnerFile,
        message,
        1,
    )
    .with_rule(rule_id)
    .with_source(TextRange::from_offsets(anchor, "", 0, 0))
}

fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn candidate_list(candidates: &[String]) -> String {
    candidates
        .iter()
        .map(|candidate| format!("`{candidate}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Returns `true` when the header declares a member function that needs a
/// definition outside the header.
pub fn header_needs_definitions(header: &SemanticFacts) -> bool {
    !required_declarations(header).is_empty()
}

/// Checks that each member function declared in `header` is defined in
/// `source`, and that the definitions follow the declaration order.
pub fn evaluate_header_source_pairing(
    rule_id: &str,
    header: &SemanticFacts,
    source_path: &Path,
    source: &SemanticFacts,
    check_order: bool,
) -> Vec<Diagnostic> {
    let required = required_declarations(header);
    let definitions = source
        .member_functions
        .iter()
        .filter(|fact| fact.role == MemberFunctionRole::Definition)
        .collect::<Vec<_>>();
    let matched = matched_definitions(&required, &definitions);
    let source_display = display(source_path);
    let mut diagnostics =
        missing_definition_diagnostics(rule_id, &required, &definitions, &matched, &source_display);
    if !check_order {
        return diagnostics;
    }
    diagnostics.extend(order_diagnostics(
        rule_id,
        &required,
        &definitions,
        &matched,
    ));
    diagnostics
}

fn matched_definitions(
    required: &[&MemberFunctionFact],
    definitions: &[&MemberFunctionFact],
) -> Vec<Option<usize>> {
    let mut matched = vec![None; required.len()];
    let mut used = vec![false; definitions.len()];
    // Exact owners are paired first, so a shorter owner written after
    // `using namespace` cannot take a definition from an exact match.
    for exact in [true, false] {
        for (index, declaration) in required.iter().enumerate() {
            if matched[index].is_some() {
                continue;
            }
            let found = definitions
                .iter()
                .enumerate()
                .position(|(position, definition)| {
                    !used[position] && same_function(declaration, definition, exact)
                });
            if let Some(position) = found {
                matched[index] = Some(position);
                used[position] = true;
            }
        }
    }
    matched
}

fn missing_definition_diagnostics(
    rule_id: &str,
    required: &[&MemberFunctionFact],
    definitions: &[&MemberFunctionFact],
    matched: &[Option<usize>],
    source_display: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (declaration, definition) in required.iter().zip(matched) {
        if definition.is_some() {
            continue;
        }
        let message = missing_definition_message(declaration, definitions, source_display);
        diagnostics.push(
            Diagnostic::new(
                "CTC4202",
                DiagnosticCategory::MissingMemberDefinition,
                message,
                1,
            )
            .with_rule(rule_id)
            .with_source(declaration.range.clone()),
        );
    }
    diagnostics
}

fn missing_definition_message(
    declaration: &MemberFunctionFact,
    definitions: &[&MemberFunctionFact],
    source_display: &str,
) -> String {
    let mut message = format!(
        "`{}` is declared here but not defined in `{source_display}`.",
        display_name(declaration)
    );
    if let Some(other) = definitions.iter().find(|definition| {
        definition.name == declaration.name
            && (owner_matches(&declaration.owner, &definition.owner, true)
                || owner_matches(&declaration.owner, &definition.owner, false))
    }) {
        message.push_str(&format!(
            " A definition with other parameters exists at `{}`: `{}`.",
            location(&other.range),
            display_name(other)
        ));
    }
    message
}

fn order_diagnostics(
    rule_id: &str,
    required: &[&MemberFunctionFact],
    definitions: &[&MemberFunctionFact],
    matched: &[Option<usize>],
) -> Vec<Diagnostic> {
    let mut pairs = matched
        .iter()
        .enumerate()
        .filter_map(|(declaration, definition)| {
            definition.map(|definition| (definition, declaration))
        })
        .collect::<Vec<_>>();
    pairs.sort_unstable();
    let header_order = pairs
        .iter()
        .map(|(_, declaration)| *declaration)
        .collect::<Vec<_>>();
    let keep = longest_increasing(&header_order);
    let kept = header_order
        .iter()
        .zip(&keep)
        .filter(|(_, keep)| **keep)
        .map(|(declaration, _)| *declaration)
        .collect::<Vec<_>>();
    let mut sorted_header_order = header_order.clone();
    sorted_header_order.sort_unstable();
    let total = pairs.len();
    let mut diagnostics = Vec::new();
    for (source_position, ((definition, declaration), keep)) in pairs.iter().zip(&keep).enumerate()
    {
        if *keep {
            continue;
        }
        let declared = required[*declaration];
        let defined = definitions[*definition];
        let neighbor = order_neighbor(required, &kept, *declaration);
        let header_position = sorted_header_order
            .binary_search(declaration)
            .expect("each paired declaration is in the header order");
        diagnostics.push(
            Diagnostic::new(
                "CTC4203",
                DiagnosticCategory::MemberDefinitionOrder,
                format!(
                    "Definition of `{}` is out of order. `{}` {neighbor}.",
                    display_name(defined),
                    location(&declared.range)
                ),
                1,
            )
            .with_rule(rule_id)
            .with_source(defined.range.clone())
            .with_values(
                format!("position {} of {total} (header order)", header_position + 1),
                format!("position {} of {total}", source_position + 1),
            ),
        );
    }
    diagnostics
}

fn order_neighbor(required: &[&MemberFunctionFact], kept: &[usize], declaration: usize) -> String {
    match kept.iter().rev().find(|kept| **kept < declaration) {
        Some(previous) => format!(
            "declares it after `{}`, so define it after that function",
            display_name(required[*previous])
        ),
        None => {
            let next = kept
                .iter()
                .find(|kept| **kept > declaration)
                .expect("an out-of-order definition has an in-order neighbor");
            format!(
                "declares it before `{}`, so define it before that function",
                display_name(required[*next])
            )
        }
    }
}

fn required_declarations(header: &SemanticFacts) -> Vec<&MemberFunctionFact> {
    let header_definitions = header
        .member_functions
        .iter()
        .filter(|fact| fact.role == MemberFunctionRole::Definition)
        .collect::<Vec<_>>();
    let mut required: Vec<&MemberFunctionFact> = Vec::new();
    for declaration in header
        .member_functions
        .iter()
        .filter(|fact| fact.role == MemberFunctionRole::Declaration)
    {
        let defined_in_header = header_definitions.iter().any(|definition| {
            same_function(declaration, definition, true)
                || same_function(declaration, definition, false)
        });
        let duplicate = required.iter().any(|existing| {
            existing.owner == declaration.owner
                && existing.name == declaration.name
                && existing.signature == declaration.signature
        });
        if !defined_in_header && !duplicate {
            required.push(declaration);
        }
    }
    required
}

fn same_function(
    declaration: &MemberFunctionFact,
    definition: &MemberFunctionFact,
    exact: bool,
) -> bool {
    declaration.name == definition.name
        && declaration.signature == definition.signature
        && owner_matches(&declaration.owner, &definition.owner, exact)
}

/// With `exact` false, the definition owner can leave out leading namespaces,
/// as it can after `using namespace`.
fn owner_matches(declaration: &[String], definition: &[String], exact: bool) -> bool {
    if exact {
        declaration == definition
    } else {
        !definition.is_empty()
            && definition.len() < declaration.len()
            && declaration.ends_with(definition)
    }
}

fn display_name(fact: &MemberFunctionFact) -> String {
    let mut name = fact.owner.join("::");
    if !name.is_empty() {
        name.push_str("::");
    }
    name.push_str(&fact.name);
    name.push_str(&fact.signature);
    name
}

fn location(range: &TextRange) -> String {
    format!("{}:{}", range.path, range.start.line)
}

/// Marks one longest strictly increasing subsequence of `values`.
fn longest_increasing(values: &[usize]) -> Vec<bool> {
    let mut tails: Vec<usize> = Vec::new();
    let mut previous = vec![None; values.len()];
    for (index, value) in values.iter().enumerate() {
        let position = tails.partition_point(|tail| values[*tail] < *value);
        if position > 0 {
            previous[index] = Some(tails[position - 1]);
        }
        if position == tails.len() {
            tails.push(index);
        } else {
            tails[position] = index;
        }
    }
    let mut keep = vec![false; values.len()];
    let mut current = tails.last().copied();
    while let Some(index) = current {
        keep[index] = true;
        current = previous[index];
    }
    keep
}

#[cfg(test)]
mod tests;
