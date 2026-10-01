use std::{collections::BTreeMap, sync::Arc};

use ctc_core::canonical::CanonicalScalar;
use tree_sitter::Node;

/// C++ member fields beyond the shared `MEMBER_FIELDS`. They are C++ only.
pub(crate) const EXTRA_MEMBER_FIELDS: [&str; 10] = [
    "const",
    "defaulted",
    "deleted",
    "destructor",
    "final",
    "friend",
    "operator",
    "override",
    "pure",
    "virtual",
];

pub(crate) fn is_access_label(node: Node<'_>) -> bool {
    node.kind() == "access_specifier" && node.parent().and_then(member_list).is_some()
}

pub(crate) fn is_access_label_colon(node: Node<'_>) -> bool {
    !node.is_named() && node.kind() == ":" && node.prev_sibling().is_some_and(is_access_label)
}

/// Returns the class body that holds `node` as a member, looking through
/// preprocessor blocks inside the body.
fn member_body(node: Node<'_>) -> Option<Node<'_>> {
    if !node.is_named()
        || node.kind() == "comment"
        || node.kind() == "access_specifier"
        || node.kind().starts_with("preproc_")
    {
        return None;
    }
    let parent = node.parent()?;
    if parent.kind().starts_with("preproc_")
        && ["name", "condition"]
            .iter()
            .any(|field| parent.child_by_field_name(field) == Some(node))
    {
        return None;
    }
    member_list(parent)
}

fn member_list(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = node;
    loop {
        if current.kind() == "field_declaration_list" {
            return Some(current);
        }
        if !current.kind().starts_with("preproc_") {
            return None;
        }
        current = current.parent()?;
    }
}

/// Finds the access of a member. In templates, a member gets an access only
/// from an access label written before it.
pub(crate) fn member_access(node: Node<'_>, source: &str, template: bool) -> Option<String> {
    let body = member_body(node)?;
    let mut current = node;
    while current.id() != body.id() {
        let mut sibling = current.prev_sibling();
        while let Some(previous) = sibling {
            if previous.kind() == "access_specifier" {
                return previous
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|text| text.trim().to_string());
            }
            sibling = previous.prev_sibling();
        }
        current = current.parent()?;
    }
    if template {
        return None;
    }
    Some(
        match body.parent().map(|parent| parent.kind()) {
            Some("class_specifier") => "private",
            _ => "public",
        }
        .to_string(),
    )
}

pub(crate) fn member_fields(node: Node<'_>, source: &str) -> BTreeMap<Arc<str>, CanonicalScalar> {
    let mut fields = BTreeMap::new();
    let Some(body) = member_body(node) else {
        return fields;
    };
    if let Some(access) = member_access(node, source, false) {
        fields.insert(
            Arc::from("access"),
            CanonicalScalar::String(Arc::from(access)),
        );
    }
    let declaration = unwrap_template(node);
    let class_name = body
        .parent()
        .and_then(|class| class.child_by_field_name("name"))
        .and_then(|name| name.utf8_text(source.as_bytes()).ok());
    let constructor = class_name.is_some_and(|class_name| {
        declared_name(declaration, source).is_some_and(|name| name == base_name(class_name))
    });
    let function = member_function(declaration);
    let name_kind = function.map(|function| function.name.kind());
    let has_specifier = |text: &str| {
        function
            .is_some_and(|function| has_child(function.suffix, "virtual_specifier", text, source))
    };
    let flags = [
        ("constructor", constructor),
        ("static", has_static(declaration, source)),
        (
            "const",
            function.is_some_and(|function| {
                has_child(function.suffix, "type_qualifier", "const", source)
            }),
        ),
        (
            "defaulted",
            has_child(declaration, "default_method_clause", "", source),
        ),
        (
            "deleted",
            has_child(declaration, "delete_method_clause", "", source),
        ),
        ("destructor", name_kind == Some("destructor_name")),
        ("final", has_specifier("final")),
        ("friend", declaration.kind() == "friend_declaration"),
        (
            "operator",
            matches!(name_kind, Some("operator_name" | "operator_cast")),
        ),
        ("override", has_specifier("override")),
        ("pure", function.is_some() && is_pure(declaration, source)),
        (
            "virtual",
            function.is_some() && has_child(declaration, "virtual", "", source),
        ),
    ];
    for (name, value) in flags {
        fields.insert(Arc::from(name), CanonicalScalar::Bool(value));
    }
    fields
}

