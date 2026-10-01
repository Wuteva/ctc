use std::fs;

use assert_cmd::Command;
use serde_json::{Value, json};
use tempfile::TempDir;

struct Project {
    dir: TempDir,
}

impl Project {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.dir.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn config(&self, value: &Value) {
        self.write(".ctc.json", &serde_json::to_string_pretty(value).unwrap());
    }

    fn run(&self, arguments: &[&str]) -> (i32, Value) {
        self.run_impl(None, arguments)
    }

    fn run_subcommand(&self, subcommand: &str, arguments: &[&str]) -> (i32, Value) {
        self.run_impl(Some(subcommand), arguments)
    }

    fn run_impl(&self, subcommand: Option<&str>, arguments: &[&str]) -> (i32, Value) {
        let mut command = Command::cargo_bin("ctc").unwrap();
        if let Some(subcommand) = subcommand {
            command.arg(subcommand);
        }
        let output = command
            .args(arguments)
            .args([
                "--root",
                self.dir.path().to_str().unwrap(),
                "--format",
                "json",
            ])
            .output()
            .unwrap();
        let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "not JSON ({error}): {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        (output.status.code().unwrap(), report)
    }
}

fn summary(report: &Value) -> Vec<(String, String, String, u64)> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["code"].as_str().unwrap().to_string(),
                diagnostic["ruleId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                diagnostic["source"]["path"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                diagnostic["source"]["start"]["line"]
                    .as_u64()
                    .unwrap_or_default(),
            )
        })
        .collect()
}

fn entry(code: &str, rule: &str, path: &str, line: u64) -> (String, String, String, u64) {
    (code.to_string(), rule.to_string(), path.to_string(), line)
}

