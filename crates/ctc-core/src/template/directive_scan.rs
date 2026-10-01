use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirectiveScanMode {
    Code,
    BlockComment,
    SingleString,
    DoubleString,
    Template,
    Regex,
}

pub(super) struct DirectiveScan {
    pub(super) line_comments: Vec<Range<usize>>,
    pub(super) first_token: Option<usize>,
}

pub(super) fn scan_directive_comments(source: &str) -> DirectiveScan {
    DirectiveScanner::new(source).scan()
}

struct DirectiveScanner<'a> {
    source: &'a str,
    bytes: &'a [u8],
    line_comments: Vec<Range<usize>>,
    first_token: Option<usize>,
    mode: DirectiveScanMode,
    index: usize,
    can_start_regex: bool,
    regex_character_class: bool,
    template_depths: Vec<usize>,
}

impl<'a> DirectiveScanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            line_comments: Vec::new(),
            first_token: None,
            mode: DirectiveScanMode::Code,
            index: usize::from(source.starts_with('\u{feff}')) * '\u{feff}'.len_utf8(),
            can_start_regex: true,
            regex_character_class: false,
            template_depths: Vec::new(),
        }
    }

    fn scan(mut self) -> DirectiveScan {
        while self.index < self.bytes.len() {
            match self.mode {
                DirectiveScanMode::Code => self.advance_code(),
                DirectiveScanMode::BlockComment => self.advance_block_comment(),
                DirectiveScanMode::SingleString | DirectiveScanMode::DoubleString => {
                    self.advance_string()
                }
                DirectiveScanMode::Template => self.advance_template(),
                DirectiveScanMode::Regex => self.advance_regex(),
            }
        }
        DirectiveScan {
            line_comments: self.line_comments,
            first_token: self.first_token,
        }
    }

    fn advance_code(&mut self) {
        if self.bytes[self.index..].starts_with(b"//") {
            let end = self.source[self.index..]
                .find(['\r', '\n'])
                .map_or(self.source.len(), |offset| self.index + offset);
            self.line_comments.push(self.index..end);
            self.index = end;
            return;
        }
        if self.bytes[self.index..].starts_with(b"/*") {
            self.mode = DirectiveScanMode::BlockComment;
            self.index += 2;
            return;
        }
        if self.bytes[self.index].is_ascii_whitespace() {
            self.index += 1;
            return;
        }
        self.first_token.get_or_insert(self.index);
        match self.bytes[self.index] {
            b'\'' => self.enter_string(DirectiveScanMode::SingleString),
            b'"' => self.enter_string(DirectiveScanMode::DoubleString),
            b'`' => self.enter_string(DirectiveScanMode::Template),
            b'/' if self.can_start_regex => {
                self.mode = DirectiveScanMode::Regex;
                self.regex_character_class = false;
                self.index += 1;
            }
            b'+' | b'-'
                if self.index + 1 < self.bytes.len()
                    && self.bytes[self.index + 1] == self.bytes[self.index] =>
            {
                self.can_start_regex = false;
                self.index += 2;
            }
            b'{' if !self.template_depths.is_empty() => self.open_template_interpolation(),
            b'}' if !self.template_depths.is_empty() => self.close_template_interpolation(),
            byte if byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$') => {
                self.advance_identifier();
            }
            byte => {
                self.can_start_regex = !matches!(byte, b')' | b']');
                self.index += 1;
            }
        }
    }

    fn advance_identifier(&mut self) {
        let start = self.index;
        self.index += 1;
        while self.index < self.bytes.len()
            && (self.bytes[self.index].is_ascii_alphanumeric()
                || matches!(self.bytes[self.index], b'_' | b'$'))
        {
            self.index += 1;
        }
        self.can_start_regex = matches!(
            &self.source[start..self.index],
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

    fn advance_block_comment(&mut self) {
        if self.bytes[self.index..].starts_with(b"*/") {
            self.mode = DirectiveScanMode::Code;
            self.index += 2;
        } else {
            self.index += 1;
        }
    }

    fn advance_string(&mut self) {
        let delimiter = if self.mode == DirectiveScanMode::SingleString {
            b'\''
        } else {
            b'"'
        };
        if self.bytes[self.index] == b'\\' {
            self.index = (self.index + 2).min(self.bytes.len());
        } else if self.bytes[self.index] == delimiter {
            self.mode = DirectiveScanMode::Code;
            self.can_start_regex = false;
            self.index += 1;
        } else {
            self.index += 1;
        }
    }

    fn advance_template(&mut self) {
        if self.bytes[self.index] == b'\\' {
            self.index = (self.index + 2).min(self.bytes.len());
        } else if self.bytes[self.index] == b'`' {
            self.mode = DirectiveScanMode::Code;
            self.can_start_regex = false;
            self.index += 1;
        } else if self.bytes[self.index..].starts_with(b"${") {
            self.template_depths.push(1);
            self.mode = DirectiveScanMode::Code;
            self.can_start_regex = true;
            self.index += 2;
        } else {
            self.index += 1;
        }
    }

    fn advance_regex(&mut self) {
        if self.bytes[self.index] == b'\\' {
            self.index = (self.index + 2).min(self.bytes.len());
        } else if self.bytes[self.index] == b'[' {
            self.regex_character_class = true;
            self.index += 1;
        } else if self.bytes[self.index] == b']' {
            self.regex_character_class = false;
            self.index += 1;
        } else if self.bytes[self.index] == b'/' && !self.regex_character_class {
            self.mode = DirectiveScanMode::Code;
            self.index += 1;
            while self.index < self.bytes.len() && self.bytes[self.index].is_ascii_alphabetic() {
                self.index += 1;
            }
            self.can_start_regex = false;
        } else if matches!(self.bytes[self.index], b'\r' | b'\n') {
            self.mode = DirectiveScanMode::Code;
            self.index += 1;
        } else {
            self.index += 1;
        }
    }

    fn enter_string(&mut self, mode: DirectiveScanMode) {
        self.mode = mode;
        self.index += 1;
    }

    fn open_template_interpolation(&mut self) {
        if let Some(depth) = self.template_depths.last_mut() {
            *depth += 1;
        }
        self.can_start_regex = true;
        self.index += 1;
    }

    fn close_template_interpolation(&mut self) {
        let Some(depth) = self.template_depths.last_mut() else {
            return;
        };
        *depth = depth.saturating_sub(1);
        self.index += 1;
        if *depth == 0 {
            self.template_depths.pop();
            self.mode = DirectiveScanMode::Template;
        } else {
            self.can_start_regex = false;
        }
    }
}
