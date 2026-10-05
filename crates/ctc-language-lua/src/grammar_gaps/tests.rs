use std::path::Path;

use super::{mask_grammar_gaps, valid_name, valid_numeral};
use crate::canonicalize::parse_source;

fn parse_messages(source: &str) -> Vec<String> {
    match parse_source(source, Path::new("source.lua")) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .map(|diagnostic| format!("{} {}", diagnostic.code, diagnostic.message))
            .collect(),
    }
}

#[test]
fn keeps_sources_without_grammar_gaps() {
    assert_eq!(mask_grammar_gaps("local s = \"a\\\nb\"\n"), None);
    assert_eq!(mask_grammar_gaps("local s = 'x' -- \\\r\n"), None);
    assert_eq!(mask_grammar_gaps("local s = [[\\\r\n]]\r\n"), None);
}

#[test]
fn masks_a_backslash_before_a_carriage_return_with_the_same_length() {
    let source = "local s = \"a\\\r\nb\"\r\nlocal t = 'c\\\rd'\n";
    let masked = mask_grammar_gaps(source).unwrap();
    assert_eq!(masked.len(), source.len());
    assert_eq!(masked, "local s = \"a\\z\nb\"\r\nlocal t = 'c\\zd'\n");
}

#[test]
fn parses_and_decodes_a_string_with_an_escaped_crlf_line_break() {
    let parsed = parse_source("local s = \"a\\\r\nb\"\r\n", Path::new("source.lua")).unwrap();
    assert_eq!(parsed.source, "local s = \"a\\\r\nb\"\r\n");
    let mut values = Vec::new();
    collect_string_values(&parsed.root, &mut values);
    assert_eq!(values, vec!["a\nb".to_string()]);
}

fn collect_string_values(node: &ctc_core::canonical::CanonicalNode, values: &mut Vec<String>) {
    if node.kind.as_ref() == "StringLiteral"
        && let Some(value) = &node.value
    {
        values.push(value.display());
    }
    for child in &node.children {
        collect_string_values(child, values);
    }
}

#[test]
fn reports_names_that_lua_rejects() {
    for source in ["local a$b = 1\n", "local x = a!b\n", "local café = 1\n"] {
        assert_eq!(
            parse_messages(source),
            vec!["CTC3001 The source contains an invalid Lua name."],
            "{source}"
        );
    }
}

#[test]
fn reports_numbers_that_lua_rejects() {
    for source in [
        "local n = 1LL\n",
        "local n = 0x10ULL\n",
        "local n = 1i\n",
        "local n = 0b101\n",
    ] {
        assert_eq!(
            parse_messages(source),
            vec!["CTC3001 The source contains a malformed Lua number."],
            "{source}"
        );
    }
}

#[test]
fn reports_line_breaks_and_escapes_that_lua_rejects() {
    assert_eq!(
        parse_messages("local s = \"a\nb\"\n"),
        vec!["CTC3001 The source contains an unfinished Lua string."]
    );
    assert_eq!(
        parse_messages("local s = \"\\256\"\n"),
        vec!["CTC3001 The source contains an invalid Lua escape sequence."]
    );
    assert_eq!(
        parse_messages("local s = \"\\u{80000000}\"\n"),
        vec!["CTC3001 The source contains an invalid Lua escape sequence."]
    );
}

#[test]
fn accepts_valid_names_and_numerals() {
    for name in ["x", "_G", "camelCase2", "__index"] {
        assert!(valid_name(name), "{name}");
    }
    for numeral in [
        "3", "3.", ".5", "3.14", "1e10", "1E+5", "2e-3", "0xff", "0XA", "0x.1p-2", "0xA.8P0",
        "0x1p4",
    ] {
        assert!(valid_numeral(numeral), "{numeral}");
    }
    for numeral in ["0x", "1e", "1..2", "0x1e+", "1f"] {
        assert!(!valid_numeral(numeral), "{numeral}");
    }
}
