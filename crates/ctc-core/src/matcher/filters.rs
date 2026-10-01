use std::{collections::BTreeMap, sync::Arc};

use crate::{
    canonical::{CanonicalNode, CanonicalScalar},
    file_name::{NameCase, file_name_and_stem},
    template::{FieldOperator, Filter, Placeholder, RegexPattern, TemplateNode},
};

use super::failure::{MatchFailure, category_failure};

pub(super) fn invert_name_filters(actual: &str, filters: &[Filter]) -> Option<String> {
    let mut value = actual.to_string();
    for filter in filters.iter().rev() {
        match filter {
            Filter::Prefix { value: prefix } => {
                value = value.strip_prefix(prefix)?.to_string();
            }
            Filter::Suffix { value: suffix } => {
                value = value.strip_suffix(suffix)?.to_string();
            }
            Filter::RemovePrefix { value: prefix } => value = format!("{prefix}{value}"),
            Filter::RemoveSuffix { value: suffix } => value.push_str(suffix),
            Filter::FileName { .. } | Filter::Regex { .. } => {}
            Filter::Kind { .. } | Filter::Field { .. } => return None,
        }
    }
    Some(value)
}

pub(super) fn collect_derived_filters(
    node: &TemplateNode,
    filters: &mut BTreeMap<String, Vec<Vec<Filter>>>,
) {
    match node {
        TemplateNode::Derived { placeholder } => {
            filters
                .entry(placeholder.name.clone())
                .or_default()
                .push(placeholder.filters.clone());
        }
        TemplateNode::Literal { children, .. } => {
            for child in children {
                collect_derived_filters(child, filters);
            }
        }
        TemplateNode::Optional { node, .. } => collect_derived_filters(node, filters),
        TemplateNode::Capture { .. } | TemplateNode::Sequence { .. } => {}
    }
}

pub(super) fn collect_identifiers(node: &CanonicalNode, identifiers: &mut Vec<String>) {
    if node.kind.as_ref() == "Identifier"
        && let Some(CanonicalScalar::String(value)) = &node.value
    {
        identifiers.push(value.to_string());
    }
    for child in &node.children {
        collect_identifiers(child, identifiers);
    }
}

pub(super) fn fields_match(
    expected: &BTreeMap<Arc<str>, CanonicalScalar>,
    actual: &BTreeMap<Arc<str>, CanonicalScalar>,
) -> bool {
    expected
        .iter()
        .all(|(name, value)| actual.get(name) == Some(value))
}

pub(super) fn constraints_match(node: &CanonicalNode, placeholder: &Placeholder) -> bool {
    placeholder.filters.iter().all(|filter| match filter {
        Filter::Kind { values } => values.iter().any(|value| value == node.kind.as_ref()),
        Filter::Field {
            name,
            operator,
            value,
            pattern,
        } => {
            let actual = node.field_value(name);
            field_operator_passes(operator, actual.as_ref(), value.as_ref(), pattern.as_ref())
        }
        Filter::Prefix { .. }
        | Filter::Suffix { .. }
        | Filter::RemovePrefix { .. }
        | Filter::RemoveSuffix { .. }
        | Filter::FileName { .. }
        | Filter::Regex { .. } => true,
    })
}

pub(super) fn rejected_filter(node: &CanonicalNode, placeholder: &Placeholder) -> Option<String> {
    let shown = |value: Option<&CanonicalScalar>| {
        value.map_or_else(
            || "missing".to_string(),
            |value| format!("`{}`", value.display()),
        )
    };
    placeholder.filters.iter().find_map(|filter| match filter {
        Filter::Kind { values } if !values.iter().any(|value| value == node.kind.as_ref()) => {
            let names = values
                .iter()
                .map(|value| format!("`{value}`"))
                .collect::<Vec<_>>();
            let needed = if names.len() == 1 {
                names[0].clone()
            } else {
                format!("one of {}", names.join(", "))
            };
            Some(format!("kind is `{}`, needs {needed}", node.kind))
        }
        Filter::Field {
            name,
            operator,
            value,
            pattern,
        } => {
            let actual = node.field_value(name);
            let actual = actual.as_ref();
            match operator {
                FieldOperator::Matches | FieldOperator::NotMatches
                    if !field_operator_passes(
                        operator,
                        actual,
                        value.as_ref(),
                        pattern.as_ref(),
                    ) =>
                {
                    let needs = if *operator == FieldOperator::Matches {
                        "must match"
                    } else {
                        "must not match"
                    };
                    Some(format!(
                        "field `{name}` is {}, {needs} {}",
                        shown(actual),
                        shown(value.as_ref())
                    ))
                }
                FieldOperator::Equal if actual != value.as_ref() => Some(format!(
                    "field `{name}` is {}, needs {}",
                    shown(actual),
                    shown(value.as_ref())
                )),
                FieldOperator::NotEqual if actual == value.as_ref() => Some(format!(
                    "field `{name}` is {}, must not be {}",
                    shown(actual),
                    shown(value.as_ref())
                )),
                FieldOperator::Exists if actual.is_none() => {
                    Some(format!("field `{name}` is missing, must exist"))
                }
                FieldOperator::NotExists if actual.is_some() => Some(format!(
                    "field `{name}` is {}, must be missing",
                    shown(actual)
                )),
                ordering
                    if ordering.is_ordering()
                        && !field_operator_passes(ordering, actual, value.as_ref(), None) =>
                {
                    let needs = match ordering {
                        FieldOperator::LessThan => "less than",
                        FieldOperator::LessThanOrEqual => "at most",
                        FieldOperator::GreaterThan => "more than",
                        _ => "at least",
                    };
                    Some(format!(
                        "field `{name}` is {}, needs {needs} {}",
                        shown(actual),
                        shown(value.as_ref())
                    ))
                }
                _ => None,
            }
        }
        _ => None,
    })
}

