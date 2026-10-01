use std::path::Path;

use ctc_core::LanguageAdapter;

use super::TypeScriptAdapter;

#[test]
fn rejects_all_typescript_declaration_suffixes() {
    let adapter = TypeScriptAdapter::new();
    assert!(!adapter.supports_path(Path::new("types.d.ts")));
    assert!(!adapter.supports_path(Path::new("types.d.mts")));
    assert!(!adapter.supports_path(Path::new("types.d.cts")));
}
