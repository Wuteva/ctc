use std::{collections::BTreeMap, path::Path, sync::Arc};

use ctc_core::{
    canonical::{CanonicalNode, CanonicalScalar},
    diagnostic::{Diagnostic, DiagnosticCategory, LineIndex, TextRange},
    engine::ParsedSource,
};
use tree_sitter::{Language, Node, Parser};

use crate::{
    modules::module_scalar,
    semantic::{extract_semantic_facts, semantic_text},
};

pub fn parse_source(source: &str, path: &Path) -> Result<ParsedSource, Vec<Diagnostic>> {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts")
        })
    {
        return Err(vec![
            Diagnostic::new(
                "CTC1008",
                DiagnosticCategory::UnsupportedSourceType,
                "TypeScript declaration files are not supported.",
                2,
            )
            .with_source(TextRange::from_offsets(path, source, 0, 0)),
        ]);
    }

    let tree = parse_tree(source).map_err(|message| {
        vec![Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            message,
            2,
        )]
    })?;
    let error_nodes = error_nodes(tree.root_node());
    if !error_nodes.is_empty() {
        return Err(error_nodes
            .into_iter()
            .map(|node| {
                Diagnostic::new(
                    "CTC3001",
                    DiagnosticCategory::SourceParseError,
                    "The source contains invalid TypeScript syntax.",
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
            "The TypeScript adapter removed the source-file root.",
            2,
        )]);
    };
    Ok(ParsedSource {
        path: path.to_path_buf(),
        language_id: "typescript",
        source: source.to_string(),
        root,
        semantic_facts: extract_semantic_facts(tree.root_node(), source, &lines),
        comments: comment_ranges(tree.root_node()),
    })
}

