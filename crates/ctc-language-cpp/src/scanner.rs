use std::{ops::Range, path::Path};

use ctc_core::diagnostic::{Diagnostic, DiagnosticCategory, TextRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Code,
    LineComment,
    BlockComment,
    String,
    Character,
}

pub fn scan_placeholders(source: &str, path: &Path) -> Result<Vec<Range<usize>>, Vec<Diagnostic>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut diagnostics = Vec::new();
    let mut mode = Mode::Code;
    let mut index = usize::from(source.starts_with('\u{feff}')) * '\u{feff}'.len_utf8();

    while index < bytes.len() {
        match mode {
            Mode::Code => {
                if bytes[index..].starts_with(b"{{") {
                    let line_end = source[index..]
                        .find(['\r', '\n'])
                        .map_or(bytes.len(), |offset| index + offset);
                    if let Some(close) = source[index + 2..line_end].find("}}") {
                        let end = index + 2 + close + 2;
                        ranges.push(index..end);
                        index = end;
                        continue;
                    }
                    diagnostics.push(
                        Diagnostic::new(
                            "CTC2001",
                            DiagnosticCategory::InvalidTemplateSyntax,
                            "The placeholder is missing `}}` on the same line.",
                            2,
                        )
                        .with_template(TextRange::from_offsets(path, source, index, line_end)),
                    );
                    index = line_end;
                    continue;
                }
                if bytes[index..].starts_with(b"//") {
                    mode = Mode::LineComment;
                    index += 2;
                    continue;
                }
                if bytes[index..].starts_with(b"/*") {
                    mode = Mode::BlockComment;
                    index += 2;
                    continue;
                }
                if let Some(end) = raw_string_end(source, index) {
                    index = end;
                    continue;
                }
                match bytes[index] {
                    b'"' => {
                        mode = Mode::String;
                        index += 1;
                    }
                    b'\'' => {
                        mode = Mode::Character;
                        index += 1;
                    }
                    _ => index += 1,
                }
            }
            Mode::LineComment => {
                if matches!(bytes[index], b'\r' | b'\n') {
                    mode = Mode::Code;
                }
                index += 1;
            }
            Mode::BlockComment => {
                if bytes[index..].starts_with(b"*/") {
                    mode = Mode::Code;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            Mode::String | Mode::Character => {
                let delimiter = if mode == Mode::String { b'"' } else { b'\'' };
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == delimiter {
                    mode = Mode::Code;
                    index += 1;
                } else {
                    index += 1;
                }
            }
        }
    }

    if diagnostics.is_empty() {
        Ok(ranges)
    } else {
        Err(diagnostics)
    }
}

fn raw_string_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let prefix_length = [b"u8R\"".as_slice(), b"uR\"", b"UR\"", b"LR\"", b"R\""]
        .into_iter()
        .find(|prefix| bytes[start..].starts_with(prefix))
        .map(<[u8]>::len)?;
    let delimiter_start = start + prefix_length;
    let relative_open = bytes[delimiter_start..]
        .iter()
        .take(17)
        .position(|byte| *byte == b'(')?;
    if relative_open > 16 {
        return None;
    }
    let open = delimiter_start + relative_open;
    let delimiter = &source[delimiter_start..open];
    if delimiter
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || matches!(byte, b'\\' | b'(' | b')'))
    {
        return None;
    }
    let close = format!("){delimiter}\"");
    source[open + 1..]
        .find(&close)
        .map(|offset| open + 1 + offset + close.len())
        .or(Some(source.len()))
}

#[cfg(test)]
mod tests;
