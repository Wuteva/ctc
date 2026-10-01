use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::{Value, json};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/service-adapter")
}

fn cpp_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cpp")
}

#[test]
fn reports_value_import_outside_composition_root() {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            fixture().to_str().unwrap(),
            "--rule",
            "no-value-imports",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report,
        json!({
            "schemaVersion": 1,
            "matches": false,
            "diagnostics": [{
                "code": "CTC3006",
                "category": "forbidden-structure",
                "message": "The source contains a forbidden structure.",
                "ruleId": "no-value-imports",
                "source": {
                    "path": "src/adapter.ts",
                    "start": { "offset": 0, "line": 1, "column": 1 },
                    "end": { "offset": 39, "line": 1, "column": 40 }
                },
                "template": {
                    "path": ".ctmpl/no-value-imports.ts.ctmpl",
                    "start": { "offset": 0, "line": 1, "column": 1 },
                    "end": { "offset": 105, "line": 1, "column": 106 }
                }
            }]
        })
    );
}

#[test]
fn all_rules_keep_the_same_single_violation() {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(["--root", fixture().to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics.len(),
        1,
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(diagnostics[0]["ruleId"], "no-value-imports");
}

#[test]
fn validates_discovered_template() {
    let template = fixture().join(".ctmpl/class-factory.ts.ctmpl");
    Command::cargo_bin("ctc")
        .unwrap()
        .args(["validate-template", template.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn version_uses_binary_name() {
    Command::cargo_bin("ctc")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout("ctc 0.1.0\n");
}

#[test]
fn human_diagnostic_format_is_stable() {
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            fixture().to_str().unwrap(),
            "--rule",
            "no-value-imports",
            "--format",
            "human",
        ])
        .assert()
        .code(1)
        .stdout(
            "src/adapter.ts:1:1 [no-value-imports] The source contains a forbidden structure.\n  Code: CTC3006\n  Template: .ctmpl/no-value-imports.ts.ctmpl:1:1\n",
        );
}

#[test]
fn human_output_supports_forced_colors() {
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            fixture().to_str().unwrap(),
            "--rule",
            "no-value-imports",
            "--color",
            "always",
        ])
        .assert()
        .code(1)
        .stdout(predicates::str::contains(
            "\u{1b}[36msrc/adapter.ts:1:1\u{1b}[0m",
        ))
        .stdout(predicates::str::contains(
            "\u{1b}[31mThe source contains a forbidden structure.\u{1b}[0m",
        ));
}

#[test]
fn json_output_never_contains_ansi_sequences() {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            fixture().to_str().unwrap(),
            "--rule",
            "no-value-imports",
            "--format",
            "json",
            "--color",
            "always",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stdout.contains(&0x1b));
    serde_json::from_slice::<Value>(&output.stdout).unwrap();
}

#[test]
fn ad_hoc_template_checks_an_explicit_source() {
    let template = fixture().join(".ctmpl/class-factory.ts.ctmpl");
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "src/logger.ts",
            "--root",
            fixture().to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(
            "{
  \"schemaVersion\": 1,
  \"matches\": true,
  \"diagnostics\": []
}\n",
        );
}

#[test]
fn mode_without_template_is_a_cli_error() {
    Command::cargo_bin("ctc")
        .unwrap()
        .args(["--mode", "contains"])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("CTC0001"));
}

#[test]
fn scope_without_template_is_a_cli_error() {
    Command::cargo_bin("ctc")
        .unwrap()
        .args(["--scope", "descendants"])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("CTC0001"));
}

#[test]
fn clap_error_preserves_requested_json_format() {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(["--format", "json", "--unknown-option"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "CTC0001");
}

#[test]
fn ad_hoc_template_requires_supported_template_suffix() {
    let invalid_template =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/invalid.txt.ctmpl");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "src/adapter.ts",
            "--root",
            fixture().to_str().unwrap(),
            "--template",
            invalid_template.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "CTC1008");
}

#[test]
fn missing_ad_hoc_source_is_a_command_line_error() {
    let template = fixture().join(".ctmpl/class-factory.ts.ctmpl");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "src/missing.ts",
            "--root",
            fixture().to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "CTC0001");
}

