use std::{collections::BTreeMap, sync::Arc};

use crate::{
    canonical::{CanonicalNode, CanonicalScalar},
    diagnostic::{TextPosition, TextRange},
    template::{CaptureCategory, Cardinality, Placeholder, TemplateNode},
};

use super::*;

fn range(offset: u64) -> TextRange {
    TextRange {
        path: "source.ts".to_string(),
        start: TextPosition {
            offset,
            line: 1,
            column: offset as u32 + 1,
        },
        end: TextPosition {
            offset: offset + 1,
            line: 1,
            column: offset as u32 + 2,
        },
    }
}

fn node(kind: &str, value: Option<&str>, offset: u64) -> CanonicalNode {
    CanonicalNode {
        kind: Arc::from(kind),
        value: value.map(|value| CanonicalScalar::String(Arc::from(value))),
        fields: BTreeMap::new(),
        children: Vec::new(),
        range: range(offset),
    }
}

fn placeholder(name: &str, offset: u64) -> Placeholder {
    Placeholder {
        name: name.to_string(),
        category: CaptureCategory::Inferred,
        cardinality: Cardinality::Scalar,
        filters: Vec::new(),
        range: range(offset),
        byte_range: 0..1,
    }
}

fn sequence(name: &str, offset: u64) -> Placeholder {
    Placeholder {
        cardinality: Cardinality::Sequence,
        ..placeholder(name, offset)
    }
}

#[test]
fn repeated_capture_requires_structural_equality() {
    let template = crate::template::CompiledTemplate {
        path: "rule.ts.ctmpl".to_string(),
        language_id: "test",
        mode: MatchMode::Exact,
        root: TemplateNode::Literal {
            kind: Arc::from("SourceFile"),
            value: None,
            fields: BTreeMap::new(),
            children: vec![
                TemplateNode::Capture {
                    placeholder: placeholder("Name", 0),
                },
                TemplateNode::Capture {
                    placeholder: placeholder("Name", 1),
                },
            ],
            range: range(0),
        },
    };
    let source = CanonicalNode {
        kind: Arc::from("SourceFile"),
        value: None,
        fields: BTreeMap::new(),
        children: vec![
            node("Identifier", Some("User"), 0),
            node("Identifier", Some("Other"), 1),
        ],
        range: range(0),
    };
    let result = match_template("rule", &template, &source, &|_| true);
    assert!(!result.matches);
    assert_eq!(result.diagnostics[0].code, "CTC3004");
}

#[test]
fn repeated_sequence_requires_the_same_node_list() {
    let template = crate::template::CompiledTemplate {
        path: "rule.ts.ctmpl".to_string(),
        language_id: "test",
        mode: MatchMode::Exact,
        root: TemplateNode::Literal {
            kind: Arc::from("SourceFile"),
            value: None,
            fields: BTreeMap::new(),
            children: vec![
                TemplateNode::Sequence {
                    placeholder: sequence("Items", 0),
                },
                TemplateNode::Literal {
                    kind: Arc::from("Token"),
                    value: Some(CanonicalScalar::String(Arc::from("marker"))),
                    fields: BTreeMap::new(),
                    children: Vec::new(),
                    range: range(1),
                },
                TemplateNode::Sequence {
                    placeholder: sequence("Items", 2),
                },
            ],
            range: range(0),
        },
    };
    let valid = CanonicalNode {
        kind: Arc::from("SourceFile"),
        value: None,
        fields: BTreeMap::new(),
        children: vec![
            node("Identifier", Some("A"), 0),
            node("Token", Some("marker"), 1),
            node("Identifier", Some("A"), 2),
        ],
        range: range(0),
    };
    assert!(match_template("rule", &template, &valid, &|_| true).matches);

    let invalid = CanonicalNode {
        children: vec![
            node("Identifier", Some("A"), 0),
            node("Token", Some("marker"), 1),
            node("Identifier", Some("B"), 2),
        ],
        ..valid
    };
    assert!(!match_template("rule", &template, &invalid, &|_| true).matches);
}

