use std::{collections::BTreeSet, ops::Range, path::Path};

use super::{Suppressions, collect_suppressions};
use crate::diagnostic::{Diagnostic, DiagnosticCategory, TextRange};

fn comment_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < source.len() {
        let rest = &source[index..];
        if rest.starts_with("//") {
            let end = rest.find('\n').map_or(source.len(), |end| index + end);
            ranges.push(index..end);
            index = end;
        } else if rest.starts_with("/*") {
            let end = rest.find("*/").map_or(source.len(), |end| index + end + 2);
            ranges.push(index..end);
            index = end;
        } else {
            index += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    ranges
}

fn collect(source: &str) -> Suppressions {
    collect_with_ignorable(source, &["first", "second", "third"])
}

fn collect_with_ignorable(source: &str, ignorable: &[&str]) -> Suppressions {
    let known = ["first", "second", "third"]
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let ignorable = ignorable
        .iter()
        .map(|rule| rule.to_string())
        .collect::<BTreeSet<_>>();
    collect_suppressions(
        Path::new("source.ts"),
        source,
        &comment_ranges(source),
        &known,
        &ignorable,
    )
}

fn diagnostic_at(source: &str, rule_id: &str, line: u32) -> Diagnostic {
    let offset = source
        .split_inclusive('\n')
        .take(line as usize - 1)
        .map(str::len)
        .sum();
    Diagnostic::new("CTC3006", DiagnosticCategory::ForbiddenStructure, "x", 1)
        .with_rule(rule_id)
        .with_source(TextRange::from_offsets(
            Path::new("source.ts"),
            source,
            offset,
            offset,
        ))
}

#[test]
fn next_line_comment_suppresses_only_the_next_code_line() {
    let source = "// ctc-ignore-next-line first\nconst a = 1;\nconst b = 2;\n";
    let suppressions = collect(source);
    assert!(suppressions.diagnostics.is_empty());
    assert!(suppressions.suppresses(&diagnostic_at(source, "first", 2)));
    assert!(!suppressions.suppresses(&diagnostic_at(source, "first", 3)));
    assert!(!suppressions.suppresses(&diagnostic_at(source, "second", 2)));
}

#[test]
fn next_line_comment_skips_blank_lines_and_other_comments() {
    let source = "/* ctc-ignore-next-line first, second -- reason */\n\n// note\nconst a = 1;\n";
    let suppressions = collect(source);
    assert!(suppressions.diagnostics.is_empty());
    assert!(suppressions.suppresses(&diagnostic_at(source, "first", 4)));
    assert!(suppressions.suppresses(&diagnostic_at(source, "second", 4)));
    assert!(!suppressions.suppresses(&diagnostic_at(source, "third", 4)));
}

#[test]
fn file_comment_suppresses_the_rule_on_every_line() {
    let source = "const a = 1;\n/** ctc-ignore-file third */\nconst b = 2;\n";
    let suppressions = collect(source);
    assert!(suppressions.diagnostics.is_empty());
    assert!(suppressions.suppresses(&diagnostic_at(source, "third", 1)));
    assert!(suppressions.suppresses(&diagnostic_at(source, "third", 3)));
    assert!(!suppressions.suppresses(&diagnostic_at(source, "first", 3)));
}

#[test]
fn comments_naming_protected_rules_do_not_suppress_and_report_ctc5002() {
    let source = "// ctc-ignore-next-line first, second\nconst a = 1;\n";
    let suppressions = collect_with_ignorable(source, &["second"]);
    assert!(!suppressions.suppresses(&diagnostic_at(source, "first", 2)));
    assert!(suppressions.suppresses(&diagnostic_at(source, "second", 2)));
    assert_eq!(suppressions.diagnostics.len(), 1);
    let diagnostic = &suppressions.diagnostics[0];
    assert_eq!(diagnostic.code, "CTC5002");
    assert_eq!(
        diagnostic.category,
        DiagnosticCategory::SuppressionNotAllowed
    );
    assert_eq!(diagnostic.rule_id.as_deref(), Some("first"));
    assert_eq!(diagnostic.exit_class, 1);
    assert_eq!(diagnostic.source.as_ref().unwrap().start.line, 1);
}

#[test]
fn file_comments_naming_protected_rules_do_not_suppress() {
    let source = "// ctc-ignore-file first\nconst a = 1;\n";
    let suppressions = collect_with_ignorable(source, &[]);
    assert!(!suppressions.suppresses(&diagnostic_at(source, "first", 2)));
    assert_eq!(suppressions.diagnostics.len(), 1);
    assert_eq!(suppressions.directives.len(), 1);
    assert_eq!(suppressions.directives[0].rule_id, "first");
}

#[test]
fn diagnostics_without_a_rule_are_never_suppressed() {
    let source = "// ctc-ignore-file first\nconst a = 1;\n";
    let suppressions = collect(source);
    let diagnostic =
        Diagnostic::new("CTC3001", DiagnosticCategory::SourceParseError, "x", 1).with_source(
            TextRange::from_offsets(Path::new("source.ts"), source, 0, 0),
        );
    assert!(!suppressions.suppresses(&diagnostic));
}

#[test]
fn reason_separator_needs_surrounding_white_space() {
    let source = "// ctc-ignore-next-line first -- see first--second\nconst a = 1;\n";
    let suppressions = collect(source);
    assert!(suppressions.diagnostics.is_empty());
    assert!(suppressions.suppresses(&diagnostic_at(source, "first", 2)));
}

#[test]
fn invalid_comments_produce_diagnostics() {
    let source = concat!(
        "// ctc-ignore-next-line\n",
        "// ctc-ignore-file -- only a reason\n",
        "// ctc-ignore-next-line first, missing\n",
        "// ctc-ignore-next-line first,\n",
        "// ctc-ignore-lines first\n",
        "const a = 1;\n",
    );
    let suppressions = collect(source);
    let messages = suppressions
        .diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic.code, "CTC5001");
            assert_eq!(diagnostic.category, DiagnosticCategory::InvalidSuppression);
            assert_eq!(diagnostic.rule_id, None);
            (
                diagnostic.source.as_ref().unwrap().start.line,
                diagnostic.message.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        vec![
            (
                1,
                "A `ctc-ignore-next-line` comment must name at least one rule identifier."
            ),
            (
                2,
                "A `ctc-ignore-file` comment must name at least one rule identifier."
            ),
            (
                3,
                "The suppression comment names unknown rule identifier `missing`."
            ),
            (
                4,
                "The suppression comment contains an empty rule identifier."
            ),
            (
                5,
                "Unknown suppression directive `ctc-ignore-lines`. Use `ctc-ignore-next-line` or `ctc-ignore-file`."
            ),
        ]
    );
    assert!(suppressions.suppresses(&diagnostic_at(source, "first", 6)));
}

#[test]
fn ordinary_comments_are_ignored() {
    let source = "// see ctc-ignore-file first\n// ctc: ignore\nconst a = 1;\n";
    let suppressions = collect(source);
    assert!(suppressions.diagnostics.is_empty());
    assert!(!suppressions.suppresses(&diagnostic_at(source, "first", 3)));
}
