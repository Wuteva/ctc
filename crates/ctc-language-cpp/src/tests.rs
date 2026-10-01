use std::path::Path;

use ctc_core::LanguageAdapter;

use super::CppAdapter;

#[test]
fn supports_common_cpp_source_and_header_suffixes() {
    let adapter = CppAdapter::new();
    for path in [
        "source.cpp",
        "source.cc",
        "source.cxx",
        "source.h",
        "source.hh",
        "source.hpp",
        "source.hxx",
    ] {
        assert!(adapter.supports_path(Path::new(path)), "{path}");
    }
    assert!(!adapter.supports_path(Path::new("source.c")));
    assert!(!adapter.supports_path(Path::new("source.ts")));
}

#[test]
fn validates_cpp_identifiers_and_keywords() {
    let adapter = CppAdapter::new();
    assert!(adapter.validate_identifier("Widget_2"));
    assert!(adapter.validate_identifier("_private"));
    assert!(!adapter.validate_identifier("$extension"));
    assert!(!adapter.validate_identifier("2Widget"));
    assert!(adapter.known_keyword("constexpr"));
    assert!(adapter.known_keyword("noexcept"));
    assert!(!adapter.known_keyword("sometimes"));
}

#[test]
fn accepts_cpp_member_field_names() {
    let adapter = CppAdapter::new();
    for name in [
        "access",
        "constructor",
        "static",
        "destructor",
        "virtual",
        "pure",
        "override",
        "final",
        "operator",
        "friend",
        "deleted",
        "defaulted",
        "const",
    ] {
        assert!(adapter.known_field(name), "{name}");
    }
    assert!(!adapter.known_field("typeOnly"));
    assert!(!adapter.known_field("inline"));
}
