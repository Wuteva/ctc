use std::collections::{BTreeMap, BTreeSet};

use crate::{
    canonical::{CanonicalScalar, LINE_COUNT_FIELD},
    diagnostic::{Diagnostic, DiagnosticCategory},
    file_name::NameCase,
    language::LanguageAdapter,
};

use super::{CaptureCategory, Cardinality, FieldOperator, Filter, Placeholder, RegexPattern};

pub(super) fn build_filter(
    name: &str,
    arguments: Vec<serde_json::Value>,
) -> Result<Filter, String> {
    match name {
        "prefix" | "suffix" | "removePrefix" | "removeSuffix" => {
            build_string_filter(name, arguments)
        }
        "kind" => build_kind_filter(arguments),
        "field" => build_field_filter(arguments),
        "matches" | "notMatches" => build_regex_filter(name, arguments),
        "fileName" => build_file_name_filter(arguments),
        _ => Err(format!("Unknown placeholder filter `{name}`.")),
    }
}

fn build_string_filter(name: &str, arguments: Vec<serde_json::Value>) -> Result<Filter, String> {
    if arguments.len() != 1 {
        return Err(format!("The `{name}` filter requires one string."));
    }
    let value = arguments[0]
        .as_str()
        .ok_or_else(|| format!("The `{name}` filter requires one string."))?
        .to_string();
    Ok(match name {
        "prefix" => Filter::Prefix { value },
        "suffix" => Filter::Suffix { value },
        "removePrefix" => Filter::RemovePrefix { value },
        _ => Filter::RemoveSuffix { value },
    })
}

fn build_kind_filter(arguments: Vec<serde_json::Value>) -> Result<Filter, String> {
    if arguments.is_empty() || arguments.iter().any(|value| !value.is_string()) {
        return Err("The `kind` filter requires one or more strings.".to_string());
    }
    let mut values = Vec::with_capacity(arguments.len());
    for value in arguments {
        let Some(value) = value.as_str() else {
            return Err("The `kind` filter requires one or more strings.".to_string());
        };
        values.push(value.to_string());
    }
    Ok(Filter::Kind { values })
}

fn build_field_filter(arguments: Vec<serde_json::Value>) -> Result<Filter, String> {
    if !(2..=3).contains(&arguments.len()) {
        return Err("The `field` filter requires two or three arguments.".to_string());
    }
    let field_name = arguments[0]
        .as_str()
        .ok_or_else(|| "The canonical field name must be a string.".to_string())?
        .to_string();
    let operator_name = arguments[1]
        .as_str()
        .ok_or_else(|| "The field operator must be a string.".to_string())?;
    let operator = parse_field_operator(operator_name)?;
    validate_field_operator_arity(&operator, arguments.len())?;
    let value = arguments
        .into_iter()
        .nth(2)
        .map(|value| {
            CanonicalScalar::from_json(value)
                .ok_or_else(|| "A field value must be a JSON scalar.".to_string())
        })
        .transpose()?;
    if operator.is_ordering() && !matches!(value, Some(CanonicalScalar::Number(_))) {
        return Err(format!(
            "The `{operator:?}` field operator requires a number."
        ));
    }
    let pattern = if matches!(operator, FieldOperator::Matches | FieldOperator::NotMatches) {
        let Some(CanonicalScalar::String(source)) = &value else {
            return Err(format!(
                "The `{operator:?}` field operator requires a regular expression string."
            ));
        };
        Some(RegexPattern::new(source).map_err(|error| {
            format!("The `{operator:?}` field operator has an invalid regular expression: {error}")
        })?)
    } else {
        None
    };
    Ok(Filter::Field {
        name: field_name,
        operator,
        value,
        pattern,
    })
}

fn parse_field_operator(operator: &str) -> Result<FieldOperator, String> {
    match operator {
        "equal" => Ok(FieldOperator::Equal),
        "notEqual" => Ok(FieldOperator::NotEqual),
        "exists" => Ok(FieldOperator::Exists),
        "notExists" => Ok(FieldOperator::NotExists),
        "lessThan" => Ok(FieldOperator::LessThan),
        "lessThanOrEqual" => Ok(FieldOperator::LessThanOrEqual),
        "greaterThan" => Ok(FieldOperator::GreaterThan),
        "greaterThanOrEqual" => Ok(FieldOperator::GreaterThanOrEqual),
        "matches" => Ok(FieldOperator::Matches),
        "notMatches" => Ok(FieldOperator::NotMatches),
        _ => Err(format!("Unknown field operator `{operator}`.")),
    }
}

