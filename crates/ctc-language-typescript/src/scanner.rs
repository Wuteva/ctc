use std::{ops::Range, path::Path};

use ctc_core::diagnostic::{Diagnostic, DiagnosticCategory, TextRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Code,
    LineComment,
    BlockComment,
    SingleString,
    DoubleString,
    Template,
    Regex,
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
    can_start_regex: bool,
    regex_character_class: bool,
    template_expression_depths: Vec<usize>,
}

impl<'a> ScanState<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            ranges: Vec::new(),
            diagnostics: Vec::new(),
            mode: Mode::Code,
            index: usize::from(source.starts_with('\u{feff}')) * '\u{feff}'.len_utf8(),
            can_start_regex: true,
            regex_character_class: false,
            template_expression_depths: Vec::new(),
        }
    }

    fn scan(mut self, source: &str, path: &Path) -> Result<Vec<Range<usize>>, Vec<Diagnostic>> {
        while self.index < self.bytes.len() {
            match self.mode {
                Mode::Code => self.step_code(source, path),
                Mode::LineComment => self.step_line_comment(),
                Mode::BlockComment => self.step_block_comment(),
                Mode::SingleString | Mode::DoubleString => self.step_string(),
                Mode::Template => self.step_template(),
                Mode::Regex => self.step_regex(),
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
            || self.handle_literal_start()
            || self.handle_template_expression_brace()
        {
            return;
        }
        match self.bytes[self.index] {
            byte if byte.is_ascii_whitespace() => self.index += 1,
            byte if is_identifier_start(byte) => self.scan_identifier(source),
            byte if byte.is_ascii_digit() => self.scan_number(),
            byte => {
                self.can_start_regex = !matches!(byte, b')' | b']');
                self.index += 1;
            }
        }
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
            self.can_start_regex = false;
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
            self.index += 2;
            true
        } else {
            false
        }
    }

    fn handle_literal_start(&mut self) -> bool {
        match self.bytes[self.index] {
            b'\'' => {
                self.mode = Mode::SingleString;
                self.index += 1;
                true
            }
            b'"' => {
                self.mode = Mode::DoubleString;
                self.index += 1;
                true
            }
            b'`' => {
                self.mode = Mode::Template;
                self.index += 1;
                true
            }
            b'/' if self.can_start_regex => {
                self.mode = Mode::Regex;
                self.regex_character_class = false;
                self.index += 1;
                true
            }
            b'+' | b'-'
                if self.index + 1 < self.bytes.len()
                    && self.bytes[self.index + 1] == self.bytes[self.index] =>
            {
                self.can_start_regex = false;
                self.index += 2;
                true
            }
            _ => false,
        }
    }

    fn handle_template_expression_brace(&mut self) -> bool {
        match self.bytes[self.index] {
            b'{' if !self.template_expression_depths.is_empty() => {
                if let Some(depth) = self.template_expression_depths.last_mut() {
                    *depth += 1;
                }
                self.index += 1;
                self.can_start_regex = true;
                true
            }
            b'}' if !self.template_expression_depths.is_empty() => {
                let Some(depth) = self.template_expression_depths.last_mut() else {
                    return false;
                };
                *depth = depth.saturating_sub(1);
                self.index += 1;
                if *depth == 0 {
                    self.template_expression_depths.pop();
                    self.mode = Mode::Template;
                } else {
                    self.can_start_regex = false;
                }
                true
            }
            _ => false,
        }
    }

    fn scan_identifier(&mut self, source: &str) {
        let start = self.index;
        self.index += 1;
        while self.index < self.bytes.len() && is_identifier_continue(self.bytes[self.index]) {
            self.index += 1;
        }
        let word = &source[start..self.index];
        self.can_start_regex = matches!(
            word,
            "return"
                | "throw"
                | "case"
                | "delete"
                | "void"
                | "typeof"
                | "instanceof"
                | "new"
                | "in"
                | "of"
                | "yield"
                | "await"
                | "else"
                | "do"
        );
    }

    fn scan_number(&mut self) {
        self.index += 1;
        while self.index < self.bytes.len()
            && (self.bytes[self.index].is_ascii_alphanumeric()
                || matches!(self.bytes[self.index], b'.' | b'_'))
        {
            self.index += 1;
        }
        self.can_start_regex = false;
    }

    fn step_line_comment(&mut self) {
        if matches!(self.bytes[self.index], b'\r' | b'\n') {
            self.mode = Mode::Code;
        }
        self.index += 1;
    }

    fn step_block_comment(&mut self) {
        if self.bytes[self.index..].starts_with(b"*/") {
            self.mode = Mode::Code;
            self.index += 2;
        } else {
            self.index += 1;
        }
    }

    fn step_string(&mut self) {
        let delimiter = if self.mode == Mode::SingleString {
            b'\''
        } else {
            b'"'
        };
        if self.bytes[self.index] == b'\\' {
            self.index = (self.index + 2).min(self.bytes.len());
        } else if self.bytes[self.index] == delimiter {
            self.mode = Mode::Code;
            self.can_start_regex = false;
            self.index += 1;
        } else {
            self.index += 1;
        }
    }

    fn step_template(&mut self) {
        if self.bytes[self.index] == b'\\' {
            self.index = (self.index + 2).min(self.bytes.len());
        } else if self.bytes[self.index] == b'`' {
            self.mode = Mode::Code;
            self.can_start_regex = false;
            self.index += 1;
        } else if self.bytes[self.index..].starts_with(b"${") {
            self.template_expression_depths.push(1);
            self.mode = Mode::Code;
            self.can_start_regex = true;
            self.index += 2;
        } else {
            self.index += 1;
        }
    }

    fn step_regex(&mut self) {
        if self.bytes[self.index] == b'\\' {
            self.index = (self.index + 2).min(self.bytes.len());
        } else if self.bytes[self.index] == b'[' {
            self.regex_character_class = true;
            self.index += 1;
        } else if self.bytes[self.index] == b']' {
            self.regex_character_class = false;
            self.index += 1;
        } else if self.bytes[self.index] == b'/' && !self.regex_character_class {
            self.mode = Mode::Code;
            self.index += 1;
            while self.index < self.bytes.len() && self.bytes[self.index].is_ascii_alphabetic() {
                self.index += 1;
            }
            self.can_start_regex = false;
        } else if matches!(self.bytes[self.index], b'\r' | b'\n') {
            self.mode = Mode::Code;
            self.index += 1;
        } else {
            self.index += 1;
        }
    }
}

fn is_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic()
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

#[cfg(test)]
mod tests;
