//! Masks valid C++ forms that the bundled tree-sitter-cpp grammar rejects.
//!
//! Every replacement writes the same number of ASCII bytes, so node offsets
//! still point into the original source and node text comes from it.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mask {
    Keep,
    Set(u8),
}

/// Returns `None` when the source holds none of the masked forms.
pub(crate) fn mask_grammar_gaps(source: &str) -> Option<String> {
    let bytes = source.as_bytes();
    let code = code_bytes(bytes);
    let mut masks = vec![Mask::Keep; bytes.len()];
    let mut changed = false;
    for index in 0..bytes.len() {
        if !code[index] {
            continue;
        }
        changed |= mask_brace_default(bytes, &code, index, &mut masks);
        changed |= mask_typeid_keyword(bytes, &code, index, &mut masks);
    }
    changed |= mask_macro_expansions(bytes, &code, &mut masks);
    if !changed {
        return None;
    }
    let masked = bytes
        .iter()
        .zip(&masks)
        .map(|(byte, mask)| match mask {
            Mask::Keep => *byte,
            Mask::Set(value) => *value,
        })
        .collect::<Vec<_>>();
    String::from_utf8(masked).ok()
}

/// `int value = {}` and `Rect area = {1, 2}` in parameter lists. The grammar
/// accepts only an expression as a default argument, so the braces become the
/// literal `0` and blanks. Line breaks stay, so line numbers do not move.
fn mask_brace_default(bytes: &[u8], code: &[bool], index: usize, masks: &mut [Mask]) -> bool {
    if bytes[index] != b'=' || !plain_assignment(bytes, index) || designator(bytes, index) {
        return false;
    }
    let Some(open) = next_code(bytes, code, index + 1).filter(|at| bytes[*at] == b'{') else {
        return false;
    };
    let Some(close) = matching_brace(bytes, code, open) else {
        return false;
    };
    let ends_parameter =
        next_code(bytes, code, close + 1).is_some_and(|at| bytes[at] == b',' || bytes[at] == b')');
    if !ends_parameter {
        return false;
    }
    masks[open] = Mask::Set(b'0');
    for at in open + 1..=close {
        if bytes[at] != b'\n' {
            masks[at] = Mask::Set(b' ');
        }
    }
    true
}

fn plain_assignment(bytes: &[u8], index: usize) -> bool {
    let next_is_equal = bytes.get(index + 1) == Some(&b'=');
    let previous_is_operator = index > 0 && b"=!<>+-*/%&|^".contains(&bytes[index - 1]);
    !next_is_equal && !previous_is_operator
}

/// `.field = {...}` inside a designated initializer is already valid.
fn designator(bytes: &[u8], equal: usize) -> bool {
    let name_end = (0..equal)
        .rev()
        .find(|at| !bytes[*at].is_ascii_whitespace())
        .map_or(0, |at| at + 1);
    let name_start = (0..name_end)
        .rev()
        .find(|at| !identifier_byte(bytes[*at]))
        .map_or(0, |at| at + 1);
    name_start < name_end
        && (0..name_start)
            .rev()
            .find(|at| !bytes[*at].is_ascii_whitespace())
            .is_some_and(|at| bytes[at] == b'.')
}

/// A line that only expands an X-macro, such as `ITEM_LIST(DECLARE)`, has
/// no semicolon, which the grammar rejects. Such lines become blank.
fn mask_macro_expansions(bytes: &[u8], code: &[bool], masks: &mut [Mask]) -> bool {
    let mut changed = false;
    let mut continued = false;
    let mut start = 0;
    while start < bytes.len() {
        let end = line_end(bytes, start);
        let line = &bytes[start..end];
        let trimmed = line.trim_ascii();
        let first = start + (line.len() - line.trim_ascii_start().len());
        if !continued && first < end && code[first] && macro_expansion(trimmed) {
            for mask in &mut masks[start..end] {
                *mask = Mask::Set(b' ');
            }
            changed = true;
        }
        continued = trimmed.last() == Some(&b'\\');
        start = end + 1;
    }
    changed
}

fn macro_expansion(line: &[u8]) -> bool {
    let Some(open) = line.iter().position(|byte| *byte == b'(') else {
        return false;
    };
    line.len() > open + 1
        && line.last() == Some(&b')')
        && macro_name(&line[..open])
        && macro_name(&line[open + 1..line.len() - 1])
}

