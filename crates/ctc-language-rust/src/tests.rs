use std::path::Path;

use ctc_core::LanguageAdapter;

use super::RustAdapter;

#[test]
fn supports_rust_source_and_template_suffixes() {
    let adapter = RustAdapter::new();
    assert!(adapter.supports_path(Path::new("lib.rs")));
    assert!(adapter.supports_selector_suffix("rule.rs"));
    assert!(adapter.supports_selector_suffix("rule.rs.ctmpl"));
    assert!(!adapter.supports_path(Path::new("lib.ts")));
    assert!(!adapter.supports_selector_suffix("rule.ts.ctmpl"));
}

#[test]
fn validates_rust_identifiers_and_keywords() {
    let adapter = RustAdapter::new();
    assert!(adapter.validate_identifier("Widget2"));
    assert!(adapter.validate_identifier("_private"));
    assert!(!adapter.validate_identifier("2Widget"));
    assert!(!adapter.validate_identifier("r#type"));
    assert!(adapter.known_keyword("async"));
    assert!(adapter.known_keyword("pub"));
    assert!(!adapter.known_keyword("inline"));
}

#[test]
fn accepts_rust_field_names() {
    let adapter = RustAdapter::new();
    for name in [
        "callee",
        "public",
        "visibility",
        "async",
        "unsafe",
        "const",
        "mutable",
    ] {
        assert!(adapter.known_field(name), "{name}");
    }
    assert!(!adapter.known_field("typeOnly"));
}