/// Applies a field operator. An ordering operator fails when the field is
/// missing or is not a number. `matches` needs an existing field whose text
/// matches the pattern, and `notMatches` passes when the field is missing.
fn field_operator_passes(
    operator: &FieldOperator,
    actual: Option<&CanonicalScalar>,
    value: Option<&CanonicalScalar>,
    pattern: Option<&RegexPattern>,
) -> bool {
    match operator {
        FieldOperator::Matches | FieldOperator::NotMatches => {
            let matched = actual
                .zip(pattern)
                .is_some_and(|(actual, pattern)| pattern.is_match(&actual.display()));
            matched == (*operator == FieldOperator::Matches)
        }
        FieldOperator::Equal => actual == value,
        FieldOperator::NotEqual => actual != value,
        FieldOperator::Exists => actual.is_some(),
        FieldOperator::NotExists => actual.is_none(),
        FieldOperator::LessThan
        | FieldOperator::LessThanOrEqual
        | FieldOperator::GreaterThan
        | FieldOperator::GreaterThanOrEqual => {
            let (Some(CanonicalScalar::Number(actual)), Some(CanonicalScalar::Number(value))) =
                (actual, value)
            else {
                return false;
            };
            let (Some(actual), Some(value)) = (actual.as_f64(), value.as_f64()) else {
                return false;
            };
            match operator {
                FieldOperator::LessThan => actual < value,
                FieldOperator::LessThanOrEqual => actual <= value,
                FieldOperator::GreaterThan => actual > value,
                _ => actual >= value,
            }
        }
    }
}

pub(super) fn apply_name_filters(base: &str, filters: &[Filter]) -> Option<String> {
    let mut value = base.to_string();
    for filter in filters {
        match filter {
            Filter::Prefix { value: prefix } => value = format!("{prefix}{value}"),
            Filter::Suffix { value: suffix } => value.push_str(suffix),
            Filter::RemovePrefix { value: prefix } => {
                value = value.strip_prefix(prefix.as_str())?.to_string();
            }
            Filter::RemoveSuffix { value: suffix } => {
                value = value.strip_suffix(suffix.as_str())?.to_string();
            }
            Filter::FileName { .. } | Filter::Regex { .. } => {}
            Filter::Kind { .. } | Filter::Field { .. } => return None,
        }
    }
    Some(value)
}

/// Checks the `matches` and `notMatches` filters of a capture. The name
/// filters before them run on the captured text first.
pub(super) fn check_regex_filters(
    placeholder: &Placeholder,
    source: &CanonicalNode,
    candidate_start: usize,
) -> Result<(), MatchFailure> {
    if !placeholder
        .filters
        .iter()
        .any(|filter| matches!(filter, Filter::Regex { .. }))
    {
        return Ok(());
    }
    let Some(CanonicalScalar::String(actual)) = &source.value else {
        return Err(category_failure(
            placeholder,
            Some(source),
            0,
            candidate_start,
        ));
    };
    let checked = apply_name_filters(actual, &placeholder.filters);
    for filter in &placeholder.filters {
        let Filter::Regex { pattern, negate } = filter else {
            continue;
        };
        let passes = checked
            .as_deref()
            .is_some_and(|text| pattern.is_match(text) != *negate);
        if passes {
            continue;
        }
        let requirement = if *negate { "not match" } else { "match" };
        return Err(MatchFailure {
            code: "CTC3010",
            category: crate::diagnostic::DiagnosticCategory::IdentifierPatternMismatch,
            message: format!(
                "Capture `{}` must {requirement} the pattern `{}`.",
                placeholder.name,
                pattern.source()
            ),
            template: placeholder.range.clone(),
            source: Some(source.range.clone()),
            expected: Some(format!("{requirement} {}", pattern.source())),
            actual: Some(checked.unwrap_or_else(|| actual.to_string())),
            matched_nodes: 0,
            source_offset: source.range.start.offset,
            exit_class: 1,
            candidate_start,
            trace: None,
        });
    }
    Ok(())
}

/// Checks a `fileName` capture. The filters before `fileName` run on the
/// captured text, and the result must equal the converted file stem. The file
/// name comes from the source path stored in the node range.
pub(super) fn check_file_name(
    placeholder: &Placeholder,
    case: NameCase,
    source: &CanonicalNode,
    candidate_start: usize,
) -> Result<(), MatchFailure> {
    let Some(CanonicalScalar::String(actual)) = &source.value else {
        return Err(category_failure(
            placeholder,
            Some(source),
            0,
            candidate_start,
        ));
    };
    let (file_name, stem) = file_name_and_stem(&source.range.path);
    let converted = case.convert(stem);
    if apply_name_filters(actual, &placeholder.filters).as_deref() == Some(converted.as_str()) {
        return Ok(());
    }
    let expected =
        invert_name_filters(&converted, &placeholder.filters).unwrap_or_else(|| converted.clone());
    let explanation = match case {
        NameCase::AsIs => format!("The file name without its extension is `{converted}`."),
        _ => format!("The file name in {} is `{converted}`.", case.name()),
    };
    Err(MatchFailure {
        code: "CTC3007",
        category: crate::diagnostic::DiagnosticCategory::FileNameMismatch,
        message: format!(
            "Capture `{}` must match the file name `{file_name}`. {explanation}",
            placeholder.name
        ),
        template: placeholder.range.clone(),
        source: Some(source.range.clone()),
        expected: Some(expected),
        actual: Some(actual.to_string()),
        matched_nodes: 0,
        source_offset: source.range.start.offset,
        exit_class: 1,
        candidate_start,
        trace: None,
    })
}
