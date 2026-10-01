#![forbid(unsafe_code)]

mod canonicalize;
mod compiler;
mod modules;
mod scanner;
mod semantic;

use std::path::Path;

use ctc_core::{
    diagnostic::Diagnostic,
    engine::ParsedSource,
    language::LanguageAdapter,
    matcher::MatchMode,
    template::{CompiledTemplate, Placeholder},
};

#[derive(Debug, Default)]
pub struct TypeScriptAdapter;

impl TypeScriptAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageAdapter for TypeScriptAdapter {
    fn id(&self) -> &'static str {
        "typescript"
    }

    fn supports_path(&self, path: &Path) -> bool {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        !is_declaration_file(name)
            && matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("ts" | "mts" | "cts")
            )
    }

    fn supports_selector_suffix(&self, suffix: &str) -> bool {
        !is_declaration_file(suffix)
            && (suffix.ends_with(".ts") || suffix.ends_with(".mts") || suffix.ends_with(".cts"))
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
        (first == '_' || first == '$' || first.is_alphabetic())
            && characters.all(|character| {
                character == '_'
                    || character == '$'
                    || character.is_alphanumeric()
                    || character == '\u{200c}'
                    || character == '\u{200d}'
            })
    }

    fn known_kind(&self, value: &str) -> bool {
        canonicalize::known_kind(value)
    }

    fn known_field(&self, value: &str) -> bool {
        value == "typeOnly" || ctc_core::template::MEMBER_FIELDS.contains(&value)
    }

    fn known_keyword(&self, value: &str) -> bool {
        matches!(
            value,
            "abstract"
                | "async"
                | "declare"
                | "default"
                | "export"
                | "override"
                | "private"
                | "protected"
                | "public"
                | "readonly"
                | "static"
        )
    }
}

fn is_declaration_file(name: &str) -> bool {
    name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts")
}

#[cfg(test)]
mod tests;
