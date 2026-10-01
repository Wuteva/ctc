use crate::{
    canonical::{CanonicalNode, CanonicalScalar},
    diagnostic::TextRange,
    template::{CaptureCategory, Placeholder, TemplateNode},
};

use super::{
    capture::{CaptureEnvironment, CaptureValue, MatchContext, MatchSuccess},
    failure::{MatchFailure, capture_failure, category_failure, derived_failure, literal_failure},
    filters::{
        apply_name_filters, check_file_name, check_regex_filters, constraints_match, fields_match,
    },
    list::match_list,
    trace::{TraceNode, TraceStep, TraceStepKind, placeholder_step},
};

pub(super) fn match_node(
    template: &TemplateNode,
    source: &CanonicalNode,
    environment: CaptureEnvironment,
    validate_identifier: &dyn Fn(&str) -> bool,
    context: &mut MatchContext,
) -> Result<MatchSuccess, MatchFailure> {
    if !context.tracing {
        return match_node_inner(template, source, environment, validate_identifier, context);
    }
    let trace = environment.trace.clone();
    match_node_inner(template, source, environment, validate_identifier, context)
        .map_err(|failure| failure.with_trace(trace))
}

fn match_node_inner(
    template: &TemplateNode,
    source: &CanonicalNode,
    environment: CaptureEnvironment,
    validate_identifier: &dyn Fn(&str) -> bool,
    context: &mut MatchContext,
) -> Result<MatchSuccess, MatchFailure> {
    match template {
        TemplateNode::Literal {
            kind,
            value,
            fields,
            children,
            range,
        } => match_literal_node(
            LiteralTemplate {
                kind,
                value,
                fields,
                children,
                range,
                template,
            },
            source,
            environment,
            validate_identifier,
            context,
        ),
        TemplateNode::Capture { placeholder } => match_capture_node(
            placeholder,
            source,
            environment,
            validate_identifier,
            context,
        ),
        TemplateNode::Derived { placeholder } => {
            match_derived_node(placeholder, source, environment, context)
        }
        TemplateNode::Sequence { placeholder } => Err(category_failure(
            placeholder,
            Some(source),
            0,
            context.candidate_start,
        )),
        TemplateNode::Optional { .. } => Err(optional_scalar_failure(template, source, context)),
    }
}

struct LiteralTemplate<'a> {
    kind: &'a std::sync::Arc<str>,
    value: &'a Option<CanonicalScalar>,
    fields: &'a std::collections::BTreeMap<std::sync::Arc<str>, CanonicalScalar>,
    children: &'a [TemplateNode],
    range: &'a TextRange,
    template: &'a TemplateNode,
}