fn validate_field_operator_arity(
    operator: &FieldOperator,
    argument_len: usize,
) -> Result<(), String> {
    let expects_value = !matches!(operator, FieldOperator::Exists | FieldOperator::NotExists);
    if expects_value == (argument_len == 3) {
        return Ok(());
    }
    Err(match expects_value {
        true => format!("The `{operator:?}` field operator requires a value."),
        false => format!("The `{operator:?}` field operator does not accept a value."),
    })
}

fn build_regex_filter(name: &str, arguments: Vec<serde_json::Value>) -> Result<Filter, String> {
    let message = format!("The `{name}` filter requires one regular expression string.");
    let [argument] = arguments.as_slice() else {
        return Err(message);
    };
    let source = argument.as_str().ok_or(message)?;
    let pattern = RegexPattern::new(source).map_err(|error| {
        format!("The `{name}` filter has an invalid regular expression: {error}")
    })?;
    Ok(Filter::Regex {
        pattern,
        negate: name == "notMatches",
    })
}

fn build_file_name_filter(arguments: Vec<serde_json::Value>) -> Result<Filter, String> {
    let message = format!(
        "The `fileName` filter requires one of these strings: {}.",
        NameCase::NAMES.map(|name| format!("\"{name}\"")).join(", ")
    );
    let [argument] = arguments.as_slice() else {
        return Err(message);
    };
    let case = argument.as_str().and_then(NameCase::parse).ok_or(message)?;
    Ok(Filter::FileName { case })
}

