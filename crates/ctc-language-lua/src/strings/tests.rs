use super::{decode_string, escape_is_valid};

fn decoded(text: &str) -> Option<String> {
    decode_string(text)
}

#[test]
fn decodes_quoted_strings_the_same_way() {
    assert_eq!(decoded(r#""a'b""#).as_deref(), Some("a'b"));
    assert_eq!(decoded(r#"'a"b'"#).as_deref(), Some("a\"b"));
    assert_eq!(decoded(r#""""#).as_deref(), Some(""));
}

#[test]
fn decodes_simple_escapes() {
    assert_eq!(
        decoded(r#""\a\b\f\n\r\t\v\\\"\'""#).as_deref(),
        Some("\u{7}\u{8}\u{c}\n\r\t\u{b}\\\"'")
    );
}

#[test]
fn decodes_numeric_and_utf8_escapes() {
    assert_eq!(decoded(r#""\65\x42\u{43}""#).as_deref(), Some("ABC"));
    assert_eq!(decoded(r#""\0659""#).as_deref(), Some("A9"));
    assert_eq!(decoded(r#""\u{1F600}""#).as_deref(), Some("\u{1F600}"));
    assert_eq!(decoded(r#""\u{E9}""#).as_deref(), Some("\u{e9}"));
}

#[test]
fn decodes_escaped_line_breaks_and_z() {
    assert_eq!(decoded("\"a\\\nb\"").as_deref(), Some("a\nb"));
    assert_eq!(decoded("\"a\\\r\nb\"").as_deref(), Some("a\nb"));
    assert_eq!(decoded("\"a\\z\n    b\"").as_deref(), Some("ab"));
}

#[test]
fn decodes_long_strings() {
    assert_eq!(decoded("[[a]]").as_deref(), Some("a"));
    assert_eq!(decoded("[==[a]]b]==]").as_deref(), Some("a]]b"));
    assert_eq!(decoded("[[\nline]]").as_deref(), Some("line"));
    assert_eq!(
        decoded("[[\r\nline\r\nnext\rlast]]").as_deref(),
        Some("line\nnext\nlast")
    );
    assert_eq!(decoded("[[\\n]]").as_deref(), Some("\\n"));
}

#[test]
fn rejects_escapes_that_lua_rejects() {
    assert_eq!(decoded(r#""\256""#), None);
    assert_eq!(decoded(r#""\x4""#), None);
    assert_eq!(decoded(r#""\x+1""#), None);
    assert_eq!(decoded(r#""\u{80000000}""#), None);
    assert_eq!(decoded(r#""\u{}""#), None);
    assert_eq!(decoded(r#""\q""#), None);
}

#[test]
fn rejects_bytes_that_are_not_utf8() {
    assert_eq!(decoded(r#""\xff""#), None);
}

#[test]
fn checks_escape_limits() {
    assert!(escape_is_valid("\\255"));
    assert!(escape_is_valid("\\u{7FFFFFFF}"));
    assert!(!escape_is_valid("\\256"));
    assert!(!escape_is_valid("\\u{80000000}"));
    assert!(!escape_is_valid("x"));
}
