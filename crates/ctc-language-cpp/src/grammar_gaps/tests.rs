use std::path::Path;

use super::mask_grammar_gaps;
use crate::canonicalize::parse_source;

fn masked(source: &str) -> String {
    mask_grammar_gaps(source).unwrap_or_else(|| source.to_string())
}

#[test]
fn keeps_sources_without_grammar_gaps() {
    assert_eq!(mask_grammar_gaps("int run(int value = 1);"), None);
    assert_eq!(mask_grammar_gaps("int value = {};"), None);
    assert_eq!(mask_grammar_gaps("bool same = a == b;"), None);
}

#[test]
fn masks_brace_default_arguments_with_the_same_length() {
    let source = "void run(Options options = {}, Rect area = {1, {2}}, int x = { });";
    let result = masked(source);
    assert_eq!(result.len(), source.len());
    assert_eq!(
        result,
        "void run(Options options = 0 , Rect area = 0       , int x = 0  );"
    );
}

#[test]
fn keeps_line_breaks_and_designated_initializers() {
    let source = "void run(Limits limits = {.count = 1,\n  .depth = 2});\nsend(item, {.options = {.urgent = true}, .retry = 1});";
    let result = masked(source);
    assert_eq!(
        result,
        "void run(Limits limits = 0           \n             );\nsend(item, {.options = {.urgent = true}, .retry = 1});"
    );
}

#[test]
fn masks_lone_type_keywords_in_typeid() {
    assert_eq!(
        masked("auto& info = typeid(void);"),
        "auto& info = typeid(Void);"
    );
    assert_eq!(mask_grammar_gaps("auto& info = typeid(value);"), None);
}

#[test]
fn ignores_braces_in_comments_and_strings() {
    for source in [
        "// void run(int x = {});\nint a;",
        "/* void run(int x = {}); */ int a;",
        "const char* text = \"f(int x = {})\";",
        "const char* raw = R\"x(f(int y = {}))x\";",
    ] {
        assert_eq!(mask_grammar_gaps(source), None, "{source}");
    }
}

#[test]
fn parses_sources_that_use_the_masked_forms() {
    for source in [
        "struct Widget { void run(int value = {}); };",
        "Widget make(Rect area = {640,\n                         480});",
        "const auto& type = typeid(void);",
        "int value = 1'000; void run(char c = '{', int x = {});",
        "Status step(Limits limits = {.count = 64,\n                             .depth = 64});",
        "void run() { send(item, {.options = {.urgent = true, .offset = {2, 0, 1}}, .retry = 1}); }",
    ] {
        assert!(
            parse_source(source, Path::new("source.cpp")).is_ok(),
            "{source}"
        );
    }
}

#[test]
fn keeps_original_text_for_masked_nodes() {
    let source = "void run(int value = {});";
    let parsed = parse_source(source, Path::new("source.cpp")).unwrap();
    assert_eq!(parsed.source, source);
}

#[test]
fn blanks_lines_that_only_expand_an_x_macro() {
    let source = "struct Table {\n  ITEM_LIST(DECLARE_ITEM)\n  int value;\n};\n";
    let result = masked(source);
    assert_eq!(
        result,
        "struct Table {\n                         \n  int value;\n};\n"
    );
    assert!(parse_source(source, Path::new("source.h")).is_ok());
}

#[test]
fn keeps_macro_definitions_and_ordinary_calls() {
    for source in [
        "#define ITEM_LIST(X) \\\n  X(FIRST)\nint a;",
        "void run() { CHECK(VALUE); }",
        "void run() { check(value); }",
    ] {
        assert_eq!(mask_grammar_gaps(source), None, "{source}");
    }
}
