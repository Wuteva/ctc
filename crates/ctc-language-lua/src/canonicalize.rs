use std::{borrow::Cow, collections::BTreeMap, ops::Range, path::Path, sync::Arc};

use ctc_core::{
    canonical::{CanonicalNode, CanonicalScalar, SemanticFacts},
    diagnostic::{Diagnostic, DiagnosticCategory, LineIndex, TextRange},
    engine::ParsedSource,
};
use tree_sitter::{Language, Node, Parser};

use crate::{
    fields::{canonical_fields, is_declaration_keyword},
    grammar_gaps::{lexical_errors, mask_grammar_gaps},
    strings::decode_string,
};

const VALUE_NODES: [&str; 3] = ["hash_bang_line", "number", "string"];

pub fn parse_source(source: &str, path: &Path) -> Result<ParsedSource, Vec<Diagnostic>> {
    let tree = parse_tree(source).map_err(internal_error)?;
    let errors = syntax_errors(tree.root_node(), source);
    if !errors.is_empty() {
        return Err(errors
            .into_iter()
            .map(|(range, message)| {
                Diagnostic::new("CTC3001", DiagnosticCategory::SourceParseError, message, 1)
                    .with_source(TextRange::from_offsets(
                        path,
                        source,
                        range.start,
                        range.end,
                    ))
            })
            .collect());
    }
    let lines = LineIndex::new(path, source);
    let Some(root) = canonicalize_indexed(tree.root_node(), source, &lines) else {
        return Err(vec![Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            "The Lua adapter removed the chunk root.",
            2,
        )]);
    };
    Ok(ParsedSource {
        path: path.to_path_buf(),
        language_id: "lua",
        source: source.to_string(),
        root,
        semantic_facts: SemanticFacts::default(),
        comments: comment_ranges(tree.root_node()),
    })
}

pub(crate) fn parse_tree(source: &str) -> Result<tree_sitter::Tree, String> {
    let mut parser = Parser::new();
    let language: Language = tree_sitter_lua::LANGUAGE.into();
    parser
        .set_language(&language)
        .map_err(|error| format!("Cannot load the Lua grammar: {error}"))?;
    let parser_source = normalize_parser_input(source);
    parser
        .parse(parser_source.as_ref(), None)
        .ok_or_else(|| "The Lua parser did not return a syntax tree.".to_string())
}

/// Error and missing nodes, then the forms that the grammar accepts but Lua
/// rejects, in source order.
pub(crate) fn syntax_errors(root: Node<'_>, source: &str) -> Vec<(Range<usize>, &'static str)> {
    let mut errors = error_nodes(root)
        .into_iter()
        .map(|node| {
            (
                node.start_byte()..node.end_byte(),
                "The source contains invalid Lua syntax.",
            )
        })
        .chain(
            lexical_errors(root, source)
                .into_iter()
                .map(|error| (error.range, error.message)),
        )
        .collect::<Vec<_>>();
    errors.sort_by_key(|(range, message)| (range.start, range.end, *message));
    errors.dedup();
    errors
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
        if node.kind() == "global" && !is_declaration_keyword(node) {
            return "Identifier".to_string();
        }
        return "Token".to_string();
    }
    canonical_kind_from_raw(node.kind())
}

pub fn known_kind(value: &str) -> bool {
    let language: Language = tree_sitter_lua::LANGUAGE.into();
    (0..language.node_kind_count()).any(|id| {
        let id = id as u16;
        language.node_kind_is_named(id)
            && !language.node_kind_is_supertype(id)
            && language
                .node_kind_for_id(id)
                .is_some_and(|kind| canonical_kind_from_raw(kind) == value)
    })
}

/// A child in the canonical tree: a syntax node, or an empty statement block
/// that the grammar leaves out.
pub(crate) enum SyntaxChild<'tree> {
    Node(Node<'tree>),
    /// An empty body at this byte offset, such as in `function f() end`. A
    /// missing block would make `{{* Body }}` fail on an empty body, so
    /// sources and templates both get an empty `StatementBlock`.
    EmptyBlock(usize),
}