pub(crate) fn is_member_field(name: &str) -> bool {
    ctc_core::template::MEMBER_FIELDS.contains(&name) || EXTRA_MEMBER_FIELDS.contains(&name)
}

pub(crate) fn remove_member_fields(fields: &mut BTreeMap<Arc<str>, CanonicalScalar>) {
    fields.retain(|name, _| !is_member_field(name));
}

#[derive(Clone, Copy)]
struct MemberFunction<'tree> {
    /// The declared name, such as an identifier, `destructor_name`,
    /// `operator_name`, or the whole `operator_cast`.
    name: Node<'tree>,
    /// The declarator that holds `const`, `override`, and `final` after the
    /// parameter list.
    suffix: Node<'tree>,
}

/// Finds the member function that a declaration declares. A data member with a
/// function pointer type is not a member function.
fn member_function(node: Node<'_>) -> Option<MemberFunction<'_>> {
    let mut declarator = node.child_by_field_name("declarator")?;
    loop {
        match declarator.kind() {
            "function_declarator" => {
                let name = declarator.child_by_field_name("declarator")?;
                if name.kind() == "parenthesized_declarator" {
                    return None;
                }
                return Some(MemberFunction {
                    name,
                    suffix: declarator,
                });
            }
            "operator_cast" => {
                return Some(MemberFunction {
                    name: declarator,
                    suffix: declarator
                        .child_by_field_name("declarator")
                        .unwrap_or(declarator),
                });
            }
            "pointer_declarator" | "init_declarator" => {
                declarator = declarator.child_by_field_name("declarator")?;
            }
            // The grammar does not name the inner declarator of a reference.
            "reference_declarator" => {
                let last = (declarator.named_child_count() as u32).checked_sub(1)?;
                declarator = declarator.named_child(last)?;
            }
            _ => return None,
        }
    }
}

/// Checks for a direct child of `kind`. When `text` is not empty, the child
/// text must also equal it.
fn has_child(node: Node<'_>, kind: &str, text: &str, source: &str) -> bool {
    (0..node.child_count()).any(|index| {
        node.child(index).is_some_and(|child| {
            child.kind() == kind
                && (text.is_empty()
                    || child
                        .utf8_text(source.as_bytes())
                        .is_ok_and(|child_text| child_text.trim() == text))
        })
    })
}

/// A pure virtual function ends with `= 0`. The grammar gives that either a
/// `pure_virtual_clause` or a `default_value` of `0`.
fn is_pure(node: Node<'_>, source: &str) -> bool {
    has_child(node, "pure_virtual_clause", "", source)
        || node
            .child_by_field_name("default_value")
            .is_some_and(|value| {
                value.kind() == "number_literal"
                    && value
                        .utf8_text(source.as_bytes())
                        .is_ok_and(|text| text.trim() == "0")
            })
}

fn unwrap_template(node: Node<'_>) -> Node<'_> {
    let mut current = node;
    while current.kind() == "template_declaration" {
        let inner = (0..current.named_child_count() as u32)
            .filter_map(|index| current.named_child(index))
            .find(|child| {
                matches!(
                    child.kind(),
                    "declaration"
                        | "field_declaration"
                        | "function_definition"
                        | "friend_declaration"
                        | "template_declaration"
                )
            });
        let Some(inner) = inner else {
            break;
        };
        current = inner;
    }
    current
}

fn declared_name<'a>(node: Node<'_>, source: &'a str) -> Option<&'a str> {
    let name = member_function(node)?.name;
    match name.kind() {
        "identifier" | "field_identifier" | "type_identifier" => {
            name.utf8_text(source.as_bytes()).ok()
        }
        _ => None,
    }
}

fn base_name(class_name: &str) -> &str {
    let without_arguments = class_name.split('<').next().unwrap_or(class_name);
    without_arguments
        .rsplit("::")
        .next()
        .unwrap_or(without_arguments)
        .trim()
}

fn has_static(node: Node<'_>, source: &str) -> bool {
    (0..node.named_child_count() as u32).any(|index| {
        node.named_child(index).is_some_and(|child| {
            child.kind() == "storage_class_specifier"
                && child
                    .utf8_text(source.as_bytes())
                    .is_ok_and(|text| text.trim() == "static")
        })
    })
}

#[cfg(test)]
mod tests;
