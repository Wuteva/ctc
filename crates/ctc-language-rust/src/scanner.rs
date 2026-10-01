use std::{ops::Range, path::Path};

use ctc_core::diagnostic::{Diagnostic, DiagnosticCategory, TextRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Code,
    LineComment,
    BlockComment,
    String,
    RawString,
    Character,
}

pub fn scan_placeholders(source: &str, path: &Path) -> Result<Vec<Range<usize>>, Vec<Diagnostic>> {
    ScanState::new(source).scan(source, path)
}

struct ScanState<'a> {
    bytes: &'a [u8],
    ranges: Vec<Range<usize>>,
    diagnostics: Vec<Diagnostic>,
    mode: Mode,
    index: usize,
    block_depth: usize,
    raw_hashes: usize,
}

impl<'a> ScanState<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            ranges: Vec::new(),
            diagnostics: Vec::new(),
            mode: Mode::Code,
            index: usize::from(source.starts_with('\u{feff}')) * '\u{feff}'.len_utf8(),
            block_depth: 0,
            raw_hashes: 0,
        }
    }

    fn scan(mut self, source: &str, path: &Path) -> Result<Vec<Range<usize>>, Vec<Diagnostic>> {
        while self.index < self.bytes.len() {
            match self.mode {
                Mode::Code => self.step_code(source, path),
                Mode::LineComment => self.step_line_comment(),
                Mode::BlockComment => self.step_block_comment(),
                Mode::String => self.step_string(),
                Mode::RawString => self.step_raw_string(),
                Mode::Character => self.step_character(),
            }
        }
        if self.diagnostics.is_empty() {
            Ok(self.ranges)
        } else {
            Err(self.diagnostics)
        }
    }

    fn step_code(&mut self, source: &str, path: &Path) {
        if self.handle_placeholder(source, path)
            || self.handle_comment_start()
            || self.handle_raw_string_start()
            || self.handle_string_start()
            || self.handle_character_start()
        {
            return;
        }
        self.index += 1;
    }

    fn handle_placeholder(&mut self, source: &str, path: &Path) -> bool {
        if !self.bytes[self.index..].starts_with(b"{{") {
            return false;
        }
        let line_end = source[self.index..]
            .find(['\r', '\n'])
            .map_or(self.bytes.len(), |offset| self.index + offset);
        if let Some(close) = source[self.index + 2..line_end].find("}}") {
            let end = self.index + 2 + close + 2;
            self.ranges.push(self.index..end);
            self.index = end;
            return true;
        }
        self.diagnostics.push(
            Diagnostic::new(
                "CTC2001",
                DiagnosticCategory::InvalidTemplateSyntax,
                "The placeholder is missing `}}` on the same line.",
                2,
            )
            .with_template(TextRange::from_offsets(path, source, self.index, line_end)),
        );
        self.index = line_end;
        true
    }

    fn handle_comment_start(&mut self) -> bool {
        if self.bytes[self.index..].starts_with(b"//") {
            self.mode = Mode::LineComment;
            self.index += 2;
            true
        } else if self.bytes[self.index..].starts_with(b"/*") {
            self.mode = Mode::BlockComment;
            self.block_depth = 1;
            self.index += 2;
            true
        } else {
            false
        }
    }

    fn handle_raw_string_start(&mut self) -> bool {
        let Some((prefix, hashes)) = raw_string_start(self.bytes, self.index) else {
            return false;
        };
        self.mode = Mode::RawString;
        self.raw_hashes = hashes;
        self.index += prefix;
        true
    }

    fn handle_string_start(&mut self) -> bool {
        if self.bytes[self.index..].starts_with(b"b\"") {
            self.mode = Mode::String;
            self.index += 2;
            return true;
        }
        if self.bytes[self.index] == b'"' {
            self.mode = Mode::String;
            self.index += 1;
            return true;
        }
        false
    }

    fn handle_character_start(&mut self) -> bool {
        if self.bytes[self.index..].starts_with(b"b'") {
            self.mode = Mode::Character;
            self.index += 2;
            return true;
        }
        if self.bytes[self.index] == b'\'' && !is_lifetime_start(self.bytes, self.index) {
            self.mode = Mode::Character;
            self.index += 1;
            return true;
        }
        false
    }

    fn step_line_comment(&mut self) {
        if matches!(self.bytes[self.index], b'\r' | b'\n') {
            self.mode = Mode::Code;
        }
        self.index += 1;
    }

    fn step_block_comment(&mut self) {
        if self.bytes[self.index..].starts_with(b"/*") {
            self.block_depth += 1;
            self.index += 2;
            return;
        }
        if self.bytes[self.index..].starts_with(b"*/") {
            self.block_depth -= 1;
            self.index += 2;
            if self.block_depth == 0 {
                self.mode = Mode::Code;
            }
            return;
        }
        self.index += 1;
    }

    fn step_string(&mut self) {
        if self.bytes[self.index] == b'\\' {
            self.index = (self.index + 2).min(self.bytes.len());
        } else if self.bytes[self.index] == b'"' {
            self.mode = Mode::Code;
            self.index += 1;
        } else {
            self.index += 1;
        }
    }

    fn step_raw_string(&mut self) {
        if self.bytes[self.index] != b'"' {
            self.index += 1;
            return;
        }
        let hashes_end = self.index + 1 + self.raw_hashes;
        if hashes_end <= self.bytes.len()
            && self.bytes[self.index + 1..hashes_end]
                .iter()
                .all(|byte| *byte == b'#')
        {
            self.mode = Mode::Code;
            self.index = hashes_end;
        } else {
            self.index += 1;
        }
    }

    fn step_character(&mut self) {
        if self.bytes[self.index] == b'\\' {
            self.index = (self.index + 2).min(self.bytes.len());
        } else if self.bytes[self.index] == b'\'' {
            self.mode = Mode::Code;
            self.index += 1;
        } else {
            self.index += 1;
        }
    }
}

fn raw_string_start(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let prefix = if bytes[start..].starts_with(b"br") {
        2
    } else if bytes[start..].starts_with(b"r") {
        1
    } else {
        return None;
    };
    let mut index = start + prefix;
    while index < bytes.len() && bytes[index] == b'#' {
        index += 1;
    }
    (index < bytes.len() && bytes[index] == b'"')
        .then_some((index + 1 - start, index - start - prefix))
}

fn is_lifetime_start(bytes: &[u8], start: usize) -> bool {
    let Some(next) = bytes.get(start + 1).copied() else {
        return false;
    };
    if !is_identifier_start(next) {
        return false;
    }
    let mut index = start + 2;
    while index < bytes.len() && is_identifier_continue(bytes[index]) {
        index += 1;
    }
    bytes.get(index).copied() != Some(b'\'')
}

fn is_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_identifier_continue(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests;