pub(super) fn validate_placeholders(
    placeholders: &[Placeholder],
    adapter: &dyn LanguageAdapter,
) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let mut definitions: BTreeMap<&str, (CaptureCategory, Cardinality)> = BTreeMap::new();
    let mut plain_names = BTreeSet::new();
    let mut derived_names = Vec::new();

    for placeholder in placeholders {
        if placeholder.category == CaptureCategory::Keyword {
            validate_keyword_placeholder(placeholder, adapter, &mut diagnostics);
            continue;
        }
        validate_named_placeholder(placeholder, adapter, &mut diagnostics);
        if placeholder.is_derived() {
            derived_names.push(placeholder);
        } else {
            plain_names.insert(placeholder.name.as_str());
            record_definition(placeholder, &mut definitions, &mut diagnostics);
        }
    }
    for placeholder in derived_names {
        if !plain_names.contains(placeholder.name.as_str()) {
            diagnostics.push(template_diagnostic(
                "CTC2005",
                DiagnosticCategory::DerivedSourceMissing,
                format!(
                    "Derived capture `{}` has no plain capture in the template.",
                    placeholder.name
                ),
                placeholder,
            ));
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn validate_keyword_placeholder(
    placeholder: &Placeholder,
    adapter: &dyn LanguageAdapter,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if placeholder.cardinality != Cardinality::Optional {
        diagnostics.push(template_diagnostic(
            "CTC2001",
            DiagnosticCategory::InvalidTemplateSyntax,
            "A keyword placeholder must use optional cardinality.",
            placeholder,
        ));
    }
    if !adapter.known_keyword(&placeholder.name) {
        diagnostics.push(template_diagnostic(
            "CTC2001",
            DiagnosticCategory::InvalidTemplateSyntax,
            format!("Unknown language keyword `{}`.", placeholder.name),
            placeholder,
        ));
    }
    if !placeholder.filters.is_empty() {
        diagnostics.push(template_diagnostic(
            "CTC2008",
            DiagnosticCategory::InvalidFilter,
            "An optional keyword cannot use filters.",
            placeholder,
        ));
    }
}

fn validate_named_placeholder(
    placeholder: &Placeholder,
    adapter: &dyn LanguageAdapter,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let has_name_filter = placeholder.filters.iter().any(Filter::is_name_filter);
    let has_constraint = placeholder
        .filters
        .iter()
        .any(|filter| matches!(filter, Filter::Kind { .. } | Filter::Field { .. }));

    validate_filter_combinations(placeholder, has_name_filter, has_constraint, diagnostics);
    validate_file_name_filters(placeholder, diagnostics);
    validate_check_filter_order(placeholder, diagnostics);
    validate_known_filter_values(placeholder, adapter, diagnostics);
}

fn validate_filter_combinations(
    placeholder: &Placeholder,
    has_name_filter: bool,
    has_constraint: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if has_name_filter && has_constraint {
        diagnostics.push(template_diagnostic(
            "CTC2008",
            DiagnosticCategory::InvalidFilter,
            "Name filters and node constraints cannot occur in one placeholder.",
            placeholder,
        ));
    }
    if has_name_filter && placeholder.cardinality == Cardinality::Sequence {
        diagnostics.push(template_diagnostic(
            "CTC2008",
            DiagnosticCategory::InvalidFilter,
            "A sequence placeholder cannot use name filters.",
            placeholder,
        ));
    }
}

fn validate_file_name_filters(placeholder: &Placeholder, diagnostics: &mut Vec<Diagnostic>) {
    let file_name_filters = placeholder
        .filters
        .iter()
        .filter(|filter| matches!(filter, Filter::FileName { .. }))
        .count();
    if file_name_filters > 1 {
        diagnostics.push(template_diagnostic(
            "CTC2008",
            DiagnosticCategory::InvalidFilter,
            "A placeholder can use the `fileName` filter only once.",
            placeholder,
        ));
    } else if file_name_filters == 1
        && !matches!(placeholder.filters.last(), Some(Filter::FileName { .. }))
    {
        diagnostics.push(template_diagnostic(
            "CTC2008",
            DiagnosticCategory::InvalidFilter,
            "The `fileName` filter must be the last filter in a placeholder.",
            placeholder,
        ));
    }
}

fn validate_check_filter_order(placeholder: &Placeholder, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(first_check) = placeholder.filters.iter().position(Filter::is_check)
        && placeholder.filters[first_check..]
            .iter()
            .any(|filter| filter.is_name_filter() && !filter.is_check())
    {
        diagnostics.push(template_diagnostic(
            "CTC2008",
            DiagnosticCategory::InvalidFilter,
            "A check filter (`fileName`, `matches`, or `notMatches`) cannot be followed by a filter that changes the name.",
            placeholder,
        ));
    }
}

fn validate_known_filter_values(
    placeholder: &Placeholder,
    adapter: &dyn LanguageAdapter,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for filter in &placeholder.filters {
        match filter {
            Filter::Kind { values } => {
                validate_kind_values(values, placeholder, adapter, diagnostics)
            }
            Filter::Field { name, .. }
                if name != LINE_COUNT_FIELD && !adapter.known_field(name) =>
            {
                diagnostics.push(template_diagnostic(
                    "CTC2004",
                    DiagnosticCategory::InvalidCanonicalField,
                    format!("Unknown canonical field `{name}`."),
                    placeholder,
                ));
            }
            _ => {}
        }
    }
}

fn validate_kind_values(
    values: &[String],
    placeholder: &Placeholder,
    adapter: &dyn LanguageAdapter,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for value in values {
        if !adapter.known_kind(value) {
            diagnostics.push(template_diagnostic(
                "CTC2003",
                DiagnosticCategory::InvalidNodeKind,
                format!("Unknown canonical node kind `{value}`."),
                placeholder,
            ));
        }
    }
}

fn record_definition<'a>(
    placeholder: &'a Placeholder,
    definitions: &mut BTreeMap<&'a str, (CaptureCategory, Cardinality)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some((category, cardinality)) = definitions.get_mut(placeholder.name.as_str()) {
        let category_conflict = *category != CaptureCategory::Inferred
            && placeholder.category != CaptureCategory::Inferred
            && *category != placeholder.category;
        if category_conflict || *cardinality != placeholder.cardinality {
            diagnostics.push(template_diagnostic(
                "CTC2006",
                DiagnosticCategory::PlaceholderCategoryMismatch,
                format!(
                    "Capture `{}` uses incompatible categories or cardinalities.",
                    placeholder.name
                ),
                placeholder,
            ));
        }
        if *category == CaptureCategory::Inferred
            && placeholder.category != CaptureCategory::Inferred
        {
            *category = placeholder.category;
        }
    } else {
        definitions.insert(
            placeholder.name.as_str(),
            (placeholder.category, placeholder.cardinality),
        );
    }
}

fn template_diagnostic(
    code: &str,
    category: DiagnosticCategory,
    message: impl Into<String>,
    placeholder: &Placeholder,
) -> Diagnostic {
    Diagnostic::new(code, category, message, 2).with_template(placeholder.range.clone())
}
