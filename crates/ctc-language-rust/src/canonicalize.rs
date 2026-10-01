use std::{collections::BTreeMap, path::Path, sync::Arc};

use ctc_core::{
    canonical::{CanonicalNode, CanonicalScalar, SemanticFacts},
    diagnostic::{Diagnostic, DiagnosticCategory, LineIndex, TextRange},
    engine::ParsedSource,
};
use tree_sitter::{Language, Node, Parser};

use crate::members;

const VALUE_NODES: [&str; 5] = [
    "char_literal",
    "float_literal",
    "integer_literal",
    "raw_string_literal",
    "string_literal",
];

pub fn parse_source(source: &str, path: &Path) -> Result<ParsedSource, Vec<Diagnostic>> {
    let tree = parse_tree(source).map_err(internal_error)?;
    let errors = error_nodes(tree.root_node());
    if !errors.is_empty() {
        return Err(errors
            .into_iter()
            .map(|node| {
                Diagnostic::new(
                    "CTC3001",
                    DiagnosticCategory::SourceParseError,
                    "The source contains invalid Rust syntax.",
                    1,
                )
                .with_source(TextRange::from_offsets(
                    path,
                    source,
                    node.start_byte(),
                    node.end_byte(),
                ))
            })
            .collect());
    }
    let lines = LineIndex::new(path, source);
    let Some(root) = canonicalize_indexed(tree.root_node(), source, &lines) else {
        return Err(vec![Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            "The Rust adapter removed the source-file root.",
            2,
        )]);
    };
    Ok(ParsedSource {
        path: path.to_path_buf(),
        language_id: "rust",
        source: source.to_string(),
        root,
        semantic_facts: SemanticFacts::default(),
        comments: comment_ranges(tree.root_node()),
    })
}

pub(crate) fn parse_tree(source: &str) -> Result<tree_sitter::Tree, String> {
    let mut parser = Parser::new();
    let language: Language = tree_sitter_rust::LANGUAGE.into();
    parser
        .set_language(&language)
        .map_err(|error| format!("Cannot load the Rust grammar: {error}"))?;
    let parser_source = normalize_parser_input(source);
    parser
        .parse(parser_source.as_ref(), None)
        .ok_or_else(|| "The Rust parser did not return a syntax tree.".to_string())
}

pub(crate) fn error_nodes(root: Node<'_>) -> Vec<Node<'_>> {
    fn collect<'tree>(node: Node<'tree>, errors: &mut Vec<Node<'tree>>) {
        if node.is_error() || node.is_missing() {
            errors.push(node);
        }
        for index in 0..node.child_count() {
            if let Some(child) = node.child(index) {
                collect(child, errors);
            }
        }
    }

    let mut errors = Vec::new();
    collect(root, &mut errors);
    errors.sort_by_key(|node| (node.start_byte(), node.end_byte(), node.kind().to_string()));
    errors.dedup_by_key(|node| (node.start_byte(), node.end_byte(), node.kind().to_string()));
    errors
}

pub(crate) fn canonicalize_node(
    node: Node<'_>,
    source: &str,
    path: &Path,
) -> Option<CanonicalNode> {
    canonicalize_indexed(node, source, &LineIndex::new(path, source))
}

pub(crate) fn canonical_kind(node: Node<'_>) -> String {
    if !node.is_named() {
        return "Token".to_string();
    }
    canonical_kind_from_raw(node.kind())
}

pub fn known_kind(value: &str) -> bool {
    let language: Language = tree_sitter_rust::LANGUAGE.into();
    (0..language.node_kind_count()).any(|id| {
        language.node_kind_is_named(id as u16)
            && language
                .node_kind_for_id(id as u16)
                .is_some_and(|kind| canonical_kind_from_raw(kind) == value)
    })
}

fn internal_error(message: String) -> Vec<Diagnostic> {
    vec![Diagnostic::new(
        "CTC9001",
        DiagnosticCategory::InternalToolError,
        message,
        2,
    )]
}

fn normalize_parser_input(source: &str) -> std::borrow::Cow<'_, str> {
    let Some(source_without_bom) = source.strip_prefix('\u{feff}') else {
        return std::borrow::Cow::Borrowed(source);
    };
    let mut normalized = String::with_capacity(source.len());
    normalized.push_str("   ");
    normalized.push_str(source_without_bom);
    std::borrow::Cow::Owned(normalized)
}