fn literal(kind: &str, value: Option<&str>, children: Vec<TemplateNode>) -> TemplateNode {
    TemplateNode::Literal {
        kind: Arc::from(kind),
        value: value.map(|value| CanonicalScalar::String(Arc::from(value))),
        fields: BTreeMap::new(),
        children,
        range: range(0),
    }
}

fn parent(kind: &str, offset: u64, children: Vec<CanonicalNode>) -> CanonicalNode {
    CanonicalNode {
        children,
        ..node(kind, None, offset)
    }
}

fn every_template(pattern: TemplateNode) -> crate::template::CompiledTemplate {
    crate::template::CompiledTemplate {
        path: "rule.hpp.ctmpl".to_string(),
        language_id: "test",
        mode: MatchMode::Every,
        root: literal("SourceFile", None, vec![pattern]),
    }
}

fn every_offsets(
    template: &crate::template::CompiledTemplate,
    source: &CanonicalNode,
    kinds: &[&str],
) -> Vec<u64> {
    let kinds = kinds
        .iter()
        .map(|kind| kind.to_string())
        .collect::<Vec<_>>();
    let result = match_template_in_scope_with_kinds(
        "rule",
        template,
        source,
        SearchScope::TopLevel,
        &kinds,
        &|_| true,
    );
    assert_eq!(result.matches, result.diagnostics.is_empty());
    result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.source.as_ref().unwrap().start.offset)
        .collect()
}

#[test]
fn every_class_template_accepts_listed_kinds_and_loose_headers() {
    let template = every_template(literal(
        "ClassDeclaration",
        None,
        vec![
            literal("Token", Some("class"), Vec::new()),
            TemplateNode::Capture {
                placeholder: placeholder("Name", 0),
            },
            literal("ClassBody", None, Vec::new()),
        ],
    ));
    let source = parent(
        "SourceFile",
        0,
        vec![
            parent(
                "StructDeclaration",
                10,
                vec![
                    node("Token", Some("struct"), 11),
                    node("Identifier", Some("Point"), 12),
                    node("BaseClassClause", None, 13),
                    parent("ClassBody", 14, Vec::new()),
                ],
            ),
            parent(
                "ClassDeclaration",
                20,
                vec![
                    node("Token", Some("class"), 21),
                    node("Identifier", Some("Forward"), 22),
                ],
            ),
            parent(
                "StructDeclaration",
                30,
                vec![
                    node("Token", Some("struct"), 31),
                    node("Identifier", Some("Filled"), 32),
                    parent("ClassBody", 33, vec![node("FieldDeclaration", None, 34)]),
                ],
            ),
            parent(
                "StructDeclaration",
                40,
                vec![
                    node("Token", Some("struct"), 41),
                    parent("ClassBody", 42, Vec::new()),
                ],
            ),
        ],
    );

    assert!(every_offsets(&template, &source, &[]).is_empty());
    assert_eq!(
        every_offsets(
            &template,
            &source,
            &["ClassDeclaration", "StructDeclaration"]
        ),
        [34, 40]
    );
}

#[test]
fn every_non_class_template_compares_children_of_listed_kinds() {
    let template = every_template(literal(
        "EnumDeclaration",
        None,
        vec![literal("Identifier", Some("Color"), Vec::new())],
    ));
    let source = parent(
        "SourceFile",
        0,
        vec![
            parent(
                "EnumDeclaration",
                10,
                vec![node("Identifier", Some("Color"), 11)],
            ),
            parent(
                "UnionDeclaration",
                20,
                vec![node("Identifier", Some("Color"), 21)],
            ),
            parent(
                "UnionDeclaration",
                30,
                vec![
                    node("Identifier", Some("Color"), 31),
                    node("Token", Some("extra"), 32),
                ],
            ),
        ],
    );

    assert!(every_offsets(&template, &source, &[]).is_empty());
    assert_eq!(
        every_offsets(&template, &source, &["EnumDeclaration", "UnionDeclaration"]),
        [32]
    );
}
