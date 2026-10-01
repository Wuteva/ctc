use std::{collections::BTreeMap, sync::Arc};

use crate::{
    canonical::{CanonicalNode, CanonicalScalar},
    diagnostic::{TextPosition, TextRange},
    matcher::{
        MatchMode, SearchScope, TraceStepKind, explain_template_in_scope, match_template_in_scope,
    },
    template::{
        CaptureCategory, Cardinality, CompiledTemplate, FieldOperator, Filter, Placeholder,
        TemplateNode,
    },
};

use super::snippet;

fn range(line: u32) -> TextRange {
    let offset = u64::from(line) * 10;
    TextRange {
        path: "source.ts".to_string(),
        start: TextPosition {
            offset,
            line,
            column: 1,
        },
        end: TextPosition {
            offset: offset + 1,
            line,
            column: 2,
        },
    }
}

fn text(value: &str) -> CanonicalScalar {
    CanonicalScalar::String(Arc::from(value))
}

fn member(line: u32, access: &str, constructor: bool) -> CanonicalNode {
    CanonicalNode {
        kind: Arc::from("MethodDefinition"),
        value: None,
        fields: BTreeMap::from([
            (Arc::from("access"), text(access)),
            (Arc::from("constructor"), CanonicalScalar::Bool(constructor)),
        ]),
        children: Vec::new(),
        range: range(line),
    }
}

fn group(name: &str, line: u32, field: &str, value: CanonicalScalar) -> TemplateNode {
    TemplateNode::Sequence {
        placeholder: Placeholder {
            name: name.to_string(),
            category: CaptureCategory::Inferred,
            cardinality: Cardinality::Sequence,
            filters: vec![Filter::Field {
                pattern: None,
                name: field.to_string(),
                operator: FieldOperator::Equal,
                value: Some(value),
            }],
            range: range(line),
            byte_range: 0..1,
        },
    }
}

fn literal(kind: &str, line: u32, children: Vec<TemplateNode>) -> TemplateNode {
    TemplateNode::Literal {
        kind: Arc::from(kind),
        value: None,
        fields: BTreeMap::new(),
        children,
        range: range(line),
    }
}

fn class_template(mode: MatchMode) -> CompiledTemplate {
    CompiledTemplate {
        path: "rule.ts.ctmpl".to_string(),
        language_id: "test",
        mode,
        root: literal(
            "SourceFile",
            1,
            vec![literal(
                "ClassBody",
                1,
                vec![
                    group(
                        "Constructors",
                        2,
                        "constructor",
                        CanonicalScalar::Bool(true),
                    ),
                    group("PublicMembers", 3, "access", text("public")),
                    group("PrivateMembers", 4, "access", text("private")),
                ],
            )],
        ),
    }
}

fn body(line: u32, members: Vec<CanonicalNode>) -> CanonicalNode {
    CanonicalNode {
        kind: Arc::from("ClassBody"),
        value: None,
        fields: BTreeMap::new(),
        children: members,
        range: range(line),
    }
}

fn file(bodies: Vec<CanonicalNode>) -> CanonicalNode {
    CanonicalNode {
        kind: Arc::from("SourceFile"),
        value: None,
        fields: BTreeMap::new(),
        children: bodies,
        range: range(1),
    }
}