#[test]
fn overlapping_ad_hoc_paths_do_not_duplicate_diagnostics() {
    let template = fixture().join(".ctmpl/no-value-imports.ts.ctmpl");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "src",
            "src/adapter.ts",
            "--root",
            fixture().to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--mode",
            "forbid",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 3);
}

#[test]
fn check_subcommand_remains_a_compatibility_alias() {
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "check",
            "--root",
            fixture().to_str().unwrap(),
            "--rule",
            "class-factory",
        ])
        .assert()
        .success();
}

#[test]
fn explicit_config_path_overrides_default_discovery() {
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            fixture().to_str().unwrap(),
            "--config",
            ".ctc.json",
            "--rule",
            "class-factory",
        ])
        .assert()
        .success();
}

#[test]
fn descendants_scope_finds_nested_try_statement() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scopes");
    let template = root.join(".ctmpl/no-try.ts.ctmpl");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "source.ts",
            "--root",
            root.to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--mode",
            "forbid",
            "--scope",
            "descendants",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "CTC3006");
    assert_eq!(report["diagnostics"][0]["source"]["start"]["line"], 2);
}

#[test]
fn top_level_scope_does_not_find_nested_try_statement() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scopes");
    let template = root.join(".ctmpl/no-try.ts.ctmpl");
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "source.ts",
            "--root",
            root.to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--mode",
            "forbid",
            "--scope",
            "top-level",
        ])
        .assert()
        .success();
}

#[test]
fn configured_descendants_scope_finds_nested_try_statement() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scopes");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(["--root", root.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["ruleId"], "no-try");
    assert_eq!(report["diagnostics"][0]["source"]["start"]["line"], 2);
}

#[test]
fn configured_optional_async_keyword_accepts_both_forms() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/optional");
    Command::cargo_bin("ctc")
        .unwrap()
        .args(["--root", root.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn semantic_rules_report_control_flow_and_exception_policies() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/semantic");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(["--root", root.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = report["diagnostics"].as_array().unwrap();
    let count = |code: &str| {
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic["code"] == code)
            .count()
    };
    assert_eq!(count("CTC4001"), 4);
    assert_eq!(count("CTC4002"), 1);
    assert_eq!(count("CTC4101"), 1);
    assert_eq!(count("CTC4102"), 2);
    assert_eq!(count("CTC4103"), 4);
    assert_eq!(count("CTC4104"), 2);
}

#[test]
fn rule_filter_selects_one_semantic_policy() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/semantic");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            root.to_str().unwrap(),
            "--rule",
            "return-paths",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|diagnostic| diagnostic["code"].as_str().unwrap().starts_with("CTC40"))
    );
}

#[test]
fn every_mode_reports_each_class_with_members_out_of_order() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/member-order");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(["--root", root.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let found = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["ruleId"].as_str().unwrap().to_string(),
                diagnostic["source"]["path"].as_str().unwrap().to_string(),
                diagnostic["source"]["start"]["line"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let expected = [
        ("cpp-member-order", "src/point.hpp", 17),
        ("cpp-member-order", "src/point.hpp", 32),
        ("ts-member-order", "src/service.ts", 19),
        ("ts-member-order", "src/shapes.ts", 14),
        ("ts-member-order", "src/shapes.ts", 31),
        ("cpp-member-order", "src/widget.hpp", 17),
    ]
    .map(|(rule, path, line)| (rule.to_string(), path.to_string(), line));
    assert_eq!(found, expected);
}

fn file_name_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/file-name")
}

#[test]
fn file_name_filter_reports_names_that_do_not_match_the_file() {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            file_name_fixture().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let found = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["code"].as_str().unwrap().to_string(),
                diagnostic["category"].as_str().unwrap().to_string(),
                diagnostic["ruleId"].as_str().unwrap().to_string(),
                diagnostic["source"]["path"].as_str().unwrap().to_string(),
                diagnostic["expected"].as_str().unwrap().to_string(),
                diagnostic["actual"].as_str().unwrap().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let expected = [
        (
            "cpp-class-name",
            "include/user_service.hpp",
            "UserService",
            "UserManager",
        ),
        (
            "ts-class-name",
            "src/order-service.ts",
            "OrderService",
            "OrderManager",
        ),
    ]
    .map(|(rule, path, expected, actual)| {
        (
            "CTC3007".to_string(),
            "file-name-mismatch".to_string(),
            rule.to_string(),
            path.to_string(),
            expected.to_string(),
            actual.to_string(),
        )
    });
    assert_eq!(found, expected);
    assert_eq!(
        report["diagnostics"][1]["message"],
        "Capture `Name` must match the file name `order-service.ts`. The file name in PascalCase is `OrderService`."
    );
}

