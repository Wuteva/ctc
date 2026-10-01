use std::path::Path;

use crate::language::LanguageAdapter;

use super::*;

struct TestAdapter;

impl LanguageAdapter for TestAdapter {
    fn id(&self) -> &'static str {
        "test"
    }

    fn known_kind(&self, value: &str) -> bool {
        value == "ImportDeclaration"
    }

    fn known_field(&self, value: &str) -> bool {
        value == "typeOnly"
    }
}

#[test]
fn parses_capture_and_filters() {
    let source =
        "{{* Imports | kind(\"ImportDeclaration\") | field(\"typeOnly\", \"notEqual\", true) }}";
    let ranges = std::iter::once(0..source.len()).collect::<Vec<_>>();
    let placeholders =
        parse_placeholders(source, Path::new("rule.ts.ctmpl"), &ranges, &TestAdapter).unwrap();
    let placeholder = &placeholders[0];
    assert_eq!(placeholder.name, "Imports");
    assert_eq!(placeholder.cardinality, Cardinality::Sequence);
    assert_eq!(placeholder.filters.len(), 2);
}

#[test]
fn parses_exclusion_directive() {
    let directives = parse_directives("// ctc: exclude\n", Path::new("index.ts.ctmpl")).unwrap();
    assert!(directives.excluded);
}

#[test]
fn rejects_body_in_exclusion() {
    let result = parse_directives("// ctc: exclude\nexport {};\n", Path::new("index.ts.ctmpl"));
    assert!(result.is_err());
}

#[test]
fn ignores_directive_text_inside_a_string() {
    let source = "const first = 1;\nconst second = \"// ctc: mode=forbid\";\n";
    let directives = parse_directives(source, Path::new("rule.ts.ctmpl")).unwrap();
    assert_eq!(directives.mode, MatchMode::Exact);
}

#[test]
fn ignores_directive_text_inside_multiline_template_literal() {
    let source = "const value = `first\n// ctc: mode=forbid\nlast`;\n";
    let directives = parse_directives(source, Path::new("rule.ts.ctmpl")).unwrap();
    assert_eq!(directives.mode, MatchMode::Exact);
}

#[test]
fn rejects_standalone_directive_after_source_token() {
    let source = "const first = 1;\n// ctc: mode=forbid\n";
    let diagnostics = parse_directives(source, Path::new("rule.ts.ctmpl")).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC1005");
}

#[test]
fn permits_directive_after_byte_order_mark() {
    let source = "\u{feff}// ctc: mode=forbid\n{{ Value }}";
    let directives = parse_directives(source, Path::new("rule.ts.ctmpl")).unwrap();
    assert_eq!(directives.mode, MatchMode::Forbid);
}

#[test]
fn parses_every_mode_directive() {
    let source = "// ctc: mode=every\nclass {{ Name }} {}";
    let directives = parse_directives(source, Path::new("rule.ts.ctmpl")).unwrap();
    assert_eq!(directives.mode, MatchMode::Every);
}

#[test]
fn invalid_filter_uses_filter_diagnostic_code() {
    let source = "{{ Value | prefix(1) }}";
    let ranges = std::iter::once(0..source.len()).collect::<Vec<_>>();
    let diagnostics =
        parse_placeholders(source, Path::new("rule.ts.ctmpl"), &ranges, &TestAdapter).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC2008");
}

#[test]
fn unknown_optional_keyword_is_invalid() {
    let source = "{{? keyword:unknown }}";
    let ranges = std::iter::once(0..source.len()).collect::<Vec<_>>();
    let diagnostics =
        parse_placeholders(source, Path::new("rule.ts.ctmpl"), &ranges, &TestAdapter).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC2001");
}

fn parse_one(source: &str) -> Result<Vec<Placeholder>, Vec<Diagnostic>> {
    let ranges = std::iter::once(0..source.len()).collect::<Vec<_>>();
    parse_placeholders(source, Path::new("rule.ts.ctmpl"), &ranges, &TestAdapter)
}

#[test]
fn file_name_placeholder_is_a_plain_capture() {
    let placeholders =
        parse_one("{{ Name | removePrefix(\"I\") | fileName(\"PascalCase\") }}").unwrap();
    let placeholder = &placeholders[0];
    assert_eq!(placeholder.file_name_case(), Some(NameCase::Pascal));
    assert!(!placeholder.is_derived());
}

#[test]
fn regex_placeholders_are_plain_captures() {
    for source in [
        "{{ Name | matches(\"^I[A-Z]\") }}",
        "{{ Name | notMatches(\"Impl$\") }}",
        "{{ Name | removePrefix(\"I\") | matches(\"^[A-Z]\") }}",
        "{{ Name | matches(\"^I\") | notMatches(\"Impl$\") | fileName(\"PascalCase\") }}",
    ] {
        let placeholders = parse_one(source).unwrap();
        assert!(!placeholders[0].is_derived(), "{source}");
        assert!(placeholders[0].filters.iter().any(Filter::is_name_filter));
    }
}

#[test]
fn rejects_invalid_regex_filters() {
    for source in [
        "{{ Name | matches(\"(\") }}",
        "{{ Name | matches() }}",
        "{{ Name | matches(\"a\", \"b\") }}",
        "{{ Name | notMatches(1) }}",
        "{{ Name | matches(\"^I\") | prefix(\"x\") }}",
        "{{ Name | kind(\"ImportDeclaration\") | matches(\"^I\") }}",
        "{{* Names | matches(\"^I\") }}",
    ] {
        let diagnostics = parse_one(source).unwrap_err();
        assert_eq!(diagnostics[0].code, "CTC2008", "{source}");
    }
}

#[test]
fn regex_patterns_compare_by_text() {
    let first = RegexPattern::new("^a").unwrap();
    assert_eq!(first, RegexPattern::new("^a").unwrap());
    assert_ne!(first, RegexPattern::new("^b").unwrap());
    assert!(first.is_match("abc"));
    assert!(!first.is_match("bac"));
}

#[test]
fn rejects_invalid_file_name_filters() {
    for source in [
        "{{ Name | fileName(\"kebab-case\") }}",
        "{{ Name | fileName() }}",
        "{{ Name | fileName(\"PascalCase\") | prefix(\"I\") }}",
        "{{ Name | fileName(\"PascalCase\") | fileName(\"asIs\") }}",
        "{{ Name | kind(\"ImportDeclaration\") | fileName(\"PascalCase\") }}",
        "{{* Names | fileName(\"PascalCase\") }}",
    ] {
        let diagnostics = parse_one(source).unwrap_err();
        assert_eq!(diagnostics[0].code, "CTC2008", "{source}");
    }
}
