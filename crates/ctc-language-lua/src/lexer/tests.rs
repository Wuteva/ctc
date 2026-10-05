use super::{
    comment_end, first_line_comment, long_bracket_end, long_bracket_open, short_string_end,
};

#[test]
fn reads_long_bracket_levels() {
    assert_eq!(long_bracket_open(b"[[x]]", 0), Some((0, 2)));
    assert_eq!(long_bracket_open(b"[==[x]==]", 0), Some((2, 4)));
    assert_eq!(long_bracket_open(b"[=x", 0), None);
    assert_eq!(long_bracket_open(b"[1]", 0), None);
}

#[test]
fn finds_the_matching_long_bracket_level() {
    let text = b"[==[ a ]] b ]=] c ]==] d";
    assert_eq!(long_bracket_end(text, 4, 2), 22);
    assert_eq!(long_bracket_end(b"[[ open", 2, 0), 7);
}

#[test]
fn ends_short_strings_at_the_quote_or_a_raw_line_break() {
    assert_eq!(short_string_end(br#""a\"b" x"#, 0), 6);
    assert_eq!(short_string_end(b"'a\\\nb' x", 0), 6);
    assert_eq!(short_string_end(b"\"a\nb\"", 0), 2);
    assert_eq!(short_string_end(b"\"open", 0), 5);
}

#[test]
fn keeps_escaped_line_breaks_inside_short_strings() {
    assert_eq!(short_string_end(b"\"a\\\r\nb\" x", 0), 7);
    assert_eq!(short_string_end(b"\"a\\\n\rb\" x", 0), 7);
    assert_eq!(short_string_end(b"\"a\\z\n   b\" x", 0), 10);
    assert_eq!(short_string_end(b"\"a\\", 0), 3);
}

#[test]
fn ends_line_and_long_comments() {
    assert_eq!(comment_end(b"-- x\ny", 0), 4);
    assert_eq!(comment_end(b"--[[ a\n b ]] y", 0), 12);
    assert_eq!(comment_end(b"--[==[ ]] ]==] y", 0), 14);
    assert_eq!(comment_end(b"--[= not long\ny", 0), 13);
}

#[test]
fn skips_only_a_first_line_hash() {
    assert_eq!(first_line_comment(b"#!/usr/bin/lua\nprint(1)", 0), Some(14));
    assert_eq!(first_line_comment(b"print(#t)", 0), None);
}
