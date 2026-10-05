//! Closes the gaps between the bundled tree-sitter-lua grammar and the Lua
//! 5.5 lexer.
//!
//! The grammar rejects one valid form: a backslash before a carriage return in
//! a short string. A mask replaces that carriage return with `z`, which has the
//! same length, so node offsets still point into the original source and node
//! text comes from it.
//!
//! The grammar accepts some forms that Lua rejects: names with characters
//! such as `$` or `!`, number suffixes such as `LL` or `i`, binary numbers,
//! line breaks inside a short string, and escapes out of range. A walk over the
//! syntax tree reports these.

use std::ops::Range;

use tree_sitter::Node;

use crate::{
    lexer::{comment_end, escape_end, first_line_comment, long_bracket_end, long_bracket_open},
    strings::escape_is_valid,
};

/// Returns `None` when the source holds none of the masked forms.
pub(crate) fn mask_grammar_gaps(source: &str) -> Option<String> {
    let bytes = source.as_bytes();
    let mut masked: Option<Vec<u8>> = None;
    let start = usize::from(source.starts_with('\u{feff}')) * '\u{feff}'.len_utf8();
    let mut index = first_line_comment(bytes, start).unwrap_or(start);
    while index < bytes.len() {
        if bytes[index..].starts_with(b"--") {
            index = comment_end(bytes, index);
        } else if let Some((level, length)) = long_bracket_open(bytes, index) {
            index = long_bracket_end(bytes, index + length, level);
        } else if matches!(bytes[index], b'"' | b'\'') {
            index = mask_short_string(bytes, index, &mut masked);
        } else {
            index += 1;
        }
    }
    masked.and_then(|bytes| String::from_utf8(bytes).ok())
}

/// Masks the carriage return after each backslash in the short string at
/// `start`, and returns the offset after the string.
fn mask_short_string(bytes: &[u8], start: usize, masked: &mut Option<Vec<u8>>) -> usize {
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                if bytes.get(index + 1) == Some(&b'\r') {
                    masked.get_or_insert_with(|| bytes.to_vec())[index + 1] = b'z';
                }
                index = escape_end(bytes, index);
            }
            b'\n' | b'\r' => return index,
            byte if byte == quote => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

/// A form that the grammar accepts and Lua 5.5 rejects.
pub(crate) struct LexicalError {
    pub(crate) range: Range<usize>,
    pub(crate) message: &'static str,
}

pub(crate) fn lexical_errors(root: Node<'_>, source: &str) -> Vec<LexicalError> {
    let mut errors = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if let Some(message) = node_error(node, source) {
            errors.push(LexicalError {
                range: node.start_byte()..node.end_byte(),
                message,
            });
        }
        if node.kind() != "comment" && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return errors;
            }
        }
    }
}

fn node_error(node: Node<'_>, source: &str) -> Option<&'static str> {
    let text = node.utf8_text(source.as_bytes()).unwrap_or_default();
    match node.kind() {
        "identifier" if !valid_name(text) => Some("The source contains an invalid Lua name."),
        "number" if !valid_numeral(text) => Some("The source contains a malformed Lua number."),
        "string" if unfinished_short_string(text) => {
            Some("The source contains an unfinished Lua string.")
        }
        "escape_sequence" if !escape_is_valid(text) => {
            Some("The source contains an invalid Lua escape sequence.")
        }
        _ => None,
    }
}

/// A Lua name: an ASCII letter or underscore, then ASCII letters, digits, or
/// underscores.
pub(crate) fn valid_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes
        .next()
        .is_some_and(|first| first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

/// A decimal or hexadecimal numeral as `luaO_str2num` reads it.
fn valid_numeral(text: &str) -> bool {
    let (body, is_digit, exponent): (&str, fn(&u8) -> bool, &[u8]) = match text.get(..2) {
        Some("0x" | "0X") => (&text[2..], u8::is_ascii_hexdigit, b"pP"),
        _ => (text, u8::is_ascii_digit, b"eE"),
    };
    let bytes = body.as_bytes();
    let mantissa_end = bytes
        .iter()
        .position(|byte| exponent.contains(byte))
        .unwrap_or(bytes.len());
    let mantissa = &bytes[..mantissa_end];
    let dots = mantissa.iter().filter(|byte| **byte == b'.').count();
    let digits = mantissa.iter().filter(|byte| is_digit(byte)).count();
    if dots > 1 || digits == 0 || digits + dots != mantissa.len() {
        return false;
    }
    let Some(exponent_digits) = bytes.get(mantissa_end + 1..) else {
        return true;
    };
    let exponent_digits = exponent_digits
        .strip_prefix(b"+")
        .or_else(|| exponent_digits.strip_prefix(b"-"))
        .unwrap_or(exponent_digits);
    !exponent_digits.is_empty() && exponent_digits.iter().all(u8::is_ascii_digit)
}

/// True when a quoted string holds a line break that no backslash escapes.
fn unfinished_short_string(text: &str) -> bool {
    let bytes = text.as_bytes();
    matches!(bytes.first(), Some(b'"' | b'\''))
        && crate::lexer::short_string_end(bytes, 0) != bytes.len()
}

#[cfg(test)]
mod tests;
