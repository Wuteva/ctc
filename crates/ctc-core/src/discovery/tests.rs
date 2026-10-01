use std::{fs, sync::Arc};

use tempfile::TempDir;

use super::*;
use crate::language::LanguageAdapter;

struct TestLanguage;

impl LanguageAdapter for TestLanguage {
    fn id(&self) -> &'static str {
        "test"
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension().is_some_and(|extension| extension == "ts")
    }

    fn supports_selector_suffix(&self, suffix: &str) -> bool {
        suffix.ends_with(".ts")
    }
}

fn registry() -> LanguageRegistry {
    let mut registry = LanguageRegistry::new();
    registry.register(Arc::new(TestLanguage));
    registry
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn fixture(config: &str) -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    write(directory.path(), ".ctc.json", config);
    write(directory.path(), ".ctmpl/rule.ts.ctmpl", "export {};");
    directory
}

#[test]
fn fixed_directory_is_the_path_before_the_first_wildcard() {
    for (pattern, expected) in [
        ("src/domain/**/*.ts", "src/domain"),
        ("**/*.ts", ""),
        ("*.ts", ""),
        ("src/index.ts", "src"),
        ("index.ts", ""),
        ("./src/{a,b}/*.ts", "src"),
        ("src/a/b?/c.ts", "src/a"),
    ] {
        assert_eq!(fixed_directory(pattern), expected, "{pattern}");
    }
}

#[test]
fn inside_directory_matches_whole_path_segments() {
    assert!(is_inside_directory("src/a.ts", "src"));
    assert!(is_inside_directory("src/a/b.ts", "src/a"));
    assert!(is_inside_directory("anything.ts", ""));
    assert!(!is_inside_directory("src2/a.ts", "src"));
    assert!(!is_inside_directory("src", "src"));
}

#[test]
fn applies_include_and_exclude_globs() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "rules": [{
"id": "example",
"template": ".ctmpl/rule.ts.ctmpl",
"include": ["src/**/*.ts"],
"exclude": ["src/index.ts"]
  }]
}"#,
    );
    write(directory.path(), "src/a.ts", "export {};");
    write(directory.path(), "src/nested/b.ts", "export {};");
    write(directory.path(), "src/index.ts", "export {};");

    let applications = discover(directory.path(), None, &[], &[], &registry()).unwrap();
    let paths = applications
        .iter()
        .map(|application| application.source_display_path.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [PathBuf::from("src/a.ts"), PathBuf::from("src/nested/b.ts")]
    );
}

#[test]
fn all_matching_rules_apply_to_one_file() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "rules": [
{
  "id": "first",
  "template": ".ctmpl/rule.ts.ctmpl",
  "include": ["src/**/*.ts"]
},
{
  "id": "second",
  "template": ".ctmpl/rule.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "contains",
  "scope": "descendants"
}
  ]
}"#,
    );
    write(directory.path(), "src/a.ts", "export {};");
    let applications = discover(directory.path(), None, &[], &[], &registry()).unwrap();
    assert_eq!(applications.len(), 2);
    assert_eq!(applications[0].id, "first");
    assert_eq!(applications[1].id, "second");
    assert_eq!(applications[0].scope, SearchScope::TopLevel);
    assert_eq!(applications[1].scope, SearchScope::Descendants);
}

#[test]
fn optional_config_path_overrides_default() {
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path(),
        "strict.json",
        r#"{
  "schemaVersion": 1,
  "rules": [{
"id": "example",
"template": ".ctmpl/rule.ts.ctmpl",
"include": ["src/**/*.ts"]
  }]
}"#,
    );
    write(directory.path(), ".ctmpl/rule.ts.ctmpl", "export {};");
    write(directory.path(), "src/a.ts", "export {};");
    let applications = discover(
        directory.path(),
        Some(Path::new("strict.json")),
        &[],
        &[],
        &registry(),
    )
    .unwrap();
    assert_eq!(applications.len(), 1);
}

#[test]
fn duplicate_rule_ids_are_invalid() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "rules": [
{
  "id": "example",
  "template": ".ctmpl/rule.ts.ctmpl",
  "include": ["src/**/*.ts"]
},
{
  "id": "example",
  "template": ".ctmpl/rule.ts.ctmpl",
  "include": ["src/**/*.ts"]
}
  ]
}"#,
    );
    let diagnostics = discover(directory.path(), None, &[], &[], &registry()).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC1010");
}