#[test]
fn file_name_filter_works_with_an_ad_hoc_template() {
    let root = file_name_fixture();
    let template = root.join(".ctmpl/class-name.ts.ctmpl");
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "src/user-service.ts",
            "--root",
            root.to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--mode",
            "contains",
        ])
        .assert()
        .success();
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "src/order-service.ts",
            "--root",
            root.to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--mode",
            "contains",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "CTC3007");
    assert_eq!(report["diagnostics"][0]["expected"], "OrderService");
}

#[test]
fn validates_file_name_templates_without_a_source() {
    for template in [
        "class-name.ts.ctmpl",
        "interface-name.ts.ctmpl",
        "class-name.hpp.ctmpl",
    ] {
        let template = file_name_fixture().join(".ctmpl").join(template);
        Command::cargo_bin("ctc")
            .unwrap()
            .args(["validate-template", template.to_str().unwrap()])
            .assert()
            .success();
    }
}

#[test]
fn every_mode_uses_cpp_member_fields_for_member_groups() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cpp-member-fields");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(["--root", root.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let found = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["ruleId"].as_str().unwrap().to_string(),
                diagnostic["source"]["path"].as_str().unwrap().to_string(),
                diagnostic["source"]["start"]["line"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let expected = [21, 30].map(|line| {
        (
            "cpp-member-fields".to_string(),
            "src/shape.hpp".to_string(),
            line,
        )
    });
    assert_eq!(found, expected);
}

#[test]
fn configured_cpp_exact_template_matches() {
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            cpp_fixture().to_str().unwrap(),
            "--rule",
            "class-factory",
        ])
        .assert()
        .success();
}

#[test]
fn configured_cpp_rule_reports_a_nested_throw() {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            cpp_fixture().to_str().unwrap(),
            "--rule",
            "no-throw",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["code"], "CTC3006");
    assert_eq!(diagnostics[0]["ruleId"], "no-throw");
    assert_eq!(diagnostics[0]["source"]["path"], "src/throwing.cpp");
}

#[test]
fn validates_cpp_header_template() {
    let template = cpp_fixture().join(".ctmpl/declaration.hpp.ctmpl");
    Command::cargo_bin("ctc")
        .unwrap()
        .args(["validate-template", template.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn rejects_a_cpp_source_with_a_typescript_template() {
    let template = fixture().join(".ctmpl/class-factory.ts.ctmpl");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "src/widget.cpp",
            "--root",
            cpp_fixture().to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "CTC1008");
}

fn rule_messages_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rule-messages")
}

#[test]
fn json_output_adds_rule_message_as_a_separate_field() {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            rule_messages_fixture().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = report["diagnostics"].as_array().unwrap();
    let rule_ids = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["ruleId"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        rule_ids.into_iter().collect::<Vec<_>>(),
        ["no-throw", "no-try", "return-paths"]
    );
    for diagnostic in diagnostics {
        match diagnostic["ruleId"].as_str().unwrap() {
            "no-try" => {
                assert_eq!(
                    diagnostic["message"],
                    "The source contains a forbidden structure."
                );
                assert_eq!(
                    diagnostic["ruleMessage"],
                    "Return a Result value instead of catching exceptions."
                );
            }
            "no-throw" => {
                assert_eq!(
                    diagnostic["message"],
                    "The source contains a forbidden structure."
                );
                assert!(diagnostic.get("ruleMessage").is_none());
            }
            "return-paths" => {
                assert!(
                    diagnostic["message"]
                        .as_str()
                        .unwrap()
                        .starts_with("Function `save`")
                );
                assert_eq!(
                    diagnostic["ruleMessage"],
                    "Each Result function must return a value on every path."
                );
            }
            other => panic!("unexpected rule `{other}`"),
        }
    }
}

#[test]
fn human_output_shows_rule_message_first_and_keeps_engine_detail() {
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            rule_messages_fixture().to_str().unwrap(),
            "--rule",
            "no-try",
            "--color",
            "never",
        ])
        .assert()
        .code(1)
        .stdout(
            "src/service.ts:4:3 [no-try] Return a Result value instead of catching exceptions.\n  Detail: The source contains a forbidden structure.\n  Code: CTC3006\n  Template: .ctmpl/no-try.ts.ctmpl:1:1\n",
        );
}