fn comment_ranges(root: Node<'_>) -> Vec<std::ops::Range<usize>> {
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

pub(crate) fn parse_tree(source: &str) -> Result<tree_sitter::Tree, String> {
    let mut parser = Parser::new();
    let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    parser
        .set_language(&language)
        .map_err(|error| format!("Cannot load the TypeScript grammar: {error}"))?;
    let parser_source = normalize_parser_input(source);
    parser
        .parse(parser_source.as_ref(), None)
        .ok_or_else(|| "The TypeScript parser did not return a syntax tree.".to_string())
}

fn normalize_parser_input(source: &str) -> std::borrow::Cow<'_, str> {
    if !source.contains("export type * as ") {
        return std::borrow::Cow::Borrowed(source);
    }
    let mut normalized = source.as_bytes().to_vec();
    let mut line_start = 0;
    while line_start < source.len() {
        let line_end = source[line_start..]
            .find('\n')
            .map_or(source.len(), |offset| line_start + offset);
        let line = &source[line_start..line_end];
        let indentation = line.len() - line.trim_start().len();
        let statement_start = line_start + indentation;
        if source[statement_start..line_end].starts_with("export type * as ") {
            let type_start = statement_start + "export ".len();
            normalized[type_start..type_start + "type".len()].fill(b' ');
        }
        line_start = line_end.saturating_add(1);
    }
    match String::from_utf8(normalized) {
        Ok(normalized) => std::borrow::Cow::Owned(normalized),
        Err(_) => std::borrow::Cow::Borrowed(source),
    }
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

fn canonicalize_indexed(
    node: Node<'_>,
    source: &str,
    lines: &LineIndex<'_>,
) -> Option<CanonicalNode> {
    if should_skip(node) {
        return None;
    }
    let text = node.utf8_text(source.as_bytes()).unwrap_or_default();
    if node.kind() == "string" {
        return Some(CanonicalNode {
            kind: Arc::from("StringLiteral"),
            value: Some(CanonicalScalar::String(Arc::from(decode_string(text)))),
            fields: BTreeMap::new(),
            children: Vec::new(),
            range: lines.range(node.start_byte(), node.end_byte()),
        });
    }
    let mut children = Vec::new();
    let mut decorators = Vec::new();
    for index in 0..node.child_count() {
        let Some(raw_child) = node.child(index) else {
            continue;
        };
        let Some(mut child) = canonicalize_indexed(raw_child, source, lines) else {
            continue;
        };
        if node.kind() == "class_body" {
            if raw_child.kind() == "decorator" {
                decorators.push(child);
                continue;
            }
            child.children.splice(0..0, decorators.drain(..));
        }
        children.push(child);
    }
    children.append(&mut decorators);
    let kind = canonical_kind(node, source);
    let value = canonical_value(node, text, children.is_empty() && node.child_count() == 0);
    let fields = canonical_fields(node, source);
    Some(CanonicalNode {
        kind: Arc::from(kind),
        value,
        fields,
        children,
        range: lines.range(node.start_byte(), node.end_byte()),
    })
}

pub(crate) fn canonical_kind(node: Node<'_>, source: &str) -> String {
    if !node.is_named() {
        return "Token".to_string();
    }
    if node.kind() == "import_statement" && contains_kind(node, "import_require_clause") {
        return "ImportEqualsDeclaration".to_string();
    }
    let _ = source;
    canonical_kind_from_raw(node.kind())
}

fn canonical_kind_from_raw(kind: &str) -> String {
    match kind {
        "program" => "SourceFile".to_string(),
        "import_statement" => "ImportDeclaration".to_string(),
        "interface_declaration" => "InterfaceDeclaration".to_string(),
        "class_declaration" => "ClassDeclaration".to_string(),
        "function_declaration" => "FunctionDeclaration".to_string(),
        "lexical_declaration" | "variable_declaration" => "VariableStatement".to_string(),
        "identifier" | "type_identifier" | "property_identifier" => "Identifier".to_string(),
        "string" => "StringLiteral".to_string(),
        "string_fragment" => "StringFragment".to_string(),
        "number" => "NumericLiteral".to_string(),
        other => pascal_case(other),
    }
}

fn canonical_value(node: Node<'_>, text: &str, has_no_children: bool) -> Option<CanonicalScalar> {
    if !has_no_children {
        return None;
    }
    let value = match node.kind() {
        "number" => text.replace('_', ""),
        "string_fragment" | "identifier" | "type_identifier" | "property_identifier" => {
            normalize_line_endings(text)
        }
        _ if !node.is_named() => normalize_line_endings(text),
        _ => normalize_line_endings(text),
    };
    Some(CanonicalScalar::String(Arc::from(value)))
}

pub(crate) fn decode_string(text: &str) -> String {
    let body = text
        .strip_prefix(['\'', '"'])
        .and_then(|value| value.strip_suffix(['\'', '"']))
        .unwrap_or(text);
    let mut result = String::new();
    let mut characters = body.chars();
    let mut pending_high_surrogate = None;
    while let Some(character) = characters.next() {
        if character != '\\' {
            push_decoded_value(character as u32, &mut pending_high_surrogate, &mut result);
            continue;
        }
        let Some(escaped) = characters.next() else {
            result.push('\\');
            break;
        };
        match escaped {
            'n' => push_decoded_value(b'\n' as u32, &mut pending_high_surrogate, &mut result),
            'r' => push_decoded_value(b'\r' as u32, &mut pending_high_surrogate, &mut result),
            't' => push_decoded_value(b'\t' as u32, &mut pending_high_surrogate, &mut result),
            'b' => push_decoded_value(0x08, &mut pending_high_surrogate, &mut result),
            'f' => push_decoded_value(0x0c, &mut pending_high_surrogate, &mut result),
            'v' => push_decoded_value(0x0b, &mut pending_high_surrogate, &mut result),
            '0' => push_decoded_value(0, &mut pending_high_surrogate, &mut result),
            '\\' => push_decoded_value(b'\\' as u32, &mut pending_high_surrogate, &mut result),
            '\'' => push_decoded_value(b'\'' as u32, &mut pending_high_surrogate, &mut result),
            '"' => push_decoded_value(b'"' as u32, &mut pending_high_surrogate, &mut result),
            '\n' => {}
            '\r' => {
                if characters.clone().next() == Some('\n') {
                    characters.next();
                }
            }
            'x' => {
                let digits: String = characters.by_ref().take(2).collect();
                if let Ok(value) = u32::from_str_radix(&digits, 16) {
                    push_decoded_value(value, &mut pending_high_surrogate, &mut result);
                }
            }
            'u' => {
                let mut digits = String::new();
                if characters.clone().next() == Some('{') {
                    characters.next();
                    for digit in characters.by_ref() {
                        if digit == '}' {
                            break;
                        }
                        digits.push(digit);
                    }
                } else {
                    digits.extend(characters.by_ref().take(4));
                }
                if let Ok(value) = u32::from_str_radix(&digits, 16) {
                    push_decoded_value(value, &mut pending_high_surrogate, &mut result);
                }
            }
            other => push_decoded_value(other as u32, &mut pending_high_surrogate, &mut result),
        }
    }
    if pending_high_surrogate.is_some() {
        result.push('\u{fffd}');
    }
    result
}

fn push_decoded_value(value: u32, pending_high: &mut Option<u16>, result: &mut String) {
    if let Some(high) = pending_high.take() {
        if (0xdc00..=0xdfff).contains(&value) {
            let codepoint = 0x10000 + (((high as u32 - 0xd800) << 10) | (value - 0xdc00));
            if let Some(character) = char::from_u32(codepoint) {
                result.push(character);
                return;
            }
        }
        result.push('\u{fffd}');
    }
    if (0xd800..=0xdbff).contains(&value) {
        *pending_high = Some(value as u16);
    } else if (0xdc00..=0xdfff).contains(&value) {
        result.push('\u{fffd}');
    } else if let Some(character) = char::from_u32(value) {
        result.push(character);
    } else {
        result.push('\u{fffd}');
    }
}

fn normalize_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

pub(crate) fn canonical_fields(
    node: Node<'_>,
    source: &str,
) -> BTreeMap<Arc<str>, CanonicalScalar> {
    let mut fields = BTreeMap::new();
    match node.kind() {
        "import_statement" => {
            let type_only = has_type_only_import_modifier(node);
            fields.insert(Arc::from("typeOnly"), CanonicalScalar::Bool(type_only));
        }
        "interface_declaration" | "type_alias_declaration" => {
            fields.insert(Arc::from("typeOnly"), CanonicalScalar::Bool(true));
        }
        "export_statement" => {
            let type_only = is_type_only_export(node);
            fields.insert(Arc::from("typeOnly"), CanonicalScalar::Bool(type_only));
        }
        "call_expression" | "new_expression" => {
            let callee = node.child_by_field_name(if node.kind() == "call_expression" {
                "function"
            } else {
                "constructor"
            });
            if let Some(callee) = callee {
                fields.insert(
                    Arc::from("callee"),
                    CanonicalScalar::String(Arc::from(semantic_text(callee, source))),
                );
            }
        }
        _ => {}
    }
    if let Some(module) = module_scalar(node, source) {
        fields.insert(Arc::from("module"), module);
    }
    if is_class_member(node) {
        fields.insert(
            Arc::from("access"),
            CanonicalScalar::String(Arc::from(member_access(node, source))),
        );
        let is_static = node.kind() == "class_static_block" || has_token(node, "static");
        fields.insert(Arc::from("static"), CanonicalScalar::Bool(is_static));
        let constructor = !is_static
            && matches!(node.kind(), "method_definition" | "method_signature")
            && node.child_by_field_name("name").is_some_and(|name| {
                name.kind() == "property_identifier"
                    && name.utf8_text(source.as_bytes()) == Ok("constructor")
            });
        fields.insert(Arc::from("constructor"), CanonicalScalar::Bool(constructor));
    }
    fields
}

fn is_class_member(node: Node<'_>) -> bool {
    node.is_named()
        && !matches!(node.kind(), "comment" | "decorator")
        && node
            .parent()
            .is_some_and(|parent| parent.kind() == "class_body")
}

fn member_access(node: Node<'_>, source: &str) -> String {
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index)
            && child.kind() == "accessibility_modifier"
        {
            return child
                .utf8_text(source.as_bytes())
                .unwrap_or("public")
                .trim()
                .to_string();
        }
    }
    let private_name = node
        .child_by_field_name("name")
        .is_some_and(|name| name.kind() == "private_property_identifier");
    if private_name { "private" } else { "public" }.to_string()
}