#[test]
fn stale_include_is_an_error() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "rules": [{
"id": "example",
"template": ".ctmpl/rule.ts.ctmpl",
"include": ["missing/**/*.ts"]
  }]
}"#,
    );
    let diagnostics = discover(directory.path(), None, &[], &[], &registry()).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC1003");
}

#[test]
fn missing_configuration_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let diagnostics = discover(directory.path(), None, &[], &[], &registry()).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC1010");
}

#[test]
fn exact_mode_rejects_non_top_level_scope() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "rules": [{
"id": "example",
"template": ".ctmpl/rule.ts.ctmpl",
"include": ["src/**/*.ts"],
"mode": "exact",
"scope": "descendants"
  }]
}"#,
    );
    write(directory.path(), "src/a.ts", "export {};");
    let diagnostics = discover(directory.path(), None, &[], &[], &registry()).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC1010");
}

#[test]
fn rule_message_is_passed_to_each_application() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "rules": [
{
  "id": "with-message",
  "template": ".ctmpl/rule.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "message": "Use the shared file header."
},
{
  "id": "without-message",
  "template": ".ctmpl/rule.ts.ctmpl",
  "include": ["src/**/*.ts"]
}
  ]
}"#,
    );
    write(directory.path(), "src/a.ts", "export {};");
    let applications = discover(directory.path(), None, &[], &[], &registry()).unwrap();
    assert_eq!(applications.len(), 2);
    assert_eq!(
        applications[0].message.as_deref(),
        Some("Use the shared file header.")
    );
    assert_eq!(applications[1].message, None);
}

#[test]
fn semantic_rule_message_is_passed_to_each_application() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "semanticRules": [{
"kind": "exceptionPolicy",
"id": "no-exceptions",
"include": ["src/**/*.ts"],
"forbidThrow": true,
"message": "Return a Result value instead."
  }]
}"#,
    );
    write(directory.path(), "src/a.ts", "export {};");
    let mut languages = LanguageRegistry::new();
    languages.register(Arc::new(TypeScriptStub));
    let result = discover_all(directory.path(), None, &[], &[], &languages).unwrap();
    assert_eq!(result.semantic_rules.len(), 1);
    assert_eq!(
        result.semantic_rules[0].message.as_deref(),
        Some("Return a Result value instead.")
    );
}

#[test]
fn empty_rule_message_is_invalid() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "rules": [{
"id": "example",
"template": ".ctmpl/rule.ts.ctmpl",
"include": ["src/**/*.ts"],
"message": "  "
  }]
}"#,
    );
    write(directory.path(), "src/a.ts", "export {};");
    let diagnostics = discover(directory.path(), None, &[], &[], &registry()).unwrap_err();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "CTC1010");
    assert_eq!(
        diagnostics[0].message,
        "Rule `example` has an empty message."
    );
}

#[test]
fn multi_line_rule_message_is_invalid() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "rules": [{
"id": "example",
"template": ".ctmpl/rule.ts.ctmpl",
"include": ["src/**/*.ts"],
"message": "First line.\nSecond line."
  }]
}"#,
    );
    write(directory.path(), "src/a.ts", "export {};");
    let diagnostics = discover(directory.path(), None, &[], &[], &registry()).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC1010");
    assert_eq!(
        diagnostics[0].message,
        "Rule `example` has a message with a line break. Use one line."
    );
}

#[test]
fn empty_semantic_rule_message_is_invalid() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "semanticRules": [{
"kind": "returnPaths",
"id": "return-paths",
"typeName": "Result",
"include": ["src/**/*.ts"],
"message": ""
  }]
}"#,
    );
    write(directory.path(), "src/a.ts", "export {};");
    let mut languages = LanguageRegistry::new();
    languages.register(Arc::new(TypeScriptStub));
    let diagnostics = discover_all(directory.path(), None, &[], &[], &languages)
        .err()
        .unwrap();
    assert_eq!(diagnostics[0].code, "CTC1010");
    assert_eq!(
        diagnostics[0].message,
        "Semantic rule `return-paths` has an empty message."
    );
}

#[test]
fn non_string_rule_message_is_invalid() {
    let directory = fixture(
        r#"{
  "schemaVersion": 1,
  "rules": [{
"id": "example",
"template": ".ctmpl/rule.ts.ctmpl",
"include": ["src/**/*.ts"],
"message": 42
  }]
}"#,
    );
    write(directory.path(), "src/a.ts", "export {};");
    let diagnostics = discover(directory.path(), None, &[], &[], &registry()).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC1010");
}

struct TypeScriptStub;

impl LanguageAdapter for TypeScriptStub {
    fn id(&self) -> &'static str {
        "typescript"
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension().is_some_and(|extension| extension == "ts")
    }

    fn supports_selector_suffix(&self, suffix: &str) -> bool {
        suffix.ends_with(".ts")
    }
}

