use std::{collections::BTreeMap, sync::Arc};

use ctc_core::{canonical::CanonicalScalar, template::MEMBER_FIELDS};
use tree_sitter::Node;

pub(crate) const EXTRA_SOURCE_FIELDS: [&str; 7] = [
    "attribute",
    "public",
    "visibility",
    "async",
    "unsafe",
    "const",
    "mutable",
];

const VISIBILITY_KINDS: [&str; 9] = [
    "const_item",
    "enum_item",
    "field_declaration",
    "function_item",
    "mod_item",
    "static_item",
    "struct_item",
    "trait_item",
    "use_declaration",
];

pub(crate) fn canonical_fields(
    node: Node<'_>,
    source: &str,
) -> BTreeMap<Arc<str>, CanonicalScalar> {
    let mut fields = BTreeMap::new();
    insert_callee_field(&mut fields, node, source);
    insert_attribute_field(&mut fields, node, source);
    insert_module_field(&mut fields, node, source);
    insert_visibility_fields(&mut fields, node, source);
    insert_function_flags(&mut fields, node, source);
    insert_mutable_field(&mut fields, node);
    fields
}

pub(crate) fn is_known_field(name: &str) -> bool {
    MEMBER_FIELDS.contains(&name) || EXTRA_SOURCE_FIELDS.contains(&name)
}

pub(crate) fn remove_source_only_fields(fields: &mut BTreeMap<Arc<str>, CanonicalScalar>) {
    fields.retain(|name, _| {
        !MEMBER_FIELDS.contains(&name.as_ref()) && !EXTRA_SOURCE_FIELDS.contains(&name.as_ref())
    });
}

fn insert_callee_field(
    fields: &mut BTreeMap<Arc<str>, CanonicalScalar>,
    node: Node<'_>,
    source: &str,
) {
    let callee = match node.kind() {
        "call_expression" => node
            .child_by_field_name("function")
            .and_then(|function| call_callee(function, source)),
        "macro_invocation" => node
            .child_by_field_name("macro")
            .map(|macro_name| format!("{}!", compact_text(macro_name, source))),
        _ => None,
    };
    if let Some(callee) = callee {
        fields.insert(
            Arc::from("callee"),
            CanonicalScalar::String(Arc::from(callee)),
        );
    }
}

/// The path of an attribute, such as `allow`, `derive`, or `clippy::foo`, on
/// `attribute_item` and `inner_attribute_item` nodes.
fn insert_attribute_field(
    fields: &mut BTreeMap<Arc<str>, CanonicalScalar>,
    node: Node<'_>,
    source: &str,
) {
    if !matches!(node.kind(), "attribute_item" | "inner_attribute_item") {
        return;
    }
    let mut cursor = node.walk();
    let path = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "attribute")
        .and_then(|attribute| attribute.named_child(0));
    if let Some(path) = path {
        fields.insert(
            Arc::from("attribute"),
            CanonicalScalar::String(Arc::from(compact_text(path, source))),
        );
    }
}

/// The path of a `use` declaration, such as `std::fs` or `tokio::fs::{self, File}`.
fn insert_module_field(
    fields: &mut BTreeMap<Arc<str>, CanonicalScalar>,
    node: Node<'_>,
    source: &str,
) {
    if node.kind() != "use_declaration" {
        return;
    }
    if let Some(argument) = node.child_by_field_name("argument") {
        fields.insert(
            Arc::from("module"),
            CanonicalScalar::String(Arc::from(compact_text(argument, source))),
        );
    }
}

fn call_callee(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() == "field_expression" {
        return node
            .child_by_field_name("field")
            .map(|field| compact_text(field, source));
    }
    if node.kind() == "generic_function" {
        return node.child_by_field_name("function").and_then(|function| {
            call_callee(function, source).or_else(|| Some(compact_text(function, source)))
        });
    }
    Some(compact_text(node, source))
}

fn compact_text(node: Node<'_>, source: &str) -> String {
    fn append(node: Node<'_>, source: &str, output: &mut String) {
        if matches!(node.kind(), "line_comment" | "block_comment") {
            return;
        }
        if node.child_count() == 0 {
            if let Ok(text) = node.utf8_text(source.as_bytes()) {
                output.extend(text.chars().filter(|character| !character.is_whitespace()));
            }
            return;
        }
        for index in 0..node.child_count() {
            if let Some(child) = node.child(index) {
                append(child, source, output);
            }
        }
    }

    let mut output = String::new();
    append(node, source, &mut output);
    output
}

fn insert_visibility_fields(
    fields: &mut BTreeMap<Arc<str>, CanonicalScalar>,
    node: Node<'_>,
    source: &str,
) {
    if !VISIBILITY_KINDS.contains(&node.kind()) {
        return;
    }
    let visibility = visibility_text(node, source);
    fields.insert(
        Arc::from("public"),
        CanonicalScalar::Bool(visibility.as_deref() == Some("pub")),
    );
    if let Some(visibility) = visibility {
        fields.insert(
            Arc::from("visibility"),
            CanonicalScalar::String(Arc::from(visibility)),
        );
    }
}

fn visibility_text(node: Node<'_>, source: &str) -> Option<String> {
    (0..node.named_child_count() as u32)
        .filter_map(|index| node.named_child(index))
        .find(|child| child.kind() == "visibility_modifier")
        .map(|visibility| compact_text(visibility, source))
}

fn insert_function_flags(
    fields: &mut BTreeMap<Arc<str>, CanonicalScalar>,
    node: Node<'_>,
    source: &str,
) {
    if node.kind() != "function_item" {
        return;
    }
    for keyword in ["async", "unsafe", "const"] {
        fields.insert(
            Arc::from(keyword),
            CanonicalScalar::Bool(has_direct_token(node, source, keyword)),
        );
    }
}

fn has_direct_token(node: Node<'_>, source: &str, token: &str) -> bool {
    (0..node.child_count()).any(|index| {
        node.child(index).is_some_and(|child| {
            child
                .utf8_text(source.as_bytes())
                .is_ok_and(|text| text.trim() == token)
                || has_direct_token(child, source, token)
        })
    })
}

fn insert_mutable_field(fields: &mut BTreeMap<Arc<str>, CanonicalScalar>, node: Node<'_>) {
    if !matches!(
        node.kind(),
        "let_declaration" | "parameter" | "self_parameter" | "variadic_parameter"
    ) {
        return;
    }
    fields.insert(
        Arc::from("mutable"),
        CanonicalScalar::Bool(has_named_child(node, "mutable_specifier")),
    );
}

fn has_named_child(node: Node<'_>, kind: &str) -> bool {
    (0..node.named_child_count() as u32)
        .filter_map(|index| node.named_child(index))
        .any(|child| child.kind() == kind)
}
