use super::*;

#[test]
fn rule_message_is_a_separate_json_field() {
    let diagnostic = Diagnostic::new(
        "CTC3006",
        DiagnosticCategory::ForbiddenStructure,
        "The source contains a forbidden structure.",
        1,
    )
    .with_rule("no-try")
    .with_rule_message("Return a Result value instead of using try.");
    let value = serde_json::to_value(&diagnostic).unwrap();
    assert_eq!(
        value["message"],
        "The source contains a forbidden structure."
    );
    assert_eq!(
        value["ruleMessage"],
        "Return a Result value instead of using try."
    );
}

#[test]
fn only_source_mismatches_from_rules_are_rule_failures() {
    let mismatch = Diagnostic::new(
        "CTC3006",
        DiagnosticCategory::ForbiddenStructure,
        "The source contains a forbidden structure.",
        1,
    );
    assert!(!mismatch.clone().is_rule_failure());
    assert!(mismatch.with_rule("no-try").is_rule_failure());
    let semantic = Diagnostic::new(
        "CTC4102",
        DiagnosticCategory::ForbiddenThrow,
        "A throw statement is forbidden by this exception policy.",
        1,
    )
    .with_rule("no-exceptions");
    assert!(semantic.is_rule_failure());
    let template = Diagnostic::new(
        "CTC2006",
        DiagnosticCategory::PlaceholderCategoryMismatch,
        "Capture `Name` does not match the required placeholder category.",
        1,
    )
    .with_rule("no-try");
    assert!(!template.is_rule_failure());
    let internal = Diagnostic::new(
        "CTC9001",
        DiagnosticCategory::InternalToolError,
        "Internal error.",
        2,
    )
    .with_rule("no-try");
    assert!(!internal.is_rule_failure());
}

#[test]
fn line_index_gives_the_same_positions_as_from_offsets() {
    for source in [
        "",
        "one",
        "one\ntwo\n",
        "a\r\nb\r\n\r\nc",
        "héllo wörld\nsecond ü line\n\n日本語\nend",
        "\n\n\n",
    ] {
        let path = Path::new("dir\\file.ts");
        let index = LineIndex::new(path, source);
        for offset in (0..=source.len() + 2)
            .filter(|offset| *offset > source.len() || source.is_char_boundary(*offset))
        {
            for end in [offset, source.len()] {
                assert_eq!(
                    index.range(offset, end),
                    TextRange::from_offsets(path, source, offset, end),
                    "{source:?} at {offset}"
                );
            }
        }
    }
}

#[test]
fn rule_message_is_omitted_when_absent() {
    let diagnostic = Diagnostic::new(
        "CTC3006",
        DiagnosticCategory::ForbiddenStructure,
        "The source contains a forbidden structure.",
        1,
    );
    let value = serde_json::to_value(&diagnostic).unwrap();
    assert!(value.get("ruleMessage").is_none());
}
