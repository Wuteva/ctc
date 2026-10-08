#![forbid(unsafe_code)]

mod canonicalize;
mod compiler;
mod fields;
mod globals;
mod grammar_gaps;
mod lexer;
mod scanner;
mod strings;

use std::path::Path;

use ctc_core::{
    diagnostic::Diagnostic,
    engine::ParsedSource,
    language::LanguageAdapter,
    matcher::MatchMode,
    template::{CompiledTemplate, Placeholder},
};

const EXTENSIONS: &[&str] = &["lua"];
const KEYWORDS: &[&str] = &["global", "local"];

/// Reserved words of Lua 5.5. `global` is not here: Lua 5.5 builds with
/// `LUA_COMPAT_GLOBAL` by default, so `global` can still be a name.
const RESERVED_WORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

#[derive(Debug, Default)]
pub struct LuaAdapter;

impl LuaAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageAdapter for LuaAdapter {
    fn id(&self) -> &'static str {
        "lua"
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| EXTENSIONS.contains(&extension))
    }

    fn supports_selector_suffix(&self, suffix: &str) -> bool {
        suffix.ends_with(".lua") || suffix.ends_with(".lua.ctmpl")
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
        grammar_gaps::valid_name(value) && !RESERVED_WORDS.contains(&value)
    }

    fn known_kind(&self, value: &str) -> bool {
        canonicalize::known_kind(value)
    }

    fn known_field(&self, value: &str) -> bool {
        fields::is_known_field(value)
    }

    fn known_keyword(&self, value: &str) -> bool {
        KEYWORDS.contains(&value)
    }
}

#[cfg(test)]
mod tests;
