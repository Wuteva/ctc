use super::*;

#[test]
fn collects_comments_but_not_comment_text_inside_strings() {
    let source = "// one\nconst text = \"// not a comment\";\n/* two */\nconst other = `/* no */ ${value /* three */}`;\n";
    let parsed = parse_source(source, Path::new("source.ts")).unwrap();
    let comments = parsed
        .comments
        .iter()
        .map(|range| &source[range.clone()])
        .collect::<Vec<_>>();
    assert_eq!(comments, vec!["// one", "/* two */", "/* three */"]);
}

#[test]
fn canonicalizes_import_type_field() {
    let parsed = parse_source(
        "import type { ILogger } from './interfaces';",
        Path::new("source.ts"),
    )
    .unwrap();
    let import = &parsed.root.children[0];
    assert_eq!(import.kind.as_ref(), "ImportDeclaration");
    assert_eq!(
        import.fields.get("typeOnly"),
        Some(&CanonicalScalar::Bool(true))
    );
}

#[test]
fn recognizes_type_import_without_space_before_brace() {
    let parsed = parse_source(
        "import type{ ILogger } from './interfaces';",
        Path::new("source.ts"),
    )
    .unwrap();
    assert_eq!(
        parsed.root.children[0].fields.get("typeOnly"),
        Some(&CanonicalScalar::Bool(true))
    );
}

#[test]
fn recognizes_type_import_after_comment() {
    let parsed = parse_source(
        "import /* gap */ type { ILogger } from './interfaces';",
        Path::new("source.ts"),
    )
    .unwrap();
    assert_eq!(
        parsed.root.children[0].fields.get("typeOnly"),
        Some(&CanonicalScalar::Bool(true))
    );
}

#[test]
fn default_binding_named_type_is_a_value_import() {
    let parsed = parse_source("import type from './types';", Path::new("source.ts")).unwrap();
    assert_eq!(
        parsed.root.children[0].fields.get("typeOnly"),
        Some(&CanonicalScalar::Bool(false))
    );
}

#[test]
fn type_only_named_specifier_is_not_a_type_only_declaration() {
    let parsed = parse_source(
        "import { type ILogger } from './interfaces';",
        Path::new("source.ts"),
    )
    .unwrap();
    assert_eq!(
        parsed.root.children[0].fields.get("typeOnly"),
        Some(&CanonicalScalar::Bool(false))
    );
}

fn type_only_field(source: &str, index: usize) -> Option<CanonicalScalar> {
    let parsed = parse_source(source, Path::new("source.ts")).unwrap();
    parsed.root.children[index].fields.get("typeOnly").cloned()
}

#[test]
fn marks_type_declarations_and_type_exports_as_type_only() {
    let source = r#"
interface Local {}
type Alias = string;
export interface Exported {}
export type ExportedAlias = number;
export type { Other } from "./other";
export const value = 1;
export class Value {}
export { value as renamed };
"#;
    let expected = [true, true, true, true, true, false, false, false];
    for (index, expected) in expected.into_iter().enumerate() {
        assert_eq!(
            type_only_field(source, index),
            Some(CanonicalScalar::Bool(expected)),
            "statement {index}"
        );
    }
}

#[test]
fn rejects_invalid_source() {
    assert!(parse_source("const =", Path::new("source.ts")).is_err());
}

#[test]
fn reports_each_parse_error() {
    let diagnostics =
        parse_source("const first =;\nconst second =;", Path::new("source.ts")).unwrap_err();
    assert!(diagnostics.len() >= 2);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "CTC3001")
    );
}

#[test]
fn accepts_utf8_byte_order_mark() {
    assert!(parse_source("\u{feff}export {};", Path::new("source.ts")).is_ok());
}

#[test]
fn decodes_equivalent_string_literals() {
    let plain = parse_source("const value = 'a';", Path::new("plain.ts")).unwrap();
    let escaped = parse_source(r#"const value = "\x61";"#, Path::new("escaped.ts")).unwrap();
    assert!(plain.root.structurally_eq(&escaped.root));
}

#[test]
fn accepts_type_only_namespace_reexport() {
    let source = r#"export type * as Types from "./types";"#;
    assert!(parse_source(source, Path::new("source.ts")).is_ok());
}

#[test]
fn decodes_unicode_surrogate_pairs() {
    let escaped =
        parse_source(r#"const value = "\uD83D\uDE00";"#, Path::new("escaped.ts")).unwrap();
    let unicode = parse_source("const value = \"😀\";", Path::new("unicode.ts")).unwrap();
    let empty = parse_source("const value = \"\";", Path::new("empty.ts")).unwrap();
    assert!(escaped.root.structurally_eq(&unicode.root));
    assert!(!escaped.root.structurally_eq(&empty.root));
}

#[test]
fn normalizes_template_literal_line_endings() {
    let crlf = parse_source("const value = `a\r\nb`;", Path::new("crlf.ts")).unwrap();
    let lf = parse_source("const value = `a\nb`;", Path::new("lf.ts")).unwrap();
    assert!(crlf.root.structurally_eq(&lf.root));
}

#[test]
fn kind_registry_exposes_only_canonical_names() {
    assert!(known_kind("ImportDeclaration"));
    assert!(!known_kind("ImportStatement"));
    assert!(known_kind("SourceFile"));
    assert!(!known_kind("Program"));
}

fn class_members(source: &str) -> Vec<CanonicalNode> {
    let parsed = parse_source(source, Path::new("source.ts")).unwrap();
    let class = &parsed.root.children[0];
    let body = class
        .children
        .iter()
        .find(|node| node.kind.as_ref() == "ClassBody")
        .unwrap();
    body.children.clone()
}

fn field(node: &CanonicalNode, name: &str) -> CanonicalScalar {
    node.fields.get(name).cloned().unwrap()
}

#[test]
fn exposes_class_member_fields() {
    let members = class_members(
        "class A {\n  constructor() {}\n  static constructor() {}\n  run() {}\n  protected p = 1;\n  private q = 2;\n  #r = 3;\n  static { }\n}",
    );
    let text = |value: &str| CanonicalScalar::String(Arc::from(value));
    let expected = [
        ("public", true, false),
        ("public", false, true),
        ("public", false, false),
        ("protected", false, false),
        ("private", false, false),
        ("private", false, false),
        ("public", false, true),
    ];
    assert_eq!(members.len(), expected.len());
    for (member, (access, constructor, is_static)) in members.iter().zip(expected) {
        assert_eq!(field(member, "access"), text(access), "{member:?}");
        assert_eq!(
            field(member, "constructor"),
            CanonicalScalar::Bool(constructor)
        );
        assert_eq!(field(member, "static"), CanonicalScalar::Bool(is_static));
    }
}

#[test]
fn moves_method_decorators_into_the_method() {
    let members = class_members("class A {\n  @Get()\n  @Auth() run() {}\n  @Inject() dep = 1;\n}");
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].kind.as_ref(), "MethodDefinition");
    assert_eq!(members[0].children[0].kind.as_ref(), "Decorator");
    assert_eq!(members[0].children[1].kind.as_ref(), "Decorator");
    assert_eq!(members[1].children[0].kind.as_ref(), "Decorator");
}

#[test]
fn empty_class_body_has_no_value() {
    let parsed = parse_source("class A {}", Path::new("source.ts")).unwrap();
    let body = parsed.root.children[0]
        .children
        .iter()
        .find(|node| node.kind.as_ref() == "ClassBody")
        .unwrap();
    assert_eq!(body.value, None);
    assert!(body.children.is_empty());
}
