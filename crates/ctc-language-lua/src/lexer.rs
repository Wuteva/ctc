//! Lexical helpers that the placeholder scanner and the grammar-gap masks
//! share. They follow the Lua 5.5 lexer (`llex.c`).

/// The level of a long bracket that opens at `index`, such as 0 for `[[` and
/// 2 for `[==[`. Returns the level and the length of the opening bracket.
pub(crate) fn long_bracket_open(bytes: &[u8], index: usize) -> Option<(usize, usize)> {
    if bytes.get(index) != Some(&b'[') {
        return None;
    }
    let mut cursor = index + 1;
    while bytes.get(cursor) == Some(&b'=') {
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'[')).then_some((cursor - index - 1, cursor + 1 - index))
}

/// The offset after the long bracket of `level` that closes at or after
/// `from`, or the end of the input when the bracket does not close.
pub(crate) fn long_bracket_end(bytes: &[u8], from: usize, level: usize) -> usize {
    let mut index = from;
    while index < bytes.len() {
        if bytes[index] == b']' {
            let equals_end = index + 1 + level;
            if bytes
                .get(index + 1..equals_end)
                .is_some_and(|run| run.iter().all(|byte| *byte == b'='))
                && bytes.get(equals_end) == Some(&b']')
            {
                return equals_end + 1;
            }
        }
        index += 1;
    }
    bytes.len()
}

/// The offset after a short string that starts with the quote at `start`. A
/// line break that no backslash escapes ends the string early, as in Lua.
pub(crate) fn short_string_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = escape_end(bytes, index),
            b'\n' | b'\r' => return index,
            byte if byte == quote => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

/// The offset after the escape sequence that starts with the backslash at
/// `start`. Only the forms that can hold a line break need care: a backslash
/// before a line break, and `\z`, which skips the white space after it.
pub(crate) fn escape_end(bytes: &[u8], start: usize) -> usize {
    let next = start + 1;
    match (bytes.get(next), bytes.get(next + 1)) {
        (Some(b'\r'), Some(b'\n')) | (Some(b'\n'), Some(b'\r')) => next + 2,
        (Some(b'z'), _) => {
            let mut index = next + 1;
            while bytes.get(index).is_some_and(|byte| is_lua_space(*byte)) {
                index += 1;
            }
            index
        }
        _ => (next + 1).min(bytes.len()),
    }
}

/// White space as `lisspace` in `lctype.c` defines it.
pub(crate) fn is_lua_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// The offset of the line break that ends the line at `from`, or the end of
/// the input.
pub(crate) fn line_end(bytes: &[u8], from: usize) -> usize {
    (from..bytes.len())
        .find(|at| matches!(bytes[*at], b'\n' | b'\r'))
        .unwrap_or(bytes.len())
}

/// The offset after a comment that starts with `--` at `start`.
pub(crate) fn comment_end(bytes: &[u8], start: usize) -> usize {
    match long_bracket_open(bytes, start + 2) {
        Some((level, length)) => long_bracket_end(bytes, start + 2 + length, level),
        None => line_end(bytes, start),
    }
}

/// The length of a first line that starts with `#`. Lua skips such a line, for
/// example `#!/usr/bin/env lua`.
pub(crate) fn first_line_comment(bytes: &[u8], start: usize) -> Option<usize> {
    (bytes.get(start) == Some(&b'#')).then(|| line_end(bytes, start))
}

#[cfg(test)]
mod tests;
