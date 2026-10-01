use std::{borrow::Cow, collections::BTreeMap, path::Path, sync::Arc};

use ctc_core::{
    canonical::{CanonicalNode, CanonicalScalar, SemanticFacts},
    diagnostic::{Diagnostic, DiagnosticCategory, LineIndex, TextRange},
    engine::ParsedSource,
};
use tree_sitter::{Language, Node, Parser};

use crate::{
    definitions::member_function_facts,
    grammar_gaps::mask_grammar_gaps,
    members::{is_access_label, is_access_label_colon, member_fields},
};

pub fn parse_source(source: &str, path: &Path) -> Result<ParsedSource, Vec<Diagnostic>> {
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
                    "The source contains invalid C++ syntax.",
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
            "The C++ adapter removed the source-file root.",
            2,
        )]);
    };
    Ok(ParsedSource {
        path: path.to_path_buf(),
        language_id: "cpp",
        source: source.to_string(),
        root,
        semantic_facts: SemanticFacts {
            member_functions: member_function_facts(tree.root_node(), source, &lines),
            ..SemanticFacts::default()
        },
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
    let language: Language = tree_sitter_cpp::LANGUAGE.into();
    parser
        .set_language(&language)
        .map_err(|error| format!("Cannot load the C++ grammar: {error}"))?;
    let parser_source = normalize_parser_input(source);
    parser
        .parse(parser_source.as_ref(), None)
        .ok_or_else(|| "The C++ parser did not return a syntax tree.".to_string())
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
    if matches!(
        node.kind(),
        "string_literal" | "raw_string_literal" | "char_literal"
    ) {
        return Some(CanonicalNode {
            kind: Arc::from(canonical_kind(node)),
            value: Some(CanonicalScalar::String(Arc::from(normalize_line_endings(
                text,
            )))),
            fields: BTreeMap::new(),
            children: Vec::new(),
            range: lines.range(node.start_byte(), node.end_byte()),
        });
    }

    let mut children = Vec::new();
    let class_body = node.kind() == "field_declaration_list";
    for index in 0..node.child_count() {
        let Some(child) = node.child(index) else {
            continue;
        };
        if class_body && is_preproc_directive(child) {
            continue;
        }
        if class_body && is_preproc_block(child) {
            push_preproc_members(child, source, lines, &mut children);
        } else if let Some(child) = canonicalize_indexed(child, source, lines) {
            children.push(child);
        }
    }
    Some(CanonicalNode {
        kind: Arc::from(canonical_kind(node)),
        value: canonical_value(node, text, children.is_empty() && node.child_count() == 0),
        fields: canonical_fields(node, source),
        children,
        range: lines.range(node.start_byte(), node.end_byte()),
    })
}

fn is_preproc_block(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "preproc_if" | "preproc_ifdef" | "preproc_else" | "preproc_elif" | "preproc_elifdef"
    )
}

/// `#define` and `#undef` lines in a class body are not members.
fn is_preproc_directive(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "preproc_def" | "preproc_function_def" | "preproc_call"
    )
}

/// Members inside `#if` blocks of a class body count as members of the class
/// body, so member rules see them in source order. The condition is dropped.
fn push_preproc_members(
    block: Node<'_>,
    source: &str,
    lines: &LineIndex<'_>,
    members: &mut Vec<CanonicalNode>,
) {
    let condition = block
        .child_by_field_name("condition")
        .or_else(|| block.child_by_field_name("name"));
    for index in 0..block.child_count() {
        let Some(child) = block.child(index) else {
            continue;
        };
        if !child.is_named() || condition == Some(child) || is_preproc_directive(child) {
            continue;
        }
        if is_preproc_block(child) {
            push_preproc_members(child, source, lines, members);
        } else if let Some(member) = canonicalize_indexed(child, source, lines) {
            members.push(member);
        }
    }
}

pub(crate) fn canonical_kind(node: Node<'_>) -> String {
    if !node.is_named() {
        return "Token".to_string();
    }
    canonical_kind_from_raw(node.kind())
}

