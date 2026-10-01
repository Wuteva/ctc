use ctc_core::canonical::CanonicalScalar;
use tree_sitter::Node;

use crate::canonicalize::decode_string;

/// The module that an import, an export-from, a `require(...)` call, or a
/// dynamic `import(...)` names, without quotes.
pub(crate) fn module_scalar(node: Node<'_>, source: &str) -> Option<CanonicalScalar> {
    let string = match node.kind() {
        "import_statement" | "export_statement" => node
            .child_by_field_name("source")
            .or_else(|| require_clause_string(node)),
        "call_expression" => call_argument_string(node, source),
        _ => None,
    }?;
    let text = string.utf8_text(source.as_bytes()).ok()?;
    Some(CanonicalScalar::String(decode_string(text).into()))
}

fn require_clause_string(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == "import_require_clause")
        .and_then(|clause| {
            let mut inner = clause.walk();
            clause
                .named_children(&mut inner)
                .find(|child| child.kind() == "string")
        })
}

fn call_argument_string<'tree>(node: Node<'tree>, source: &str) -> Option<Node<'tree>> {
    let function = node.child_by_field_name("function")?;
    let is_module_call = function.kind() == "import"
        || (function.kind() == "identifier"
            && function.utf8_text(source.as_bytes()) == Ok("require"));
    if !is_module_call {
        return None;
    }
    let arguments = node.child_by_field_name("arguments")?;
    arguments
        .named_child(0)
        .filter(|argument| argument.kind() == "string")
}
