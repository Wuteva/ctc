use super::*;

#[test]
fn collects_comments_but_not_comment_text_inside_strings() {
    let source = "// one\nconst char* text = \"// not a comment\";\n/* two */\nconst char* raw = R\"(/* no */)\";\n";
    let parsed = parse_source(source, Path::new("source.cpp")).unwrap();
    let comments = parsed
        .comments
        .iter()
        .map(|range| &source[range.clone()])
        .collect::<Vec<_>>();
    assert_eq!(comments, vec!["// one", "/* two */"]);
}

#[test]
fn canonicalizes_cpp_structure_for_shared_scopes() {
    let parsed = parse_source(
        r#"
class Widget {
public:
  int value() const { return 1; }
};
"#,
        Path::new("source.cpp"),
    )
    .unwrap();
    let class = parsed
        .root
        .children
        .iter()
        .find(|node| node.kind.as_ref() == "ClassDeclaration")
        .unwrap();
    assert!(
        class
            .children
            .iter()
            .any(|node| node.kind.as_ref() == "ClassBody")
    );
    assert!(contains_kind(class, "FunctionDeclaration"));
    assert!(contains_kind(class, "StatementBlock"));
}

#[test]
fn ignores_comments_and_numeric_separators() {
    let plain = parse_source("int value = 1000;", Path::new("plain.cpp")).unwrap();
    let formatted =
        parse_source("int /* note */ value = 1'000;", Path::new("formatted.cpp")).unwrap();
    assert!(plain.root.structurally_eq(&formatted.root));
}

#[test]
fn rejects_invalid_cpp_source() {
    let diagnostics = parse_source("int main( {", Path::new("source.cpp")).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "CTC3001")
    );
}

#[test]
fn accepts_utf8_byte_order_mark() {
    assert!(parse_source("\u{feff}int value;", Path::new("source.cpp")).is_ok());
}

#[test]
fn exposes_only_canonical_kind_names() {
    assert!(known_kind("ClassDeclaration"));
    assert!(known_kind("ThrowStatement"));
    assert!(!known_kind("ClassSpecifier"));
    assert!(!known_kind("class_specifier"));
}

fn contains_kind(node: &CanonicalNode, kind: &str) -> bool {
    node.kind.as_ref() == kind || node.children.iter().any(|child| contains_kind(child, kind))
}

fn body_members(source: &str) -> Vec<CanonicalNode> {
    fn find_body(node: &CanonicalNode) -> Option<&CanonicalNode> {
        if node.kind.as_ref() == "ClassBody" {
            return Some(node);
        }
        node.children.iter().find_map(find_body)
    }
    let parsed = parse_source(source, Path::new("source.hpp")).unwrap();
    find_body(&parsed.root).unwrap().children.clone()
}

fn access(node: &CanonicalNode) -> String {
    match node.fields.get("access") {
        Some(CanonicalScalar::String(value)) => value.to_string(),
        other => panic!("unexpected access {other:?}"),
    }
}

fn flag(node: &CanonicalNode, name: &str) -> bool {
    node.fields.get(name) == Some(&CanonicalScalar::Bool(true))
}

#[test]
fn drops_access_labels_and_exposes_member_fields() {
    let members = body_members(
        r#"
class Widget : public Base {
  using Id = int;
public:
  Widget();
  explicit Widget(int value) {}
  template <typename T> Widget(T value);
  ~Widget();
  static Widget make();
protected:
  int helper() const;
private:
  int value_;
};
"#,
    );
    let expected = [
        ("AliasDeclaration", "private", false, false),
        ("Declaration", "public", true, false),
        ("FunctionDeclaration", "public", true, false),
        ("TemplateDeclaration", "public", true, false),
        ("Declaration", "public", false, false),
        ("FieldDeclaration", "public", false, true),
        ("FieldDeclaration", "protected", false, false),
        ("FieldDeclaration", "private", false, false),
    ];
    assert_eq!(members.len(), expected.len(), "{members:#?}");
    for (member, (kind, member_access, constructor, is_static)) in members.iter().zip(expected) {
        assert_eq!(member.kind.as_ref(), kind);
        assert_eq!(access(member), member_access, "{kind}");
        assert_eq!(flag(member, "constructor"), constructor, "{kind}");
        assert_eq!(flag(member, "static"), is_static, "{kind}");
    }
}

#[test]
fn struct_members_default_to_public() {
    let members = body_members("struct Point { Point(); int x; };");
    assert_eq!(access(&members[0]), "public");
    assert!(flag(&members[0], "constructor"));
    assert_eq!(access(&members[1]), "public");
}

#[test]
fn empty_class_body_has_no_value() {
    let parsed = parse_source("class Empty {};", Path::new("source.hpp")).unwrap();
    let class = &parsed.root.children[0];
    let body = class
        .children
        .iter()
        .find(|node| node.kind.as_ref() == "ClassBody")
        .unwrap();
    assert_eq!(body.value, None);
}

#[test]
fn class_body_members_inside_preprocessor_blocks_are_direct_members() {
    let parsed = parse_source(
        r#"
class Widget {
public:
  void run();
#ifdef WITH_EXTRA
  void extra();
#else
  void fallback();
#endif
private:
  int value_;
};
"#,
        Path::new("source.h"),
    )
    .unwrap();
    let class = parsed
        .root
        .children
        .iter()
        .find(|node| node.kind.as_ref() == "ClassDeclaration")
        .unwrap();
    let body = class
        .children
        .iter()
        .find(|node| node.kind.as_ref() == "ClassBody")
        .unwrap();
    let kinds = body
        .children
        .iter()
        .map(|node| node.kind.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            "FieldDeclaration",
            "FieldDeclaration",
            "FieldDeclaration",
            "FieldDeclaration"
        ]
    );
    let access = body
        .children
        .iter()
        .map(|node| match node.fields.get("access") {
            Some(ctc_core::canonical::CanonicalScalar::String(value)) => value.as_ref(),
            _ => "",
        })
        .collect::<Vec<_>>();
    assert_eq!(access, vec!["public", "public", "public", "private"]);
}

#[test]
fn macro_definitions_in_class_bodies_are_not_members() {
    let parsed = parse_source(
        "struct Table {\n#define FIELD(NAME) int NAME;\n  FIELDS(FIELD)\n#undef FIELD\n  int b;\n};\n",
        Path::new("source.h"),
    )
    .unwrap();
    let class = parsed
        .root
        .children
        .iter()
        .find(|node| node.kind.as_ref() == "StructDeclaration")
        .unwrap();
    let body = class
        .children
        .iter()
        .find(|node| node.kind.as_ref() == "ClassBody")
        .unwrap();
    assert_eq!(body.children.len(), 1);
    assert_eq!(body.children[0].kind.as_ref(), "FieldDeclaration");
}