#[test]
fn human_output_without_rule_message_is_unchanged() {
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "--root",
            rule_messages_fixture().to_str().unwrap(),
            "--rule",
            "no-throw",
            "--color",
            "never",
        ])
        .assert()
        .code(1)
        .stdout(
            "src/service.ts:16:3 [no-throw] The source contains a forbidden structure.\n  Code: CTC3006\n  Template: .ctmpl/no-throw.ts.ctmpl:1:1\n",
        );
}

#[test]
fn ad_hoc_template_accepts_a_rule_message() {
    let root = rule_messages_fixture();
    let template = root.join(".ctmpl/no-try.ts.ctmpl");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "src/service.ts",
            "--root",
            root.to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--mode",
            "forbid",
            "--scope",
            "descendants",
            "--message",
            "Do not catch exceptions here.",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0]["ruleId"], "command-line");
    assert_eq!(
        diagnostics[0]["ruleMessage"],
        "Do not catch exceptions here."
    );
}

#[test]
fn message_without_template_is_a_cli_error() {
    Command::cargo_bin("ctc")
        .unwrap()
        .args(["--message", "Custom text."])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("CTC0001"));
}

#[test]
fn empty_ad_hoc_message_is_a_cli_error() {
    let root = rule_messages_fixture();
    let template = root.join(".ctmpl/no-try.ts.ctmpl");
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "src/service.ts",
            "--root",
            root.to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--message",
            "",
        ])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("CTC0001"));
}

#[test]
fn empty_configured_rule_message_is_a_configuration_error() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("empty-rule-message");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".ctmpl")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join(".ctmpl/no-try.ts.ctmpl"),
        "{{ Forbidden | kind(\"TryStatement\") }}\n",
    )
    .unwrap();
    std::fs::write(root.join("src/a.ts"), "export {};\n").unwrap();
    std::fs::write(
        root.join(".ctc.json"),
        r#"{
  "schemaVersion": 1,
  "rules": [{
    "id": "no-try",
    "template": ".ctmpl/no-try.ts.ctmpl",
    "include": ["src/**/*.ts"],
    "mode": "forbid",
    "message": ""
  }]
}"#,
    )
    .unwrap();
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(["--root", root.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "CTC1010");
    assert_eq!(
        report["diagnostics"][0]["message"],
        "Rule `no-try` has an empty message."
    );
    assert!(report["diagnostics"][0].get("ruleMessage").is_none());
}

fn suppression_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/suppressions")
}

fn run_suppression_fixture(paths: &[&str], format: &str) -> std::process::Output {
    let root = suppression_fixture();
    let mut arguments = vec!["--root", root.to_str().unwrap(), "--format", format];
    arguments.extend_from_slice(paths);
    Command::cargo_bin("ctc")
        .unwrap()
        .args(arguments)
        .output()
        .unwrap()
}

fn diagnostic_summary(report: &Value) -> Vec<(String, String, String, u64)> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["code"].as_str().unwrap().to_string(),
                diagnostic["ruleId"].as_str().unwrap_or("").to_string(),
                diagnostic["source"]["path"].as_str().unwrap().to_string(),
                diagnostic["source"]["start"]["line"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn suppression_comments_skip_matching_diagnostics() {
    let output = run_suppression_fixture(&["src"], "json");
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["matches"], false);
    assert_eq!(
        diagnostic_summary(&report),
        vec![(
            "CTC3006".to_string(),
            "no-throw-ts".to_string(),
            "src/service.ts".to_string(),
            8,
        )]
    );
}

