use std::path::Path;

use ctc_core::LanguageAdapter;

use super::LuaAdapter;

#[test]
fn supports_lua_source_and_template_suffixes() {
    let adapter = LuaAdapter::new();
    assert!(adapter.supports_path(Path::new("scripts/part.lua")));
    assert!(adapter.supports_selector_suffix("rule.lua"));
    assert!(adapter.supports_selector_suffix("rule.lua.ctmpl"));
    assert!(!adapter.supports_path(Path::new("part.luac")));
    assert!(!adapter.supports_path(Path::new("lib.rs")));
    assert!(!adapter.supports_selector_suffix("rule.rs.ctmpl"));
}

#[test]
fn validates_lua_names_and_keywords() {
    let adapter = LuaAdapter::new();
    assert!(adapter.validate_identifier("Widget2"));
    assert!(adapter.validate_identifier("_ENV"));
    assert!(adapter.validate_identifier("global"));
    assert!(!adapter.validate_identifier("2Widget"));
    assert!(!adapter.validate_identifier("end"));
    assert!(!adapter.validate_identifier("café"));
    assert!(adapter.known_keyword("local"));
    assert!(adapter.known_keyword("global"));
    assert!(!adapter.known_keyword("function"));
}

#[test]
fn accepts_lua_kinds_and_fields() {
    let adapter = LuaAdapter::new();
    for kind in [
        "SourceFile",
        "FunctionDeclaration",
        "FunctionExpression",
        "StatementBlock",
        "CallExpression",
        "VariableDeclaration",
        "ImplicitVariableDeclaration",
        "AssignmentStatement",
        "ReturnStatement",
        "TableConstructor",
        "Identifier",
        "StringLiteral",
        "NumericLiteral",
    ] {
        assert!(adapter.known_kind(kind), "{kind}");
    }
    for kind in ["Statement", "Expression", "Declaration", "ClassBody"] {
        assert!(!adapter.known_kind(kind), "{kind}");
    }
    for field in ["callee", "module", "local", "global"] {
        assert!(adapter.known_field(field), "{field}");
    }
    assert!(!adapter.known_field("public"));
}