fn members_of(steps: &[crate::matcher::TraceStep], name: &str) -> Vec<u32> {
    steps
        .iter()
        .find(|step| step.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no step for {name}: {steps:?}"))
        .nodes
        .iter()
        .map(|node| node.range.start.line)
        .collect()
}

#[test]
fn records_the_members_of_each_group_on_success() {
    let template = class_template(MatchMode::Exact);
    let source = file(vec![body(
        1,
        vec![
            member(2, "public", true),
            member(3, "public", false),
            member(4, "public", false),
            member(5, "private", false),
        ],
    )]);
    let explanation = explain_template_in_scope(
        "rule",
        &template,
        &source,
        SearchScope::TopLevel,
        &[],
        &|_| true,
    );
    assert!(explanation.result.matches);
    assert_eq!(explanation.candidates.len(), 1);
    let candidate = &explanation.candidates[0];
    assert!(candidate.template_matched);
    assert!(candidate.failure.is_none());
    let steps = &candidate.steps;
    assert_eq!(steps[0].kind, TraceStepKind::Literal);
    assert_eq!(steps[0].template_kind.as_deref(), Some("ClassBody"));
    assert_eq!(steps[0].depth, 0);
    assert_eq!(members_of(steps, "Constructors"), [2]);
    assert_eq!(members_of(steps, "PublicMembers"), [3, 4]);
    assert_eq!(members_of(steps, "PrivateMembers"), [5]);
    assert!(
        steps
            .iter()
            .filter(|step| step.kind == TraceStepKind::Sequence)
            .all(|step| step.depth == 1)
    );
}

#[test]
fn explains_a_member_that_belongs_to_an_earlier_group() {
    let template = class_template(MatchMode::Exact);
    let source = file(vec![body(
        1,
        vec![member(2, "private", false), member(3, "public", false)],
    )]);
    let explanation = explain_template_in_scope(
        "rule",
        &template,
        &source,
        SearchScope::TopLevel,
        &[],
        &|_| true,
    );
    assert!(!explanation.result.matches);
    let candidate = &explanation.candidates[0];
    assert!(!candidate.template_matched);
    assert_eq!(members_of(&candidate.steps, "PrivateMembers"), [2]);
    let failure = candidate.failure.as_ref().unwrap();
    assert_eq!(failure.diagnostic.code, "CTC3003");
    assert_eq!(
        failure.diagnostic.source.as_ref().unwrap().start.line,
        3,
        "{failure:?}"
    );
    assert_eq!(
        failure.reason.as_deref(),
        Some(
            "The node at line 3 (MethodDefinition) passes group `PublicMembers`, but the later group `PrivateMembers` already started at line 2."
        )
    );
}

#[test]
fn explains_which_filter_rejects_a_member_that_fits_no_group() {
    let template = class_template(MatchMode::Exact);
    let source = file(vec![body(1, vec![member(2, "protected", false)])]);
    let explanation = explain_template_in_scope(
        "rule",
        &template,
        &source,
        SearchScope::TopLevel,
        &[],
        &|_| true,
    );
    let failure = explanation.candidates[0].failure.as_ref().unwrap();
    assert_eq!(
        failure.reason.as_deref(),
        Some(
            "The node at line 2 (MethodDefinition) passes no group. `Constructors`: field `constructor` is `false`, needs `true`. `PublicMembers`: field `access` is `protected`, needs `public`. `PrivateMembers`: field `access` is `protected`, needs `private`."
        )
    );
}

#[test]
fn every_mode_explains_each_candidate() {
    let template = class_template(MatchMode::Every);
    let source = file(vec![
        body(1, vec![member(2, "public", false)]),
        body(
            5,
            vec![member(6, "private", false), member(7, "public", false)],
        ),
        body(9, Vec::new()),
    ]);
    let explanation = explain_template_in_scope(
        "rule",
        &template,
        &source,
        SearchScope::TopLevel,
        &[],
        &|_| true,
    );
    let found = explanation
        .candidates
        .iter()
        .map(|candidate| (candidate.source.start.line, candidate.template_matched))
        .collect::<Vec<_>>();
    assert_eq!(found, [(1, true), (5, false), (9, true)]);
}

#[test]
fn explanation_keeps_the_normal_match_result() {
    for mode in [MatchMode::Exact, MatchMode::Every, MatchMode::Contains] {
        let template = class_template(mode);
        for members in [
            vec![member(2, "public", false)],
            vec![member(2, "private", false), member(3, "public", false)],
            vec![member(2, "protected", false)],
        ] {
            let source = file(vec![body(1, members)]);
            let plain =
                match_template_in_scope("rule", &template, &source, SearchScope::TopLevel, &|_| {
                    true
                });
            let explained = explain_template_in_scope(
                "rule",
                &template,
                &source,
                SearchScope::TopLevel,
                &[],
                &|_| true,
            )
            .result;
            assert_eq!(plain.matches, explained.matches);
            let key = |result: &crate::matcher::MatchResult| {
                result
                    .diagnostics
                    .iter()
                    .map(|diagnostic| {
                        (
                            diagnostic.code.clone(),
                            diagnostic.source.clone(),
                            diagnostic.template.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(key(&plain), key(&explained));
        }
    }
}

#[test]
fn snippet_uses_the_first_line_and_a_short_length() {
    let source = format!("class A {{\n  {}\n}}", "x".repeat(80));
    let whole = TextRange {
        path: "a.ts".to_string(),
        start: TextPosition {
            offset: 0,
            line: 1,
            column: 1,
        },
        end: TextPosition {
            offset: source.len() as u64,
            line: 3,
            column: 2,
        },
    };
    assert_eq!(snippet(&source, &whole), "class A {");
    let long = TextRange {
        start: TextPosition {
            offset: 12,
            line: 2,
            column: 3,
        },
        ..whole
    };
    let cut = snippet(&source, &long);
    assert!(cut.ends_with("...") && cut.chars().count() == 60, "{cut}");
}
