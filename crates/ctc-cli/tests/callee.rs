use std::fs;

use assert_cmd::Command;
use serde_json::Value;

pub mod common;

use common::{Project, lines};

// callee field

const CALLS: &str = "export function run(code: string) {\n  eval(code);\n  const a = eval(\"1\");\n  window.eval(code);\n  const f = new Function(\"return 1\");\n  const g = new Other();\n  helper(eval(code));\n  return evaluate(1) + eval(code);\n}\n";

fn callee_project(template: &str) -> Project {
    let project = Project::new();
    project.write(".ctmpl/call.ts.ctmpl", template);
    project.write("src/a.ts", CALLS);
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-call",
            "template": ".ctmpl/call.ts.ctmpl",
            "include": ["src/**/*.ts"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    project
}

#[test]
fn callee_field_finds_a_call_in_every_position() {
    let project = callee_project(
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"equal\", \"eval\") }}\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(lines(&report), vec![2, 3, 7, 8]);
}

#[test]
fn callee_field_uses_the_whole_callee_text() {
    let project = callee_project(
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"equal\", \"window.eval\") }}\n",
    );
    let (_, report) = project.run(&[]);
    assert_eq!(lines(&report), vec![4]);
}

#[test]
fn callee_field_covers_new_expressions() {
    let project = callee_project(
        "{{ New | kind(\"NewExpression\") | field(\"callee\", \"equal\", \"Function\") }}\n",
    );
    let (_, report) = project.run(&[]);
    assert_eq!(lines(&report), vec![5]);
}

#[test]
fn callee_field_can_exclude_a_name() {
    let project = callee_project(
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"notEqual\", \"eval\") }}\n",
    );
    let (_, report) = project.run(&[]);
    assert_eq!(lines(&report), vec![4, 7, 8]);
}

#[test]
fn templates_with_calls_still_match_without_a_callee_field() {
    let project = callee_project("eval({{* Args }});\n");
    let (_, report) = project.run(&[]);
    assert_eq!(lines(&report), vec![2]);

    let project = callee_project("{{ Fn }}({{* Args }});\n");
    let (_, report) = project.run(&[]);
    assert_eq!(lines(&report), vec![2, 4, 7]);
}

#[test]
fn field_matches_tests_the_field_text_with_a_regular_expression() {
    let project = callee_project(
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"matches\", \"^(window\\\\.)?eval$\") }}\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(lines(&report), vec![2, 3, 4, 7, 8]);
}

#[test]
fn field_matches_is_not_anchored() {
    let project = callee_project(
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"matches\", \"val\") }}\n",
    );
    let (_, report) = project.run(&[]);
    assert_eq!(lines(&report), vec![2, 3, 4, 7, 8, 8]);
}

#[test]
fn field_not_matches_passes_for_other_names_and_missing_fields() {
    let project = callee_project(
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"notMatches\", \"eval\") }}\n",
    );
    let (_, report) = project.run(&[]);
    assert_eq!(lines(&report), vec![7]);

    let project = callee_project("{{ Node | field(\"callee\", \"matches\", \".\") }}\n");
    let (_, report) = project.run(&[]);
    assert!(!lines(&report).is_empty());
    let project = callee_project(
        "{{ Node | kind(\"Identifier\") | field(\"callee\", \"matches\", \".\") }}\n",
    );
    let (code, _) = project.run(&[]);
    assert_eq!(code, 0);
    let project = callee_project(
        "{{ Node | kind(\"Identifier\") | field(\"callee\", \"notMatches\", \"eval\") }}\n",
    );
    let (code, _) = project.run(&[]);
    assert_eq!(code, 1);
}

#[test]
fn field_matches_needs_a_valid_regular_expression() {
    let project = Project::new();
    for template in [
        "{{ C | kind(\"CallExpression\") | field(\"callee\", \"matches\", \"(\") }}\n",
        "{{ C | kind(\"CallExpression\") | field(\"callee\", \"matches\") }}\n",
        "{{ C | kind(\"CallExpression\") | field(\"callee\", \"matches\", 3) }}\n",
        "{{ C | kind(\"CallExpression\") | field(\"callee\", \"notMatches\", true) }}\n",
    ] {
        project.write("rule.ts.ctmpl", template);
        let output = Command::cargo_bin("ctc")
            .unwrap()
            .args([
                "validate-template",
                project.dir.path().join("rule.ts.ctmpl").to_str().unwrap(),
                "--format",
                "json",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{template}");
    }
}

#[test]
fn contains_rule_fails_when_no_field_matches_the_pattern() {
    let project = callee_project(
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"matches\", \"^eval$\") }}\n",
    );
    project.write("src/b.ts", "helper(1);\n");
    let mut config: Value =
        serde_json::from_str(&fs::read_to_string(project.dir.path().join(".ctc.json")).unwrap())
            .unwrap();
    config["rules"][0]["mode"] = Value::String("contains".to_string());
    config["rules"][0]["include"] = serde_json::json!(["src/b.ts"]);
    project.config(&config);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(report["diagnostics"][0]["code"], "CTC3004");
}

#[test]
fn cpp_callee_field_finds_calls() {
    let project = Project::new();
    project.write(
        ".ctmpl/call.cpp.ctmpl",
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"equal\", \"std::system\") }}\n",
    );
    project.write(
        "src/a.cpp",
        "#include <cstdlib>\nvoid run() {\n  std::system(\"ls\");\n  system(\"ls\");\n  int x = std::system(\"pwd\");\n}\n",
    );
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-system",
            "template": ".ctmpl/call.cpp.ctmpl",
            "include": ["src/**/*.cpp"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(lines(&report), vec![3, 5]);
}