fn has_token(node: Node<'_>, token: &str) -> bool {
    (0..node.child_count()).any(|index| {
        node.child(index)
            .is_some_and(|child| !child.is_named() && child.kind() == token)
    })
}

fn is_type_only_export(node: Node<'_>) -> bool {
    (0..node.child_count()).any(|index| {
        node.child(index).is_some_and(|child| {
            (!child.is_named() && child.kind() == "type")
                || matches!(
                    child.kind(),
                    "interface_declaration" | "type_alias_declaration"
                )
        })
    })
}

fn has_type_only_import_modifier(node: Node<'_>) -> bool {
    for index in 0..node.child_count() {
        let Some(child) = node.child(index) else {
            continue;
        };
        if !child.is_named() && child.kind() == "type" {
            return true;
        }
        if matches!(child.kind(), "import_clause" | "import_require_clause") {
            for child_index in 0..child.child_count() {
                if child
                    .child(child_index)
                    .is_some_and(|token| !token.is_named() && token.kind() == "type")
                {
                    return true;
                }
            }
        }
    }
    false
}

fn should_skip(node: Node<'_>) -> bool {
    if node.kind() == "comment" {
        return true;
    }
    !node.is_named()
        && matches!(
            node.kind(),
            "{" | "}" | "(" | ")" | "[" | "]" | "," | ";" | "\"" | "'" | "`"
        )
}

fn contains_kind(node: Node<'_>, kind: &str) -> bool {
    if node.kind() == kind {
        return true;
    }
    (0..node.child_count()).any(|index| {
        node.child(index)
            .is_some_and(|child| contains_kind(child, kind))
    })
}

fn pascal_case(value: &str) -> String {
    let mut result = String::new();
    let mut uppercase = true;
    for character in value.chars() {
        if character == '_' || character == '-' {
            uppercase = true;
        } else if uppercase {
            result.extend(character.to_uppercase());
            uppercase = false;
        } else {
            result.push(character);
        }
    }
    result
}

pub fn known_kind(value: &str) -> bool {
    if value == "ImportEqualsDeclaration" {
        return true;
    }
    let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    (0..language.node_kind_count()).any(|id| {
        language.node_kind_is_named(id as u16)
            && language
                .node_kind_for_id(id as u16)
                .is_some_and(|kind| canonical_kind_from_raw(kind) == value)
    }) || matches!(
        value,
        "SourceFile"
            | "ImportDeclaration"
            | "InterfaceDeclaration"
            | "ClassDeclaration"
            | "FunctionDeclaration"
            | "VariableStatement"
            | "Identifier"
            | "StringLiteral"
            | "NumericLiteral"
    )
}

#[cfg(test)]
mod tests;
