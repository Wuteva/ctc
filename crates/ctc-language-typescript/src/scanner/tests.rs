use super::*;

#[test]
fn ignores_protected_regions() {
    let source = r#"
// {{ Comment }}
const a = "{{ String }}";
const b = /{{ Regex }}/;
const c = `{{ Template }} ${ {{ Expression }} }`;
{{ Capture }}
"#;
    let ranges = scan_placeholders(source, Path::new("rule.ts.ctmpl")).unwrap();
    let values: Vec<_> = ranges.iter().map(|range| &source[range.clone()]).collect();
    assert_eq!(values, ["{{ Expression }}", "{{ Capture }}"]);
}

#[test]
fn treats_regex_after_instanceof_as_protected() {
    let source = r#"
const result = value instanceof /{{ NotCapture }}/;
{{ Capture }}
"#;
    let ranges = scan_placeholders(source, Path::new("rule.ts.ctmpl")).unwrap();
    assert_eq!(ranges.len(), 1);
    assert_eq!(&source[ranges[0].clone()], "{{ Capture }}");
}

#[test]
fn scans_placeholder_after_postfix_division() {
    let source = "const result = value++ / {{ Divisor }};";
    let ranges = scan_placeholders(source, Path::new("rule.ts.ctmpl")).unwrap();
    assert_eq!(ranges.len(), 1);
    assert_eq!(&source[ranges[0].clone()], "{{ Divisor }}");
}
