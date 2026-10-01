use super::*;

#[test]
fn ignores_cpp_comments_and_literals() {
    let source = r#"
// {{ Comment }}
const char* text = "{{ String }}";
const char value = '{';
const char* raw = R"tag({{ RawString }})tag";
{{ Capture }}
"#;
    let ranges = scan_placeholders(source, Path::new("rule.cpp.ctmpl")).unwrap();
    let values = ranges
        .iter()
        .map(|range| &source[range.clone()])
        .collect::<Vec<_>>();
    assert_eq!(values, ["{{ Capture }}"]);
}

#[test]
fn scans_placeholders_in_preprocessor_directives() {
    let source = "#define VALUE {{ Value }}\n";
    let ranges = scan_placeholders(source, Path::new("rule.hpp.ctmpl")).unwrap();
    assert_eq!(&source[ranges[0].clone()], "{{ Value }}");
}

#[test]
fn reports_an_unclosed_placeholder() {
    let diagnostics = scan_placeholders("int {{ Name;\n", Path::new("rule.cpp.ctmpl")).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC2001");
}