/// The children of `node` as the canonical tree has them. The
/// `assignment_statement` that the grammar puts inside `local x = 1` becomes
/// part of the declaration, so that a rule for `x = 1` does not find it.
pub(crate) fn syntax_children(node: Node<'_>) -> Vec<SyntaxChild<'_>> {
    let mut children = Vec::new();
    for index in 0..node.child_count() {
        let Some(child) = node.child(index) else {
            continue;
        };
        if node.kind() == "variable_declaration" && child.kind() == "assignment_statement" {
            children.extend((0..child.child_count()).filter_map(|inner| child.child(inner)));
        } else {
            children.push(child);
        }
    }
    let Some(anchor) = missing_body_anchor(node, &children) else {
        return children.into_iter().map(SyntaxChild::Node).collect();
    };
    let offset = children[anchor].end_byte();
    let mut result = children
        .into_iter()
        .map(SyntaxChild::Node)
        .collect::<Vec<_>>();
    result.insert(anchor + 1, SyntaxChild::EmptyBlock(offset));
    result
}

/// The index of the child after which an empty statement block belongs, when
/// `node` has a body and the grammar left out its block.
fn missing_body_anchor(node: Node<'_>, children: &[Node<'_>]) -> Option<usize> {
    let anchor_kind = match node.kind() {
        "function_declaration" | "function_definition" => "parameters",
        "do_statement" | "while_statement" | "for_statement" => "do",
        "repeat_statement" => "repeat",
        "if_statement" | "elseif_statement" => "then",
        "else_statement" => "else",
        _ => return None,
    };
    let anchor = children
        .iter()
        .position(|child| child.kind() == anchor_kind)?;
    let has_block = children[anchor + 1..]
        .iter()
        .find(|child| child.kind() != "comment")
        .is_some_and(|child| child.kind() == "block");
    (!has_block).then_some(anchor)
}

fn internal_error(message: String) -> Vec<Diagnostic> {
    vec![Diagnostic::new(
        "CTC9001",
        DiagnosticCategory::InternalToolError,
        message,
        2,
    )]
}

fn normalize_parser_input(source: &str) -> Cow<'_, str> {
    let unmasked = match source.strip_prefix('\u{feff}') {
        Some(source_without_bom) => Cow::Owned(format!("   {source_without_bom}")),
        None => Cow::Borrowed(source),
    };
    match mask_grammar_gaps(&unmasked) {
        Some(masked) => Cow::Owned(masked),
        None => unmasked,
    }
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
    for child in syntax_children(node) {
        match child {
            SyntaxChild::Node(raw_child) => {
                children.extend(canonicalize_indexed(raw_child, source, lines));
            }
            SyntaxChild::EmptyBlock(offset) => {
                children.push(empty_block(lines.range(offset, offset)))
            }
        }
    }
    let text = node.utf8_text(source.as_bytes()).unwrap_or_default();
    Some(CanonicalNode {
        kind: Arc::from(canonical_kind(node)),
        value: (children.is_empty() && node.child_count() == 0)
            .then(|| CanonicalScalar::String(Arc::from(normalize_line_endings(text)))),
        fields: canonical_fields(node, source),
        children,
        range: lines.range(node.start_byte(), node.end_byte()),
    })
}

pub(crate) fn empty_block(range: TextRange) -> CanonicalNode {
    CanonicalNode {
        kind: Arc::from("StatementBlock"),
        value: None,
        fields: BTreeMap::new(),
        children: Vec::new(),
        range,
    }
}

fn value_node(node: Node<'_>, source: &str, lines: &LineIndex<'_>) -> CanonicalNode {
    let text = node.utf8_text(source.as_bytes()).unwrap_or_default();
    CanonicalNode {
        kind: Arc::from(canonical_kind(node)),
        value: Some(CanonicalScalar::String(Arc::from(literal_value(
            node, text,
        )))),
        fields: BTreeMap::new(),
        children: Vec::new(),
        range: lines.range(node.start_byte(), node.end_byte()),
    }
}

/// A string literal has its decoded text, so `"a"`, `'a'`, and `[[a]]` are
/// equal. A string that does not decode to UTF-8 keeps its source text.
fn literal_value(node: Node<'_>, text: &str) -> String {
    if node.kind() == "string" {
        decode_string(text).unwrap_or_else(|| normalize_line_endings(text))
    } else {
        normalize_line_endings(text)
    }
}

fn canonical_kind_from_raw(kind: &str) -> String {
    match kind {
        "chunk" => "SourceFile".to_string(),
        "block" => "StatementBlock".to_string(),
        "function_declaration" => "FunctionDeclaration".to_string(),
        "function_definition" => "FunctionExpression".to_string(),
        "function_call" => "CallExpression".to_string(),
        "identifier" => "Identifier".to_string(),
        "string" => "StringLiteral".to_string(),
        "number" => "NumericLiteral".to_string(),
        other => pascal_case(other),
    }
}

fn comment_ranges(root: Node<'_>) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.kind() == "comment" {
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

/// Comments, `;` statements, and punctuation that does not change the meaning
/// do not take part in matching.
fn should_skip(node: Node<'_>) -> bool {
    if matches!(node.kind(), "comment" | "empty_statement") {
        return true;
    }
    !node.is_named() && matches!(node.kind(), "{" | "}" | "(" | ")" | "[" | "]" | "," | ";")
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