fn macro_name(text: &[u8]) -> bool {
    text.first().is_some_and(u8::is_ascii_uppercase)
        && text
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// `typeid(void)`: the grammar expects an expression, so a lone type keyword
/// becomes an identifier of the same length.
fn mask_typeid_keyword(bytes: &[u8], code: &[bool], index: usize, masks: &mut [Mask]) -> bool {
    const KEYWORDS: &[&str] = &[
        "void", "bool", "char", "short", "int", "long", "float", "double",
    ];
    if !word_at(bytes, index, "typeid") {
        return false;
    }
    let Some(open) = next_code(bytes, code, index + 6).filter(|at| bytes[*at] == b'(') else {
        return false;
    };
    let Some(start) = next_code(bytes, code, open + 1) else {
        return false;
    };
    let Some(keyword) = KEYWORDS
        .iter()
        .find(|keyword| word_at(bytes, start, keyword))
    else {
        return false;
    };
    let end = start + keyword.len();
    if !next_code(bytes, code, end).is_some_and(|at| bytes[at] == b')') {
        return false;
    }
    masks[start] = Mask::Set(bytes[start].to_ascii_uppercase());
    true
}

fn word_at(bytes: &[u8], index: usize, word: &str) -> bool {
    let end = index + word.len();
    let starts_word = index == 0 || !identifier_byte(bytes[index - 1]);
    let ends_word = bytes.get(end).is_none_or(|byte| !identifier_byte(*byte));
    starts_word && ends_word && bytes.get(index..end) == Some(word.as_bytes())
}

fn identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn next_code(bytes: &[u8], code: &[bool], from: usize) -> Option<usize> {
    (from..bytes.len()).find(|at| code[*at] && !bytes[*at].is_ascii_whitespace())
}

fn matching_brace(bytes: &[u8], code: &[bool], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for index in open..bytes.len() {
        if !code[index] {
            continue;
        }
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            b';' => return None,
            _ => {}
        }
    }
    None
}

/// Marks each byte that is outside comments, string literals and character
/// literals.
fn code_bytes(bytes: &[u8]) -> Vec<bool> {
    let mut code = vec![true; bytes.len()];
    let mut index = 0;
    while index < bytes.len() {
        let end = match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => line_end(bytes, index),
            b'/' if bytes.get(index + 1) == Some(&b'*') => block_comment_end(bytes, index),
            b'"' if raw_string_prefix(bytes, index) => raw_string_end(bytes, index),
            b'"' => quoted_end(bytes, index, b'"'),
            b'\'' if !digit_separator(bytes, index) => quoted_end(bytes, index, b'\''),
            _ => {
                index += 1;
                continue;
            }
        };
        code[index..end].fill(false);
        index = end.max(index + 1);
    }
    code
}

fn line_end(bytes: &[u8], from: usize) -> usize {
    (from..bytes.len())
        .find(|at| bytes[*at] == b'\n')
        .unwrap_or(bytes.len())
}

fn block_comment_end(bytes: &[u8], from: usize) -> usize {
    (from + 2..bytes.len().saturating_sub(1))
        .find(|at| bytes[*at] == b'*' && bytes[*at + 1] == b'/')
        .map_or(bytes.len(), |at| at + 2)
}

fn quoted_end(bytes: &[u8], from: usize, quote: u8) -> usize {
    let mut index = from + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'\n' => return index,
            byte if byte == quote => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn digit_separator(bytes: &[u8], index: usize) -> bool {
    index > 0
        && bytes[index - 1].is_ascii_alphanumeric()
        && bytes.get(index + 1).is_some_and(u8::is_ascii_alphanumeric)
        && bytes.get(index + 2) != Some(&b'\'')
}

fn raw_string_prefix(bytes: &[u8], quote: usize) -> bool {
    let start = (0..quote)
        .rev()
        .find(|at| !identifier_byte(bytes[*at]))
        .map_or(0, |at| at + 1);
    matches!(&bytes[start..quote], b"R" | b"LR" | b"uR" | b"UR" | b"u8R")
}

fn raw_string_end(bytes: &[u8], quote: usize) -> usize {
    let Some(open) = (quote + 1..bytes.len()).find(|at| bytes[*at] == b'(') else {
        return bytes.len();
    };
    let delimiter = &bytes[quote + 1..open];
    let mut index = open + 1;
    while index < bytes.len() {
        if bytes[index] == b')'
            && bytes[index + 1..].starts_with(delimiter)
            && bytes.get(index + 1 + delimiter.len()) == Some(&b'"')
        {
            return index + delimiter.len() + 2;
        }
        index += 1;
    }
    bytes.len()
}

#[cfg(test)]
mod tests;