fn canonical_kind_from_raw(kind: &str) -> String {
    match kind {
        "translation_unit" => "SourceFile".to_string(),
        "function_definition" => "FunctionDeclaration".to_string(),
        "lambda_expression" => "ArrowFunction".to_string(),
        "class_specifier" => "ClassDeclaration".to_string(),
        "struct_specifier" => "StructDeclaration".to_string(),
        "union_specifier" => "UnionDeclaration".to_string(),
        "enum_specifier" => "EnumDeclaration".to_string(),
        "namespace_definition" => "NamespaceDeclaration".to_string(),
        "field_declaration_list" => "ClassBody".to_string(),
        "compound_statement" => "StatementBlock".to_string(),
        "preproc_include" => "IncludeDirective".to_string(),
        "preproc_def" => "MacroDefinition".to_string(),
        "preproc_function_def" => "FunctionMacroDefinition".to_string(),
        "identifier"
        | "type_identifier"
        | "field_identifier"
        | "namespace_identifier"
        | "statement_identifier" => "Identifier".to_string(),
        "string_literal" | "raw_string_literal" | "concatenated_string" => {
            "StringLiteral".to_string()
        }
        "char_literal" => "CharacterLiteral".to_string(),
        "number_literal" => "NumericLiteral".to_string(),
        other => pascal_case(other),
    }
}

fn canonical_value(node: Node<'_>, text: &str, has_no_children: bool) -> Option<CanonicalScalar> {
    if !has_no_children {
        return None;
    }
    let value = if node.kind() == "number_literal" {
        text.replace('\'', "")
    } else {
        normalize_line_endings(text)
    };
    Some(CanonicalScalar::String(Arc::from(value)))
}

pub(crate) fn canonical_fields(
    node: Node<'_>,
    source: &str,
) -> BTreeMap<Arc<str>, CanonicalScalar> {
    let mut fields = member_fields(node, source);
    if node.kind() == "call_expression"
        && let Some(function) = node.child_by_field_name("function")
    {
        let callee = function
            .utf8_text(source.as_bytes())
            .unwrap_or_default()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        fields.insert(
            Arc::from("callee"),
            CanonicalScalar::String(Arc::from(callee)),
        );
    }
    if node.kind() == "preproc_include"
        && let Some(path) = node.child_by_field_name("path")
    {
        let text = path.utf8_text(source.as_bytes()).unwrap_or_default();
        let module = text.trim().trim_matches(|c| matches!(c, '<' | '>' | '"'));
        fields.insert(
            Arc::from("module"),
            CanonicalScalar::String(Arc::from(module)),
        );
    }
    fields
}

fn normalize_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn should_skip(node: Node<'_>) -> bool {
    if node.kind() == "comment" || is_access_label(node) || is_access_label_colon(node) {
        return true;
    }
    !node.is_named()
        && matches!(
            node.kind(),
            "{" | "}" | "(" | ")" | "[" | "]" | "," | ";" | "\"" | "'"
        )
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
    let language: Language = tree_sitter_cpp::LANGUAGE.into();
    (0..language.node_kind_count()).any(|id| {
        language.node_kind_is_named(id as u16)
            && language
                .node_kind_for_id(id as u16)
                .is_some_and(|kind| canonical_kind_from_raw(kind) == value)
    }) || matches!(
        value,
        "SourceFile"
            | "FunctionDeclaration"
            | "ArrowFunction"
            | "ClassDeclaration"
            | "StructDeclaration"
            | "UnionDeclaration"
            | "EnumDeclaration"
            | "NamespaceDeclaration"
            | "ClassBody"
            | "StatementBlock"
            | "IncludeDirective"
            | "MacroDefinition"
            | "FunctionMacroDefinition"
            | "Identifier"
            | "StringLiteral"
            | "CharacterLiteral"
            | "NumericLiteral"
    )
}

pub fn known_keyword(value: &str) -> bool {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
        || value.as_bytes()[0].is_ascii_digit()
    {
        return false;
    }
    let language: Language = tree_sitter_cpp::LANGUAGE.into();
    (0..language.node_kind_count()).any(|id| {
        !language.node_kind_is_named(id as u16)
            && language
                .node_kind_for_id(id as u16)
                .is_some_and(|kind| kind == value)
    })
}

#[cfg(test)]
mod tests;