fn match_literal_node(
    literal: LiteralTemplate<'_>,
    source: &CanonicalNode,
    mut environment: CaptureEnvironment,
    validate_identifier: &dyn Fn(&str) -> bool,
    context: &mut MatchContext,
) -> Result<MatchSuccess, MatchFailure> {
    if source.kind != *literal.kind
        || source.value != *literal.value
        || !fields_match(literal.fields, &source.fields)
    {
        return Err(literal_failure(
            literal.template,
            source,
            0,
            context.candidate_start,
        ));
    }
    if context.tracing {
        environment.push_step(TraceStep {
            depth: context.depth,
            kind: TraceStepKind::Literal,
            name: None,
            template_kind: Some(literal.kind.to_string()),
            template: literal.range.clone(),
            nodes: vec![TraceNode::from_node(source)],
        });
    }
    context.depth += 1;
    let result = match_list(
        literal.children,
        &source.children,
        environment,
        true,
        literal.range,
        &source.range,
        validate_identifier,
        context,
    );
    context.depth -= 1;
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

fn match_capture_node(
    placeholder: &Placeholder,
    source: &CanonicalNode,
    mut environment: CaptureEnvironment,
    validate_identifier: &dyn Fn(&str) -> bool,
    context: &mut MatchContext,
) -> Result<MatchSuccess, MatchFailure> {
    ensure_capture_category(placeholder, source, context.candidate_start)?;
    if !constraints_match(source, placeholder) {
        return Err(capture_failure(
            placeholder,
            source,
            0,
            context.candidate_start,
        ));
    }
    if let Some(case) = placeholder.file_name_case() {
        check_file_name(placeholder, case, source, context.candidate_start)?;
    }
    check_regex_filters(placeholder, source, context.candidate_start)?;
    bind_capture(
        placeholder,
        source,
        &mut environment,
        validate_identifier,
        context.candidate_start,
    )?;
    if context.tracing {
        environment.push_step(placeholder_step(
            TraceStepKind::Capture,
            placeholder,
            source,
            context.depth,
        ));
    }
    Ok(MatchSuccess {
        environment,
        matched_nodes: 1,
        consumed_nodes: 1,
    })
}

fn ensure_capture_category(
    placeholder: &Placeholder,
    source: &CanonicalNode,
    candidate_start: usize,
) -> Result<(), MatchFailure> {
    if placeholder.category == CaptureCategory::Identifier && source.kind.as_ref() != "Identifier" {
        return Err(category_failure(
            placeholder,
            Some(source),
            0,
            candidate_start,
        ));
    }
    Ok(())
}

fn bind_capture(
    placeholder: &Placeholder,
    source: &CanonicalNode,
    environment: &mut CaptureEnvironment,
    validate_identifier: &dyn Fn(&str) -> bool,
    candidate_start: usize,
) -> Result<(), MatchFailure> {
    if let Some(existing) = environment.values.get(&placeholder.name) {
        let CaptureValue::Scalar(existing) = existing else {
            return Err(category_failure(
                placeholder,
                Some(source),
                0,
                candidate_start,
            ));
        };
        if !existing.structurally_eq(source) {
            return Err(capture_failure(placeholder, source, 0, candidate_start));
        }
        return Ok(());
    }

    resolve_pending_derived(
        placeholder,
        source,
        environment,
        validate_identifier,
        candidate_start,
    )?;
    environment.values.insert(
        placeholder.name.clone(),
        CaptureValue::Scalar(source.clone()),
    );
    Ok(())
}

fn resolve_pending_derived(
    placeholder: &Placeholder,
    source: &CanonicalNode,
    environment: &mut CaptureEnvironment,
    validate_identifier: &dyn Fn(&str) -> bool,
    candidate_start: usize,
) -> Result<(), MatchFailure> {
    let Some(pending) = environment.pending_derived.remove(&placeholder.name) else {
        return Ok(());
    };
    let Some(CanonicalScalar::String(base)) = &source.value else {
        return Err(category_failure(
            placeholder,
            Some(source),
            0,
            candidate_start,
        ));
    };
    if !validate_identifier(base)
        && let Some(first) = pending.first()
    {
        return Err(derived_failure(
            &first.placeholder,
            &first.source,
            "a valid base identifier".to_string(),
            base.to_string(),
            candidate_start,
        ));
    }
    for constraint in pending {
        let expected =
            apply_name_filters(base, &constraint.placeholder.filters).unwrap_or_default();
        if expected != constraint.actual {
            return Err(derived_failure(
                &constraint.placeholder,
                &constraint.source,
                expected,
                constraint.actual,
                candidate_start,
            ));
        }
    }
    Ok(())
}

fn match_derived_node(
    placeholder: &Placeholder,
    source: &CanonicalNode,
    mut environment: CaptureEnvironment,
    context: &mut MatchContext,
) -> Result<MatchSuccess, MatchFailure> {
    let actual = identifier_text(placeholder, source, context.candidate_start)?;
    if let Some(existing) = environment.values.get(&placeholder.name) {
        match_existing_derived(
            placeholder,
            source,
            actual,
            existing,
            context.candidate_start,
        )?;
    } else {
        bind_derived_value(placeholder, source, actual, &mut environment, context)?;
    }
    if context.tracing {
        environment.push_step(placeholder_step(
            TraceStepKind::Derived,
            placeholder,
            source,
            context.depth,
        ));
    }
    Ok(MatchSuccess {
        environment,
        matched_nodes: 1,
        consumed_nodes: 1,
    })
}

fn identifier_text<'a>(
    placeholder: &Placeholder,
    source: &'a CanonicalNode,
    candidate_start: usize,
) -> Result<&'a str, MatchFailure> {
    let Some(CanonicalScalar::String(actual)) = &source.value else {
        return Err(category_failure(
            placeholder,
            Some(source),
            0,
            candidate_start,
        ));
    };
    if source.kind.as_ref() != "Identifier" {
        return Err(category_failure(
            placeholder,
            Some(source),
            0,
            candidate_start,
        ));
    }
    Ok(actual)
}

fn match_existing_derived(
    placeholder: &Placeholder,
    source: &CanonicalNode,
    actual: &str,
    existing: &CaptureValue,
    candidate_start: usize,
) -> Result<(), MatchFailure> {
    let CaptureValue::Scalar(base) = existing else {
        return Err(category_failure(
            placeholder,
            Some(source),
            0,
            candidate_start,
        ));
    };
    let Some(CanonicalScalar::String(base)) = &base.value else {
        return Err(category_failure(
            placeholder,
            Some(source),
            0,
            candidate_start,
        ));
    };
    let expected = apply_name_filters(base, &placeholder.filters);
    if expected.as_deref() == Some(actual) {
        return Ok(());
    }
    Err(derived_failure(
        placeholder,
        source,
        expected.unwrap_or_default(),
        actual.to_string(),
        candidate_start,
    ))
}

fn bind_derived_value(
    placeholder: &Placeholder,
    source: &CanonicalNode,
    actual: &str,
    environment: &mut CaptureEnvironment,
    context: &MatchContext,
) -> Result<(), MatchFailure> {
    if let Some(base) = context
        .derived_hints
        .for_other_filters(&placeholder.name, &placeholder.filters)
    {
        let expected = apply_name_filters(&base, &placeholder.filters).unwrap_or_default();
        if expected != actual {
            return Err(derived_failure(
                placeholder,
                source,
                expected,
                actual.to_string(),
                context.candidate_start,
            ));
        }
        let mut inferred = source.clone();
        inferred.value = Some(CanonicalScalar::String(base.into()));
        environment
            .values
            .insert(placeholder.name.clone(), CaptureValue::Scalar(inferred));
        return Ok(());
    }

    environment
        .pending_derived
        .entry(placeholder.name.clone())
        .or_default()
        .push(super::capture::PendingDerived {
            actual: actual.to_string(),
            source: source.clone(),
            placeholder: placeholder.clone(),
        });
    Ok(())
}

fn optional_scalar_failure(
    template: &TemplateNode,
    source: &CanonicalNode,
    context: &MatchContext,
) -> MatchFailure {
    MatchFailure {
        code: "CTC9001",
        category: crate::diagnostic::DiagnosticCategory::InternalToolError,
        message: "An optional template node reached scalar matching.".to_string(),
        template: template.range().clone(),
        source: Some(source.range.clone()),
        expected: None,
        actual: None,
        matched_nodes: 0,
        source_offset: source.range.start.offset,
        exit_class: 2,
        candidate_start: context.candidate_start,
        trace: None,
    }
}
