use std::{ops::Range, path::Path};

use ctc_core::diagnostic::{Diagnostic, DiagnosticCategory, TextRange};

use crate::lexer::{
    comment_end, first_line_comment, long_bracket_end, long_bracket_open, short_string_end,
};

/// Finds placeholder candidates outside comments, string literals, and a first
/// line that starts with `#`.
pub fn scan_placeholders(source: &str, path: &Path) -> Result<Vec<Range<usize>>, Vec<Diagnostic>> {
    let bytes = source.as_bytes();
    let start = usize::from(source.starts_with('\u{feff}')) * '\u{feff}'.len_utf8();
    let mut ranges = Vec::new();
    let mut diagnostics = Vec::new();
    let mut index = first_line_comment(bytes, start).unwrap_or(start);
    while index < bytes.len() {
        if bytes[index..].starts_with(b"{{") {
            index = placeholder(source, path, index, &mut ranges, &mut diagnostics);
        } else if bytes[index..].starts_with(b"--") {
            index = comment_end(bytes, index);
        } else if let Some((level, length)) = long_bracket_open(bytes, index) {
            index = long_bracket_end(bytes, index + length, level);
        } else if matches!(bytes[index], b'"' | b'\'') {
            index = short_string_end(bytes, index);
        } else {
            index += 1;
        }
    }
    if diagnostics.is_empty() {
        Ok(ranges)
    } else {
        Err(diagnostics)
    }
}

fn placeholder(
    source: &str,
    path: &Path,
    index: usize,
    ranges: &mut Vec<Range<usize>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    let line_end = source[index..]
        .find(['\r', '\n'])
        .map_or(source.len(), |offset| index + offset);
    if let Some(close) = source[index + 2..line_end].find("}}") {
        let end = index + 2 + close + 2;
        ranges.push(index..end);
        return end;
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
    line_end
}

#[cfg(test)]
mod tests;