#[test]
fn suppression_comments_keep_human_output_consistent() {
    let output = run_suppression_fixture(&["src"], "human");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "src/service.ts:8:3 [no-throw-ts] The source contains a forbidden structure.\n  Code: CTC3006\n  Template: .ctmpl/no-throw.ts.ctmpl:1:1\n"
    );
}

#[test]
fn fully_suppressed_files_pass() {
    let output = run_suppression_fixture(
        &["src/generated.ts", "src/legacy.cpp", "src/parser.cpp"],
        "json",
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["matches"], true);
    assert_eq!(report["diagnostics"], json!([]));
}

#[test]
fn invalid_suppression_comments_are_reported() {
    let output = run_suppression_fixture(&["other/invalid.ts"], "json");
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let summary = diagnostic_summary(&report);
    let expected_lines = [1, 4, 7];
    assert_eq!(summary.len(), expected_lines.len(), "{summary:?}");
    for ((code, rule, path, line), expected_line) in summary.iter().zip(expected_lines) {
        assert_eq!(code, "CTC5001");
        assert_eq!(rule, "");
        assert_eq!(path, "other/invalid.ts");
        assert_eq!(*line, expected_line);
    }
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["category"] == "invalid-suppression")
    );
    assert_eq!(
        diagnostics[0]["message"],
        "A `ctc-ignore-next-line` comment must name at least one rule identifier."
    );
    assert_eq!(
        diagnostics[1]["message"],
        "The suppression comment names unknown rule identifier `missing-rule`."
    );
    assert_eq!(
        diagnostics[2]["message"],
        "Unknown suppression directive `ctc-ignore-line`. Use `ctc-ignore-next-line` or `ctc-ignore-file`."
    );
}

#[test]
fn suppressions_can_name_rules_that_the_command_did_not_select() {
    let output = run_suppression_fixture(&["--rule", "no-try-ts", "src/service.ts"], "json");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn ad_hoc_templates_ignore_suppression_comments() {
    let template = suppression_fixture().join(".ctmpl/no-throw.cpp.ctmpl");
    let output = run_suppression_fixture(
        &[
            "--template",
            template.to_str().unwrap(),
            "--mode",
            "forbid",
            "--scope",
            "descendants",
            "src/parser.cpp",
        ],
        "json",
    );
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        diagnostic_summary(&report),
        vec![(
            "CTC3006".to_string(),
            "command-line".to_string(),
            "src/parser.cpp".to_string(),
            4,
        )]
    );
}

fn cross_file_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cross-file")
}

fn run_json(args: &[&str]) -> (Option<i32>, Value) {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(args)
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "invalid JSON output: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (output.status.code(), report)
}

#[test]
fn companion_file_rule_reports_a_missing_test_file() {
    let root = cross_file_fixture();
    let (code, report) = run_json(&[
        "--root",
        root.to_str().unwrap(),
        "--rule",
        "unit-test-exists",
        "--format",
        "json",
    ]);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(
        report["diagnostics"],
        json!([{
            "code": "CTC4201",
            "category": "missing-partner-file",
            "message": "No companion file found for `src/lib/beta.ts`. Expected one of: `src/lib/beta.test.ts`, `test/lib/beta.test.ts`.",
            "ruleId": "unit-test-exists",
            "source": {
                "path": "src/lib/beta.ts",
                "start": { "offset": 0, "line": 1, "column": 1 },
                "end": { "offset": 0, "line": 1, "column": 1 }
            }
        }])
    );
}

