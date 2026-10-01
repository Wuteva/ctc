use super::*;

fn fact(
    role: MemberFunctionRole,
    owner: &str,
    name: &str,
    signature: &str,
    line: u32,
) -> MemberFunctionFact {
    let path = match role {
        MemberFunctionRole::Declaration => "widget.hpp",
        MemberFunctionRole::Definition => "widget.cpp",
    };
    let mut range = TextRange::from_offsets(Path::new(path), "", 0, 0);
    range.start.line = line;
    range.end.line = line;
    MemberFunctionFact {
        role,
        owner: owner
            .split("::")
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect(),
        name: name.to_string(),
        signature: signature.to_string(),
        range,
    }
}

fn declarations(items: &[(&str, &str, &str)]) -> SemanticFacts {
    SemanticFacts {
        member_functions: items
            .iter()
            .enumerate()
            .map(|(index, (owner, name, signature))| {
                fact(
                    MemberFunctionRole::Declaration,
                    owner,
                    name,
                    signature,
                    index as u32 + 1,
                )
            })
            .collect(),
        ..SemanticFacts::default()
    }
}

fn definitions(items: &[(&str, &str, &str)]) -> SemanticFacts {
    SemanticFacts {
        member_functions: items
            .iter()
            .enumerate()
            .map(|(index, (owner, name, signature))| {
                fact(
                    MemberFunctionRole::Definition,
                    owner,
                    name,
                    signature,
                    index as u32 + 1,
                )
            })
            .collect(),
        ..SemanticFacts::default()
    }
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<(&str, u32)> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic.source.as_ref().unwrap().start.line,
            )
        })
        .collect()
}

fn check(header: &SemanticFacts, source: &SemanticFacts) -> Vec<Diagnostic> {
    evaluate_header_source_pairing("pairing", header, Path::new("widget.cpp"), source, true)
}

#[test]
fn matching_definitions_in_order_pass() {
    let header = declarations(&[
        ("W", "a", "()"),
        ("W", "b", "(int)"),
        ("W", "c", "() const"),
    ]);
    let source = definitions(&[
        ("W", "a", "()"),
        ("W", "b", "(int)"),
        ("W", "c", "() const"),
    ]);
    assert!(check(&header, &source).is_empty());
}

#[test]
fn reports_missing_definition_with_other_overload_hint() {
    let header = declarations(&[("W", "a", "()"), ("W", "b", "(int)")]);
    let source = definitions(&[("W", "a", "()"), ("W", "b", "(double)")]);
    let diagnostics = check(&header, &source);
    assert_eq!(codes(&diagnostics), [("CTC4202", 2)]);
    assert_eq!(
        diagnostics[0].message,
        "`W::b(int)` is declared here but not defined in `widget.cpp`. A definition with other parameters exists at `widget.cpp:2`: `W::b(double)`."
    );
}

#[test]
fn overloads_and_const_overloads_are_separate() {
    let header = declarations(&[("W", "get", "()"), ("W", "get", "() const")]);
    let source = definitions(&[("W", "get", "() const")]);
    assert_eq!(codes(&check(&header, &source)), [("CTC4202", 1)]);
}

#[test]
fn one_moved_definition_gives_one_order_diagnostic() {
    let header = declarations(&[
        ("W", "a", "()"),
        ("W", "b", "()"),
        ("W", "c", "()"),
        ("W", "d", "()"),
    ]);
    let source = definitions(&[
        ("W", "d", "()"),
        ("W", "a", "()"),
        ("W", "b", "()"),
        ("W", "c", "()"),
    ]);
    let diagnostics = check(&header, &source);
    assert_eq!(codes(&diagnostics), [("CTC4203", 1)]);
    assert_eq!(
        diagnostics[0].message,
        "Definition of `W::d()` is out of order. `widget.hpp:4` declares it after `W::c()`, so define it after that function."
    );
    assert_eq!(
        diagnostics[0].expected.as_deref(),
        Some("position 4 of 4 (header order)")
    );
    assert_eq!(diagnostics[0].actual.as_deref(), Some("position 1 of 4"));
}

#[test]
fn first_declaration_defined_late_names_the_next_function() {
    let header = declarations(&[("W", "a", "()"), ("W", "b", "()"), ("W", "c", "()")]);
    let source = definitions(&[("W", "b", "()"), ("W", "c", "()"), ("W", "a", "()")]);
    let diagnostics = check(&header, &source);
    assert_eq!(codes(&diagnostics), [("CTC4203", 3)]);
    assert!(
        diagnostics[0]
            .message
            .contains("declares it before `W::b()`")
    );
}

#[test]
fn check_order_false_reports_only_missing_definitions() {
    let header = declarations(&[("W", "a", "()"), ("W", "b", "()")]);
    let source = definitions(&[("W", "b", "()"), ("W", "a", "()")]);
    let diagnostics =
        evaluate_header_source_pairing("pairing", &header, Path::new("widget.cpp"), &source, false);
    assert!(diagnostics.is_empty());
}

#[test]
fn definition_owner_can_omit_leading_namespaces() {
    let header = declarations(&[("ui::W", "a", "()"), ("ui::W", "b", "()")]);
    let source = definitions(&[("W", "a", "()"), ("ui::W", "b", "()")]);
    assert!(check(&header, &source).is_empty());
}

#[test]
fn definition_in_header_satisfies_the_declaration() {
    let mut header = declarations(&[("W", "a", "()"), ("W", "b", "()")]);
    header
        .member_functions
        .push(fact(MemberFunctionRole::Definition, "W", "b", "()", 9));
    assert!(header_needs_definitions(&header));
    let source = definitions(&[("W", "a", "()")]);
    assert!(check(&header, &source).is_empty());

    let mut inline_only = declarations(&[("W", "a", "()")]);
    inline_only
        .member_functions
        .push(fact(MemberFunctionRole::Definition, "W", "a", "()", 9));
    assert!(!header_needs_definitions(&inline_only));
}

#[test]
fn longest_increasing_marks_the_kept_positions() {
    assert_eq!(longest_increasing(&[3, 0, 1, 2]), [false, true, true, true]);
    assert_eq!(longest_increasing(&[0, 1, 2]), [true, true, true]);
    assert!(longest_increasing(&[]).is_empty());
}

#[test]
fn companion_message_lists_candidates() {
    let diagnostic = missing_companion_file(
        "companion",
        Path::new("src/a.ts"),
        &["src/a.test.ts".to_string(), "test/a.test.ts".to_string()],
    );
    assert_eq!(diagnostic.code, "CTC4201");
    assert_eq!(
        diagnostic.message,
        "No companion file found for `src/a.ts`. Expected one of: `src/a.test.ts`, `test/a.test.ts`."
    );
}