struct KindLanguage;

impl LanguageAdapter for KindLanguage {
    fn id(&self) -> &'static str {
        "test"
    }

    fn supports_path(&self, path: &Path) -> bool {
        path.extension().is_some_and(|extension| extension == "ts")
    }

    fn supports_selector_suffix(&self, suffix: &str) -> bool {
        suffix.ends_with(".ts")
    }

    fn known_kind(&self, value: &str) -> bool {
        matches!(value, "ClassDeclaration" | "AbstractClassDeclaration")
    }
}

fn discover_kinds(mode: &str, kinds: &str) -> Result<Vec<RuleApplication>, Vec<Diagnostic>> {
    let directory = fixture(&format!(
        r#"{{
  "schemaVersion": 1,
  "rules": [{{
"id": "example",
"template": ".ctmpl/rule.ts.ctmpl",
"include": ["src/**/*.ts"],
"mode": "{mode}",
"scope": "descendants",
"kinds": {kinds}
  }}]
}}"#
    ));
    write(directory.path(), "src/a.ts", "export {};");
    let mut registry = LanguageRegistry::new();
    registry.register(Arc::new(KindLanguage));
    discover(directory.path(), None, &[], &[], &registry)
}

#[test]
fn every_rule_passes_listed_kinds_to_applications() {
    let applications = discover_kinds(
        "every",
        r#"["ClassDeclaration", "AbstractClassDeclaration"]"#,
    )
    .unwrap();
    assert_eq!(
        applications[0].kinds,
        ["ClassDeclaration", "AbstractClassDeclaration"]
    );
}

#[test]
fn kinds_require_every_mode_known_kinds_and_at_least_one_entry() {
    for (mode, kinds, message) in [
        (
            "contains",
            r#"["ClassDeclaration"]"#,
            "Rule `example` uses `kinds` without every mode.",
        ),
        ("every", "[]", "Rule `example` must list at least one kind."),
        (
            "every",
            r#"["ClassDeclaration", "Klass"]"#,
            "Rule `example` lists unknown kind `Klass`.",
        ),
    ] {
        let diagnostics = discover_kinds(mode, kinds).unwrap_err();
        assert_eq!(diagnostics[0].code, "CTC1010");
        assert_eq!(diagnostics[0].message, message);
    }
}

#[test]
fn partner_patterns_expand_path_tokens() {
    let expand = |pattern: &str, anchor: &str| expand_partner_pattern(pattern, anchor);
    assert_eq!(
        expand("{dir}/{stem}.cpp", "include/net/widget.hpp").as_deref(),
        Some("include/net/widget.cpp")
    );
    assert_eq!(
        expand("src/{subdir}/{stem}.cpp", "include/net/widget.hpp").as_deref(),
        Some("src/net/widget.cpp")
    );
    assert_eq!(
        expand("src/{subdir}/{stem}.cpp", "include/widget.hpp").as_deref(),
        Some("src/widget.cpp")
    );
    assert_eq!(
        expand("{dir}/{stem}.cpp", "widget.hpp").as_deref(),
        Some("widget.cpp")
    );
    assert_eq!(
        expand("{dir}/{stem}.test.{ext}", "src/a.b.ts").as_deref(),
        Some("src/a.b.test.ts")
    );
    assert_eq!(expand("{dir}", "widget.hpp"), None);
}

#[test]
fn partner_pattern_validation_rejects_bad_patterns() {
    assert!(check_partner_pattern("{dir}/{stem}.test.ts").is_ok());
    for pattern in [
        "",
        "/abs/{stem}.cpp",
        "src\\{stem}.cpp",
        "../{stem}.cpp",
        "{name}.cpp",
        "{stem.cpp",
        "stem}.cpp",
    ] {
        assert!(check_partner_pattern(pattern).is_err(), "{pattern}");
    }
}

fn companion_fixture(companions: &str) -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path(),
        ".ctc.json",
        &format!(
            r#"{{
  "schemaVersion": 1,
  "semanticRules": [{{
"kind": "companionFile",
"id": "companion",
"include": ["src/**/*.ts"],
"exclude": ["src/**/*.test.ts"],
"companions": {companions}
  }}]
}}"#
        ),
    );
    directory
}