fn partner_diagnostic_summary(report: &Value) -> Vec<(String, String, u64, String)> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["code"].as_str().unwrap().to_string(),
                diagnostic["source"]["path"].as_str().unwrap().to_string(),
                diagnostic["source"]["start"]["line"].as_u64().unwrap(),
                diagnostic["message"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

const MISSING_MAKE: &str = "`ui::Widget::make(int)` is declared here but not defined in `src/widget.cpp`. A definition with other parameters exists at `src/widget.cpp:19`: `ui::Widget::make(long)`.";
const RESET_ORDER: &str = "Definition of `ui::Widget::reset()` is out of order. `include/widget.hpp:16` declares it after `ui::Widget::rename(const std::string&)`, so define it after that function.";

#[test]
fn header_source_pairing_reports_missing_and_out_of_order_definitions() {
    let root = cross_file_fixture();
    let (code, report) = run_json(&[
        "--root",
        root.to_str().unwrap(),
        "--rule",
        "header-source-pairing",
        "--format",
        "json",
    ]);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(
        partner_diagnostic_summary(&report),
        [
            (
                "CTC4201".to_string(),
                "include/gadget.hpp".to_string(),
                1,
                "No source file found for header `include/gadget.hpp`, which declares member functions that need definitions. Expected one of: `include/gadget.cpp`, `src/gadget.cpp`.".to_string()
            ),
            (
                "CTC4202".to_string(),
                "include/widget.hpp".to_string(),
                19,
                MISSING_MAKE.to_string()
            ),
            (
                "CTC4203".to_string(),
                "src/widget.cpp".to_string(),
                15,
                RESET_ORDER.to_string()
            ),
        ]
    );
    let order = &report["diagnostics"][2];
    assert_eq!(order["category"], "member-definition-order");
    assert_eq!(order["ruleId"], "header-source-pairing");
    assert_eq!(order["expected"], "position 7 of 8 (header order)");
    assert_eq!(order["actual"], "position 6 of 8");
}

#[test]
fn header_source_pairing_can_skip_missing_sources_and_order() {
    let root = cross_file_fixture();
    let (code, report) = run_json(&[
        "--root",
        root.to_str().unwrap(),
        "--config",
        "skip.ctc.json",
        "--format",
        "json",
    ]);
    assert_eq!(code, Some(1), "{report}");
    assert_eq!(
        partner_diagnostic_summary(&report),
        [(
            "CTC4202".to_string(),
            "include/widget.hpp".to_string(),
            19,
            MISSING_MAKE.to_string()
        )]
    );
}

#[test]
fn explicit_header_path_still_reads_its_source_file() {
    let root = cross_file_fixture();
    let (code, report) = run_json(&[
        "include/widget.hpp",
        "--root",
        root.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(code, Some(1), "{report}");
    let summary = partner_diagnostic_summary(&report);
    assert_eq!(
        summary
            .iter()
            .map(|(code, path, line, _)| (code.as_str(), path.as_str(), *line))
            .collect::<Vec<_>>(),
        [
            ("CTC4202", "include/widget.hpp", 19),
            ("CTC4203", "src/widget.cpp", 15)
        ]
    );
}

#[test]
fn header_source_pairing_accepts_matching_files() {
    let root = cross_file_fixture();
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "include/net/socket.hpp",
            "include/inline_only.hpp",
            "--root",
            root.to_str().unwrap(),
        ])
        .assert()
        .success();
}

fn member_order_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/member-order")
}

fn explain_json(root: &std::path::Path, arguments: &[&str]) -> (Option<i32>, Value) {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .arg("explain")
        .args(arguments)
        .args(["--root", root.to_str().unwrap(), "--format", "json"])
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stdout)));
    (output.status.code(), report)
}

#[test]
fn explain_shows_member_groups_and_why_member_order_fails() {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "explain",
            "src/service.ts",
            "--rule",
            "ts-member-order",
            "--root",
            member_order_fixture().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    for expected in [
        "Rule: ts-member-order (every, descendants)",
        "Template: .ctmpl/member-order.ts.ctmpl",
        "Source: src/service.ts",
        "Result: does not match (1 of 3 candidates failed)",
        "Candidate 1 at src/service.ts:1:8: template matches",
        "Candidate 3 at src/service.ts:16:8: template does not match",
        "{{* Constructors }} -> 1 node",
        "2:3 MethodDefinition `constructor(private readonly name: string) {}`",
        "{{* PrivateMembers }} -> 1 node",
        "17:3 PublicFieldDefinition `private count = 0`",
        "Stopped at src/service.ts:19:3: The source contains an unexpected syntax node.",
        "Why: The node at line 19 (MethodDefinition) passes group `PublicMembers`, but the later group `PrivateMembers` already started at line 17.",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in:\n{stdout}"
        );
    }
}

