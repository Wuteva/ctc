use std::{path::Path, sync::Arc};

use ctc_core::canonical::{CanonicalNode, CanonicalScalar};

use super::{canonical_kind, parse_source, parse_tree};

#[test]
fn maps_shared_canonical_kinds() {
    let tree = parse_tree(
        "pub struct Widget { value: usize }\nimpl Widget { fn build() -> Self { Self { value: 1 } } }\n",
    )
    .unwrap();
    let root = tree.root_node();
    assert_eq!(canonical_kind(root), "SourceFile");
    let struct_item = root.named_child(0).unwrap();
    let body = struct_item.child_by_field_name("body").unwrap();
    let function = root
        .named_child(1)
        .unwrap()
        .child_by_field_name("body")
        .unwrap()
        .named_child(0)
        .unwrap();
    assert_eq!(canonical_kind(struct_item), "StructDeclaration");
    assert_eq!(canonical_kind(body), "ClassBody");
    assert_eq!(canonical_kind(function), "FunctionDeclaration");
}

#[test]
fn captures_rust_specific_fields() {
    let parsed = parse_source(
        "pub const fn build(mut value: usize) -> usize { let mut copy = value; copy }\n",
        Path::new("source.rs"),
    )
    .unwrap();
    let function = &parsed.root.children[0];
    assert_eq!(
        function.fields.get("public"),
        Some(&CanonicalScalar::Bool(true))
    );
    assert_eq!(
        function.fields.get("visibility"),
        Some(&CanonicalScalar::String(Arc::from("pub")))
    );
    assert_eq!(
        function.fields.get("const"),
        Some(&CanonicalScalar::Bool(true))
    );
    let parameter = function
        .children
        .iter()
        .find(|child| child.kind.as_ref() == "Parameters")
        .unwrap()
        .children
        .iter()
        .find(|child| child.kind.as_ref() == "Parameter")
        .unwrap();
    assert_eq!(
        parameter.fields.get("mutable"),
        Some(&CanonicalScalar::Bool(true))
    );
    let body = function
        .children
        .iter()
        .find(|child| child.kind.as_ref() == "StatementBlock")
        .unwrap();
    let declaration = body
        .children
        .iter()
        .find(|child| child.kind.as_ref() == "LetDeclaration")
        .unwrap();
    assert_eq!(
        declaration.fields.get("mutable"),
        Some(&CanonicalScalar::Bool(true))
    );
}

#[test]
fn normalizes_macro_and_call_callee_fields() {
    let parsed = parse_source(
        "fn f() { let _ = Vec::new(); value.unwrap(); println!(\"x\"); }\n",
        Path::new("source.rs"),
    )
    .unwrap();
    fn collect(node: &CanonicalNode, values: &mut Vec<CanonicalScalar>) {
        if let Some(value) = node.fields.get("callee") {
            values.push(value.clone());
        }
        for child in &node.children {
            collect(child, values);
        }
    }
    let mut values = Vec::new();
    collect(&parsed.root, &mut values);
    assert_eq!(
        values,
        vec![
            CanonicalScalar::String(Arc::from("Vec::new")),
            CanonicalScalar::String(Arc::from("unwrap")),
            CanonicalScalar::String(Arc::from("println!"))
        ]
    );
}