#[test]
fn companion_rule_uses_the_first_existing_candidate() {
    let directory =
        companion_fixture(r#"["{dir}/{stem}.test.ts", "test/{subdir}/{stem}.test.ts"]"#);
    write(directory.path(), "src/lib/a.ts", "");
    write(directory.path(), "test/lib/a.test.ts", "");
    write(directory.path(), "src/lib/b.ts", "");
    write(directory.path(), "src/lib/b.test.ts", "");
    write(directory.path(), "test/lib/b.test.ts", "");
    write(directory.path(), "src/lib/c.ts", "");
    let result = discover_all(directory.path(), None, &[], &[], &registry()).unwrap();
    let found = result
        .semantic_rules
        .iter()
        .map(|application| {
            (
                application.source_display_path.clone(),
                application
                    .partner
                    .as_ref()
                    .map(|partner| partner.display_path.clone()),
                application.partner_candidates.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        found,
        [
            (
                PathBuf::from("src/lib/a.ts"),
                Some(PathBuf::from("test/lib/a.test.ts")),
                vec![
                    "src/lib/a.test.ts".to_string(),
                    "test/lib/a.test.ts".to_string()
                ]
            ),
            (
                PathBuf::from("src/lib/b.ts"),
                Some(PathBuf::from("src/lib/b.test.ts")),
                vec![
                    "src/lib/b.test.ts".to_string(),
                    "test/lib/b.test.ts".to_string()
                ]
            ),
            (
                PathBuf::from("src/lib/c.ts"),
                None,
                vec![
                    "src/lib/c.test.ts".to_string(),
                    "test/lib/c.test.ts".to_string()
                ]
            ),
        ]
    );
}

#[test]
fn companion_directory_is_not_a_partner_file() {
    let directory = companion_fixture(r#"["{dir}/{stem}.test.ts"]"#);
    write(directory.path(), "src/a.ts", "");
    fs::create_dir_all(directory.path().join("src/a.test.ts")).unwrap();
    let result = discover_all(directory.path(), None, &[], &[], &registry()).unwrap();
    assert_eq!(result.semantic_rules[0].partner, None);
}

#[test]
fn explicit_path_selects_the_anchor_and_still_resolves_the_partner() {
    let directory = companion_fixture(r#"["test/{stem}.test.ts"]"#);
    write(directory.path(), "src/a.ts", "");
    write(directory.path(), "src/b.ts", "");
    write(directory.path(), "test/a.test.ts", "");
    let result = discover_all(
        directory.path(),
        None,
        &[PathBuf::from("src/a.ts")],
        &[],
        &registry(),
    )
    .unwrap();
    assert_eq!(result.semantic_rules.len(), 1);
    assert_eq!(
        result.semantic_rules[0]
            .partner
            .as_ref()
            .map(|partner| partner.display_path.clone()),
        Some(PathBuf::from("test/a.test.ts"))
    );
}

#[test]
fn invalid_companion_patterns_are_configuration_errors() {
    let directory = companion_fixture(r#"["../{stem}.test.ts", "{file}.ts"]"#);
    write(directory.path(), "src/a.ts", "");
    let diagnostics = discover_all(directory.path(), None, &[], &[], &registry())
        .err()
        .unwrap();
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "CTC1010")
    );

    let directory = companion_fixture("[]");
    write(directory.path(), "src/a.ts", "");
    let diagnostics = discover_all(directory.path(), None, &[], &[], &registry())
        .err()
        .unwrap();
    assert_eq!(
        diagnostics[0].message,
        "Semantic rule `companion` must contain at least one companions pattern."
    );
}

#[test]
fn pairing_rule_needs_the_cpp_adapter() {
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path(),
        ".ctc.json",
        r#"{
  "schemaVersion": 1,
  "semanticRules": [{
"kind": "headerSourcePairing",
"id": "pairing",
"include": ["src/**/*.hpp"],
"sources": ["{dir}/{stem}.cpp"]
  }]
}"#,
    );
    let diagnostics = discover_all(directory.path(), None, &[], &[], &registry())
        .err()
        .unwrap();
    assert_eq!(diagnostics[0].code, "CTC1010");
    assert!(diagnostics[0].message.contains("`cpp` language adapter"));
}

#[cfg(unix)]
#[test]
fn symbolic_link_partner_is_ignored() {
    let directory = companion_fixture(r#"["linked/{stem}.test.ts"]"#);
    write(directory.path(), "src/a.ts", "");
    write(directory.path(), "real/a.test.ts", "");
    std::os::unix::fs::symlink(
        directory.path().join("real"),
        directory.path().join("linked"),
    )
    .unwrap();
    let result = discover_all(directory.path(), None, &[], &[], &registry()).unwrap();
    assert_eq!(result.semantic_rules[0].partner, None);
}
