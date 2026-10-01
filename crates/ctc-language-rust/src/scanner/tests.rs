use std::path::Path;

use super::scan_placeholders;

#[test]
fn ignores_rust_comments_and_literals() {
    let source = r##"
// {{ line }}
/* nested {{ block /* still */ }} */
let text = "{{ string }}";
let raw = r#"{{ raw }}"#;
let bytes = b"{{ bytes }}";
let byte_raw = br#"{{ raw bytes }}"#;
let ch = '{';
let byte = b'{';
{{ Capture }}
"##;
    let ranges = scan_placeholders(source, Path::new("rule.rs.ctmpl")).unwrap();
    assert_eq!(ranges.len(), 1);
    assert_eq!(&source[ranges[0].clone()], "{{ Capture }}");
}

#[test]
fn treats_lifetimes_as_code_but_characters_as_literals() {
    let source = "fn f<'a>(value: &'a str) { let _ = 'a'; {{ Capture }} }\n";
    let ranges = scan_placeholders(source, Path::new("rule.rs.ctmpl")).unwrap();
    assert_eq!(ranges.len(), 1);
    assert_eq!(&source[ranges[0].clone()], "{{ Capture }}");
}

#[test]
fn reports_an_unclosed_placeholder() {
    let diagnostics =
        scan_placeholders("fn f() { {{ Name\n}\n", Path::new("rule.rs.ctmpl")).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC2001");
}
