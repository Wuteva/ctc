use std::{path::Path, sync::Arc};

use crate::{
    diagnostic::Diagnostic,
    engine::ParsedSource,
    template::{CompiledTemplate, Placeholder},
};

pub trait LanguageAdapter: Send + Sync {
    fn id(&self) -> &'static str;

    fn supports_path(&self, _path: &Path) -> bool {
        false
    }

    fn supports_selector_suffix(&self, _suffix: &str) -> bool {
        false
    }

    fn scan_placeholders(
        &self,
        _source: &str,
        _path: &Path,
    ) -> Result<Vec<std::ops::Range<usize>>, Vec<Diagnostic>> {
        Ok(Vec::new())
    }

    fn parse(&self, _source: &str, _path: &Path) -> Result<ParsedSource, Vec<Diagnostic>> {
        Err(vec![Diagnostic::new(
            "CTC9001",
            crate::diagnostic::DiagnosticCategory::InternalToolError,
            "The language adapter does not implement source parsing.",
            2,
        )])
    }

    fn compile_template(
        &self,
        _source: &str,
        _path: &Path,
        _placeholders: &[Placeholder],
        _mode: crate::matcher::MatchMode,
    ) -> Result<CompiledTemplate, Vec<Diagnostic>> {
        Err(vec![Diagnostic::new(
            "CTC9001",
            crate::diagnostic::DiagnosticCategory::InternalToolError,
            "The language adapter does not implement template compilation.",
            2,
        )])
    }

    fn validate_identifier(&self, _value: &str) -> bool {
        false
    }

    fn known_kind(&self, _value: &str) -> bool {
        false
    }

    fn known_field(&self, _value: &str) -> bool {
        false
    }

    fn known_keyword(&self, _value: &str) -> bool {
        false
    }
}

#[derive(Default)]
pub struct LanguageRegistry {
    adapters: Vec<Arc<dyn LanguageAdapter>>,
}

impl LanguageRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, adapter: Arc<dyn LanguageAdapter>) {
        self.adapters.push(adapter);
    }

    pub fn for_path(&self, path: &Path) -> Option<Arc<dyn LanguageAdapter>> {
        self.adapters
            .iter()
            .find(|adapter| adapter.supports_path(path))
            .cloned()
    }

    pub fn for_selector_suffix(&self, suffix: &str) -> Option<Arc<dyn LanguageAdapter>> {
        self.adapters
            .iter()
            .find(|adapter| adapter.supports_selector_suffix(suffix))
            .cloned()
    }

    pub fn by_id(&self, id: &str) -> Option<Arc<dyn LanguageAdapter>> {
        self.adapters
            .iter()
            .find(|adapter| adapter.id() == id)
            .cloned()
    }
}
