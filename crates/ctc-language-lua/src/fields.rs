use std::{collections::BTreeMap, sync::Arc};

use ctc_core::{canonical::CanonicalScalar, template::MEMBER_FIELDS};
use tree_sitter::Node;

use crate::strings::decode_string;

/// Fields that only source nodes carry, besides the shared `callee` and
/// `module`.
pub(crate) const EXTRA_SOURCE_FIELDS: [&str; 2] = ["local", "global"];

const DECLARATION_KINDS: [&str; 3] = [
    "function_declaration",
    "implicit_variable_declaration",
    "variable_declaration",
];

pub(crate) fn canonical_fields(
    node: Node<'_>,
    source: &str,
) -> BTreeMap<Arc<str>, CanonicalScalar> {
    let mut fields = BTreeMap::new();
    insert_call_fields(&mut fields, node, source);
    insert_declaration_fields(&mut fields, node, source);
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

/// True when `node` is the keyword that starts a declaration, such as `local`
/// in `local x = 1`, and not the name `global` that Lua 5.5 allows by default.
pub(crate) fn is_declaration_keyword(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        DECLARATION_KINDS.contains(&parent.kind())
            && parent.child(0).is_some_and(|first| first.id() == node.id())
    })
}

/// `callee` on a call is the text of the called expression without white
/// space or comments, such as `print`, `string.format`, or `self:emit`.
/// `module` on a `require` call is the decoded text of its string argument.
fn insert_call_fields(
    fields: &mut BTreeMap<Arc<str>, CanonicalScalar>,
    node: Node<'_>,
    source: &str,
) {
    if node.kind() != "function_call" {
        return;
    }
    let Some(callee) = node
        .child_by_field_name("name")
        .map(|name| compact_text(name, source))
    else {
        return;
    };
    if callee == "require"
        && let Some(module) = node
            .child_by_field_name("arguments")
            .and_then(|arguments| arguments.named_child(0))
            .filter(|argument| argument.kind() == "string")
            .and_then(|argument| argument.utf8_text(source.as_bytes()).ok())
            .and_then(decode_string)
    {
        fields.insert(
            Arc::from("module"),
            CanonicalScalar::String(Arc::from(module)),
        );
    }
    fields.insert(
        Arc::from("callee"),
        CanonicalScalar::String(Arc::from(callee)),
    );
}

/// `local` and `global` tell how a declaration starts. A plain
/// `function M.run() end` has both set to `false`.
fn insert_declaration_fields(
    fields: &mut BTreeMap<Arc<str>, CanonicalScalar>,
    node: Node<'_>,
    source: &str,
) {
    if !DECLARATION_KINDS.contains(&node.kind()) {
        return;
    }
    let first = node
        .child(0)
        .and_then(|child| child.utf8_text(source.as_bytes()).ok())
        .unwrap_or_default();
    for keyword in EXTRA_SOURCE_FIELDS {
        fields.insert(Arc::from(keyword), CanonicalScalar::Bool(first == keyword));
    }
}

/// The text of `node` without white space or comments. A string literal keeps
/// its text as written.
pub(crate) fn compact_text(node: Node<'_>, source: &str) -> String {
    fn append(node: Node<'_>, source: &str, output: &mut String) {
        if node.kind() == "comment" {
            return;
        }
        if node.child_count() == 0 || node.kind() == "string" {
            if let Ok(text) = node.utf8_text(source.as_bytes()) {
                if node.kind() == "string" {
                    output.push_str(text);
                } else {
                    output.extend(text.chars().filter(|character| !character.is_whitespace()));
                }
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
