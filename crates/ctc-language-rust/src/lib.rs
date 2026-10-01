#![forbid(unsafe_code)]

mod canonicalize;
mod compiler;
mod members;
mod scanner;

use std::path::Path;

use ctc_core::{
    diagnostic::Diagnostic,
    engine::ParsedSource,
    language::LanguageAdapter,
    matcher::MatchMode,
    template::{CompiledTemplate, Placeholder},
};

const EXTENSIONS: &[&str] = &["rs"];
const KEYWORDS: &[&str] = &[
    "async", "const", "default", "extern", "mut", "pub", "unsafe",
];

#[derive(Debug, Default)]
pub struct RustAdapter;

impl RustAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageAdapter for RustAdapter {
    fn id(&self) -> &'static str {
        "rust"
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| EXTENSIONS.contains(&extension))
    }

    fn supports_selector_suffix(&self, suffix: &str) -> bool {
        suffix.ends_with(".rs") || suffix.ends_with(".rs.ctmpl")
    }

    fn scan_placeholders(
        &self,
        source: &str,
        path: &Path,
    ) -> Result<Vec<std::ops::Range<usize>>, Vec<Diagnostic>> {
        scanner::scan_placeholders(source, path)
    }

    fn parse(&self, source: &str, path: &Path) -> Result<ParsedSource, Vec<Diagnostic>> {
        canonicalize::parse_source(source, path)
    }

    fn compile_template(
        &self,
        source: &str,
        path: &Path,
        placeholders: &[Placeholder],
        mode: MatchMode,
    ) -> Result<CompiledTemplate, Vec<Diagnostic>> {
        compiler::compile_template(source, path, placeholders, mode)
    }

    fn validate_identifier(&self, value: &str) -> bool {
        let mut characters = value.chars();
        let Some(first) = characters.next() else {
            return false;
        };
        (first == '_' || first.is_alphabetic())
            && characters.all(|character| character == '_' || character.is_alphanumeric())
    }

    fn known_kind(&self, value: &str) -> bool {
        canonicalize::known_kind(value)
    }

    fn known_field(&self, value: &str) -> bool {
        members::is_known_field(value)
    }

    fn known_keyword(&self, value: &str) -> bool {
        KEYWORDS.contains(&value)
    }
}

#[cfg(test)]
mod tests;