#[test]
fn exact_mode_matches_a_struct_and_impl_shape() {
    let project = Project::new();
    project.write(
        ".ctmpl/shape.rs.ctmpl",
        "pub struct {{ Name }} {\n{{* Fields }}\n}\n\nimpl {{ Name }} {\n{{* Items }}\n}\n",
    );
    project.write(
        "src/lib.rs",
        "pub struct Widget {\n    value: usize,\n}\n\nimpl Widget {\n    pub fn new(value: usize) -> Self {\n        Self { value }\n    }\n}\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "shape",
            "template": ".ctmpl/shape.rs.ctmpl",
            "include": ["src/**/*.rs"],
            "mode": "exact"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn forbid_rules_ban_unwrap_and_selected_macros() {
    let project = Project::new();
    project.write(
        ".ctmpl/no-unwrap.rs.ctmpl",
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"equal\", \"unwrap\") }}\n",
    );
    project.write(
        ".ctmpl/no-panic.rs.ctmpl",
        "{{ Macro | kind(\"MacroInvocation\") | field(\"callee\", \"equal\", \"panic!\") }}\n",
    );
    project.write(
        ".ctmpl/no-println.rs.ctmpl",
        "{{ Macro | kind(\"MacroInvocation\") | field(\"callee\", \"equal\", \"println!\") }}\n",
    );
    project.write(
        "src/lib.rs",
        "fn run(value: Option<usize>) {\n    let _ = value.unwrap();\n    panic!(\"x\");\n    println!(\"debug\");\n}\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [
            {
                "id": "no-unwrap",
                "template": ".ctmpl/no-unwrap.rs.ctmpl",
                "include": ["src/**/*.rs"],
                "mode": "forbid",
                "scope": "descendants"
            },
            {
                "id": "no-panic",
                "template": ".ctmpl/no-panic.rs.ctmpl",
                "include": ["src/**/*.rs"],
                "mode": "forbid",
                "scope": "descendants"
            },
            {
                "id": "no-println",
                "template": ".ctmpl/no-println.rs.ctmpl",
                "include": ["src/**/*.rs"],
                "mode": "forbid",
                "scope": "descendants"
            }
        ]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    let mut actual = summary(&report);
    actual.sort();
    let mut expected = vec![
        entry("CTC3006", "no-unwrap", "src/lib.rs", 2),
        entry("CTC3006", "no-panic", "src/lib.rs", 3),
        entry("CTC3006", "no-println", "src/lib.rs", 4),
    ];
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn forbid_rule_can_ban_a_specific_allow_attribute() {
    let project = Project::new();
    project.write(".ctmpl/no-allow.rs.ctmpl", "#[allow(dead_code)]\n");
    project.write("src/lib.rs", "#[allow(dead_code)]\nfn keep() {}\n");
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-allow",
            "template": ".ctmpl/no-allow.rs.ctmpl",
            "include": ["src/**/*.rs"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3006", "no-allow", "src/lib.rs", 1)]
    );
}

#[test]
fn every_mode_checks_each_impl_block() {
    let project = Project::new();
    project.write(
        ".ctmpl/inherent.rs.ctmpl",
        "impl {{ Name }} {\n{{* Items }}\n}\n",
    );
    project.write(
        "src/lib.rs",
        "struct Widget;\n\nimpl Widget {\n    fn new() -> Self { Self }\n}\n\nimpl Widget {\n    fn value(&self) -> usize { 1 }\n}\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "inherent-only",
            "template": ".ctmpl/inherent.rs.ctmpl",
            "include": ["src/**/*.rs"],
            "mode": "every",
            "scope": "descendants"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn function_body_scope_finds_unwrap_inside_functions_only() {
    let project = Project::new();
    project.write(
        ".ctmpl/no-unwrap.rs.ctmpl",
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"equal\", \"unwrap\") }}\n",
    );
    project.write(
        "src/lib.rs",
        "struct unwrap;\nfn run(value: Option<usize>) {\n    let _ = value.unwrap();\n}\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-unwrap",
            "template": ".ctmpl/no-unwrap.rs.ctmpl",
            "include": ["src/**/*.rs"],
            "mode": "forbid",
            "scope": "functionBody"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3006", "no-unwrap", "src/lib.rs", 3)]
    );
}

#[test]
fn suppression_comments_work_for_rust_rules() {
    let project = Project::new();
    project.write(
        ".ctmpl/no-unwrap.rs.ctmpl",
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"equal\", \"unwrap\") }}\n",
    );
    project.write(
        "src/lib.rs",
        "fn run(value: Option<usize>) {\n    // ctc-ignore-next-line no-unwrap\n    let _ = value.unwrap();\n}\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-unwrap",
            "template": ".ctmpl/no-unwrap.rs.ctmpl",
            "include": ["src/**/*.rs"],
            "mode": "forbid",
            "scope": "descendants",
            "allowIgnore": true
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn line_count_rule_reports_long_rust_functions() {
    let project = Project::new();
    project.write(
        ".ctmpl/long.rs.ctmpl",
        "{{ Long | kind(\"FunctionDeclaration\") | field(\"lineCount\", \"greaterThan\", 3) }}\n",
    );
    project.write(
        "src/lib.rs",
        "fn short() {}\n\nfn long() {\n    let a = 1;\n    let b = 2;\n    let c = a + b;\n}\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "long-fn",
            "template": ".ctmpl/long.rs.ctmpl",
            "include": ["src/**/*.rs"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3006", "long-fn", "src/lib.rs", 3)]
    );
}

#[test]
fn semantic_rules_apply_to_rust_files() {
    let project = Project::new();
    project.write(
        "src/too_long.rs",
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n",
    );
    project.write("src/paired.rs", "fn paired() {}\n");
    project.write("src/paired.test.rs", "#[test]\nfn paired() {}\n");
    project.write("src/missing.rs", "fn missing() {}\n");
    project.config(&json!({
        "schemaVersion": 1,
        "semanticRules": [
            {
                "kind": "fileLength",
                "id": "short-rust",
                "include": ["src/**/*.rs"],
                "exclude": ["src/**/*.test.rs"],
                "maxLines": 3
            },
            {
                "kind": "companionFile",
                "id": "has-test",
                "include": ["src/**/*.rs"],
                "exclude": ["src/**/*.test.rs"],
                "companions": ["{dir}/{stem}.test.rs"]
            }
        ]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC4201", "has-test", "src/missing.rs", 1),
            entry("CTC4201", "has-test", "src/too_long.rs", 1),
            entry("CTC4301", "short-rust", "src/too_long.rs", 4),
        ]
    );
}

#[test]
fn coverage_sees_rust_files() {
    let project = Project::new();
    project.write(
        ".ctmpl/no-unwrap.rs.ctmpl",
        "{{ Call | kind(\"CallExpression\") }}\n",
    );
    project.write("src/covered.rs", "fn covered() { value(); }\n");
    project.write("src/uncovered.rs", "fn uncovered() {}\n");
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "covered",
            "template": ".ctmpl/no-unwrap.rs.ctmpl",
            "include": ["src/covered.rs"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    let (code, report) = project.run_subcommand("coverage", &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5101", "", "src/uncovered.rs", 1)]
    );
}

#[test]
fn explain_reports_rust_rule_matches() {
    let project = Project::new();
    project.write(
        ".ctmpl/no-unwrap.rs.ctmpl",
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"equal\", \"unwrap\") }}\n",
    );
    project.write(
        "src/lib.rs",
        "fn run(value: Option<usize>) {\n    let _ = value.unwrap();\n}\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-unwrap",
            "template": ".ctmpl/no-unwrap.rs.ctmpl",
            "include": ["src/**/*.rs"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    let (code, report) = project.run_subcommand("explain", &["src/lib.rs", "--rule", "no-unwrap"]);
    assert_eq!(code, 1);
    assert_eq!(report["ruleId"], "no-unwrap");
    assert_eq!(report["mode"], "forbid");
    assert_eq!(report["scope"], "descendants");
    assert_eq!(report["matches"], false);
    assert_eq!(report["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(report["candidates"][0]["templateMatched"], true);
    assert_eq!(report["diagnostics"][0]["code"], "CTC3006");
}

#[test]
fn file_name_filter_works_for_rust_modules() {
    let project = Project::new();
    project.write(
        ".ctmpl/module.rs.ctmpl",
        "mod {{ Name | fileName(\"snake_case\") }} {}\n",
    );
    project.write("src/user_service.rs", "mod user_service {}\n");
    project.write("src/widget.rs", "mod wrong_name {}\n");
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "module-name",
            "template": ".ctmpl/module.rs.ctmpl",
            "include": ["src/**/*.rs"],
            "mode": "exact"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3007", "module-name", "src/widget.rs", 1)]
    );
    assert_eq!(report["diagnostics"][0]["expected"], "widget");
    assert_eq!(report["diagnostics"][0]["actual"], "wrong_name");
}

#[test]
fn attribute_field_bans_attributes_by_name() {
    let project = Project::new();
    project.write(
        ".ctmpl/no-lint-escape.rs.ctmpl",
        "{{ Attr | field(\"attribute\", \"matches\", \"^(allow|expect)$\") }}\n",
    );
    project.write(
        "src/lib.rs",
        "#![allow(dead_code)]\n#[derive(Debug)]\npub struct A;\n#[allow(clippy::too_many_arguments)]\npub fn f() {}\n#[expect(unused)]\nfn g() {}\n#[clippy::allow_me]\nfn h() {}\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-lint-escape",
            "template": ".ctmpl/no-lint-escape.rs.ctmpl",
            "include": ["src/**/*.rs"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    let lines = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["source"]["start"]["line"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines, vec![1, 4, 6]);
}