#[test]
fn explain_json_lists_one_candidate_per_class() {
    let (code, report) = explain_json(
        &member_order_fixture(),
        &["src/service.ts", "--rule", "ts-member-order"],
    );
    assert_eq!(code, Some(1));
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["ruleId"], "ts-member-order");
    assert_eq!(report["mode"], "every");
    assert_eq!(report["scope"], "descendants");
    assert_eq!(report["matches"], false);
    let candidates = report["candidates"].as_array().unwrap();
    let matched = candidates
        .iter()
        .map(|candidate| candidate["templateMatched"].as_bool().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(matched, [true, true, false]);
    let failed = &candidates[2];
    let private = failed["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["name"] == "PrivateMembers")
        .unwrap();
    assert_eq!(private["kind"], "sequence");
    assert_eq!(private["nodes"][0]["range"]["start"]["line"], 17);
    assert_eq!(private["nodes"][0]["text"], "private count = 0");
    assert_eq!(failed["failure"]["code"], "CTC3003");
    assert_eq!(failed["failure"]["source"]["start"]["line"], 19);
    assert!(
        failed["failure"]["reason"]
            .as_str()
            .unwrap()
            .contains("passes group `PublicMembers`")
    );
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
}

#[test]
fn explain_cpp_member_order_names_the_constructor_group() {
    let (code, report) = explain_json(
        &member_order_fixture(),
        &["src/widget.hpp", "--rule", "cpp-member-order"],
    );
    assert_eq!(code, Some(1));
    let candidates = report["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2);
    let failure = &candidates[1]["failure"];
    assert_eq!(failure["source"]["start"]["line"], 17);
    assert_eq!(
        failure["reason"],
        "The node at line 17 (Declaration) passes group `Constructors`, but the later group `PublicMembers` already started at line 16."
    );
}

#[test]
fn explain_ad_hoc_template_reports_a_match() {
    let template = fixture().join(".ctmpl/class-factory.ts.ctmpl");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "explain",
            "src/logger.ts",
            "--template",
            template.to_str().unwrap(),
            "--root",
            fixture().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("Result: matches"), "{stdout}");
    assert!(
        stdout.contains("Candidate 1 at src/logger.ts:1:1: template matches"),
        "{stdout}"
    );
}

#[test]
fn explain_forbid_mode_shows_each_forbidden_match() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scopes");
    let template = root.join(".ctmpl/no-try.ts.ctmpl");
    let (code, report) = explain_json(
        &root,
        &[
            "source.ts",
            "--template",
            template.to_str().unwrap(),
            "--mode",
            "forbid",
            "--scope",
            "descendants",
        ],
    );
    assert_eq!(code, Some(1));
    let candidates = report["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["templateMatched"], true);
    assert_eq!(candidates[0]["source"]["start"]["line"], 2);
}

#[test]
fn explain_rejects_invalid_arguments() {
    let root = member_order_fixture();
    let template = root.join(".ctmpl/member-order.ts.ctmpl");
    let template = template.to_str().unwrap();
    let semantic = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/semantic");
    let cases: Vec<(&PathBuf, Vec<&str>, &str)> = vec![
        (
            &root,
            vec![
                "src/service.ts",
                "--rule",
                "ts-member-order",
                "--template",
                template,
            ],
            "CTC0001",
        ),
        (&root, vec!["src/service.ts"], "CTC0001"),
        (
            &root,
            vec![
                "src/service.ts",
                "--rule",
                "ts-member-order",
                "--mode",
                "exact",
            ],
            "CTC0001",
        ),
        (
            &root,
            vec!["src/widget.hpp", "--rule", "ts-member-order"],
            "CTC1007",
        ),
        (
            &semantic,
            vec!["src/policies.ts", "--rule", "return-paths"],
            "CTC0001",
        ),
    ];
    for (root, arguments, code) in cases {
        let (exit, report) = explain_json(root, &arguments);
        assert_eq!(exit, Some(2), "{arguments:?}: {report}");
        assert_eq!(
            report["diagnostics"][0]["code"], code,
            "{arguments:?}: {report}"
        );
    }
}