fn canonicalize_indexed(
    node: Node<'_>,
    source: &str,
    lines: &LineIndex<'_>,
) -> Option<CanonicalNode> {
    if should_skip(node) {
        return None;
    }
    if VALUE_NODES.contains(&node.kind()) {
        return Some(value_node(node, source, lines));
    }

    let mut children = Vec::new();
    for index in 0..node.child_count() {
        if let Some(child) = node
            .child(index)
            .and_then(|child| canonicalize_indexed(child, source, lines))
        {
            children.push(child);
        }
    }
    let text = node.utf8_text(source.as_bytes()).unwrap_or_default();
    Some(CanonicalNode {
        kind: Arc::from(canonical_kind(node)),
        value: canonical_value(node, text, children.is_empty() && node.child_count() == 0),
        fields: members::canonical_fields(node, source),
        children,
        range: lines.range(node.start_byte(), node.end_byte()),
    })
}

fn value_node(node: Node<'_>, source: &str, lines: &LineIndex<'_>) -> CanonicalNode {
    let text = node.utf8_text(source.as_bytes()).unwrap_or_default();
    CanonicalNode {
        kind: Arc::from(canonical_kind(node)),
        value: Some(CanonicalScalar::String(Arc::from(canonical_literal(
            node, text,
        )))),
        fields: BTreeMap::new(),
        children: Vec::new(),
        range: lines.range(node.start_byte(), node.end_byte()),
    }
}

fn canonical_kind_from_raw(kind: &str) -> String {
    match kind {
        "source_file" => "SourceFile".to_string(),
        "function_item" => "FunctionDeclaration".to_string(),
        "closure_expression" => "ArrowFunction".to_string(),
        "block" => "StatementBlock".to_string(),
        "declaration_list" | "field_declaration_list" | "enum_variant_list" => {
            "ClassBody".to_string()
        }
        "struct_item" => "StructDeclaration".to_string(),
        "enum_item" => "EnumDeclaration".to_string(),
        "trait_item" => "TraitDeclaration".to_string(),
        "impl_item" => "ImplBlock".to_string(),
        "mod_item" => "ModuleDeclaration".to_string(),
        "use_declaration" => "UseDeclaration".to_string(),
        "identifier" | "type_identifier" | "field_identifier" | "shorthand_field_identifier" => {
            "Identifier".to_string()
        }
        "raw_string_literal" | "string_literal" => "StringLiteral".to_string(),
        "integer_literal" | "float_literal" => "NumericLiteral".to_string(),
        "char_literal" => "CharacterLiteral".to_string(),
        other => pascal_case(other),
    }
}

fn canonical_value(node: Node<'_>, text: &str, has_no_children: bool) -> Option<CanonicalScalar> {
    if !has_no_children {
        return None;
    }
    Some(CanonicalScalar::String(Arc::from(canonical_literal(
        node, text,
    ))))
}

fn canonical_literal(node: Node<'_>, text: &str) -> String {
    if matches!(node.kind(), "integer_literal" | "float_literal") {
        text.replace('_', "")
    } else {
        normalize_line_endings(text)
    }
}

fn comment_ranges(root: Node<'_>) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if matches!(node.kind(), "line_comment" | "block_comment") {
            ranges.push(node.start_byte()..node.end_byte());
        } else if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return ranges;
            }
        }
    }
}

fn should_skip(node: Node<'_>) -> bool {
    if matches!(node.kind(), "line_comment" | "block_comment") {
        return true;
    }
    !node.is_named()
        && matches!(
            node.kind(),
            "{" | "}" | "(" | ")" | "[" | "]" | "," | ";" | "\"" | "'"
        )
}

fn normalize_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn pascal_case(value: &str) -> String {
    let mut result = String::new();
    let mut uppercase = true;
    for character in value.chars() {
        if character == '_' || character == '-' {
            uppercase = true;
            continue;
        }
        if uppercase {
            result.extend(character.to_uppercase());
            uppercase = false;
        } else {
            result.push(character);
        }
    }
    result
}

#[cfg(test)]
mod tests;
