//! Decodes Lua string literals, so that `"a"`, `'a'`, and `[[a]]` have the
//! same canonical value.

use crate::lexer::{escape_end, long_bracket_open};

/// The largest value that `\u{...}` accepts.
const MAX_UTF8_ESCAPE: u32 = 0x7fff_ffff;

/// The decoded text of a string literal. Returns `None` when an escape
/// sequence is not valid or when the bytes are not UTF-8.
pub(crate) fn decode_string(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let decoded = match bytes.first()? {
        b'"' | b'\'' => decode_short(bytes.get(1..bytes.len().checked_sub(1)?)?)?,
        b'[' => decode_long(bytes)?,
        _ => return None,
    };
    String::from_utf8(decoded).ok()
}

/// Checks one escape sequence for the limits that Lua applies: a decimal
/// escape is at most 255, and a `\u{...}` escape is at most `0x7FFFFFFF`.
pub(crate) fn escape_is_valid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.first() == Some(&b'\\') && decode_escape(bytes, 0, &mut Vec::new()).is_some()
}

fn decode_long(bytes: &[u8]) -> Option<Vec<u8>> {
    let (level, length) = long_bracket_open(bytes, 0)?;
    let content = bytes.get(length..bytes.len().checked_sub(level + 2)?)?;
    let content = match content {
        [b'\r', b'\n', rest @ ..] | [b'\n', b'\r', rest @ ..] | [b'\r' | b'\n', rest @ ..] => rest,
        _ => content,
    };
    Some(normalize_line_breaks(content))
}

/// Lua reads `\r\n`, `\n\r`, `\n`, and `\r` as one line break each.
fn normalize_line_breaks(content: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(content.len());
    let mut index = 0;
    while index < content.len() {
        match (content[index], content.get(index + 1)) {
            (b'\r', Some(b'\n')) | (b'\n', Some(b'\r')) => {
                output.push(b'\n');
                index += 2;
            }
            (b'\r' | b'\n', _) => {
                output.push(b'\n');
                index += 1;
            }
            (byte, _) => {
                output.push(byte);
                index += 1;
            }
        }
    }
    output
}

fn decode_short(content: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(content.len());
    let mut index = 0;
    while index < content.len() {
        if content[index] == b'\\' {
            index = decode_escape(content, index, &mut output)?;
        } else {
            output.push(content[index]);
            index += 1;
        }
    }
    Some(output)
}

/// Decodes the escape sequence at `start` and returns the offset after it.
fn decode_escape(bytes: &[u8], start: usize, output: &mut Vec<u8>) -> Option<usize> {
    let next = start + 1;
    let simple = match bytes.get(next)? {
        b'a' => Some(0x07),
        b'b' => Some(0x08),
        b'f' => Some(0x0c),
        b'n' | b'\n' | b'\r' => Some(b'\n'),
        b'r' => Some(b'\r'),
        b't' => Some(b'\t'),
        b'v' => Some(0x0b),
        b'\\' => Some(b'\\'),
        b'"' => Some(b'"'),
        b'\'' => Some(b'\''),
        _ => None,
    };
    if let Some(byte) = simple {
        output.push(byte);
        return Some(escape_end(bytes, start));
    }
    match bytes[next] {
        b'z' => Some(escape_end(bytes, start)),
        b'x' => {
            let digits = bytes.get(next + 1..next + 3)?;
            output.push(u8::try_from(hex_value(digits)?).ok()?);
            Some(next + 3)
        }
        b'u' => decode_utf8_escape(bytes, next, output),
        byte if byte.is_ascii_digit() => {
            let end = (next..bytes.len().min(next + 3))
                .find(|at| !bytes[*at].is_ascii_digit())
                .unwrap_or(bytes.len().min(next + 3));
            let value = bytes[next..end]
                .iter()
                .fold(0u32, |value, digit| value * 10 + u32::from(digit - b'0'));
            output.push(u8::try_from(value).ok()?);
            Some(end)
        }
        _ => None,
    }
}

fn decode_utf8_escape(bytes: &[u8], u: usize, output: &mut Vec<u8>) -> Option<usize> {
    if bytes.get(u + 1) != Some(&b'{') {
        return None;
    }
    let close = (u + 2..bytes.len()).find(|at| bytes[*at] == b'}')?;
    let value = hex_value(&bytes[u + 2..close]).filter(|value| *value <= MAX_UTF8_ESCAPE)?;
    push_extended_utf8(value, output);
    Some(close + 1)
}

/// The value of one or more hexadecimal digits, or `None` for other text or
/// for a value above `u32::MAX`.
fn hex_value(digits: &[u8]) -> Option<u32> {
    if digits.is_empty() {
        return None;
    }
    digits.iter().try_fold(0u32, |value, digit| {
        let digit = char::from(*digit).to_digit(16)?;
        value.checked_mul(16)?.checked_add(digit)
    })
}

/// Encodes like `luaO_utf8esc`, which allows values up to `0x7FFFFFFF`.
fn push_extended_utf8(value: u32, output: &mut Vec<u8>) {
    if value < 0x80 {
        output.push(value as u8);
        return;
    }
    let mut continuation = Vec::new();
    let mut rest = value;
    let mut first_byte_limit = 0x3f_u32;
    while rest > first_byte_limit {
        continuation.push(0x80 | (rest & 0x3f) as u8);
        rest >>= 6;
        first_byte_limit >>= 1;
    }
    output.push(((!first_byte_limit << 1) | rest) as u8);
    output.extend(continuation.iter().rev());
}

#[cfg(test)]
mod tests;
