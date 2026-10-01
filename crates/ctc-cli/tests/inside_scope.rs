use assert_cmd::Command;
use serde_json::Value;

pub mod common;

use common::{Project, summary};

// inside scope

const LOOPS: &str = "export async function run(items: number[]) {\n  await start();\n  for (const item of items) {\n    await work(item);\n    items.map(async (value) => await work(value));\n  }\n  while (items.length > 0) {\n    if (items.length > 1) {\n      await work(1);\n    }\n  }\n}\n";

fn loop_project(scope: Value) -> Project {
    let project = Project::new();
    project.write(
        ".ctmpl/no-await.ts.ctmpl",
        "{{ Forbidden | kind(\"AwaitExpression\") }}\n",
    );
    project.write("src/a.ts", LOOPS);
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-await-in-loop",
            "template": ".ctmpl/no-await.ts.ctmpl",
            "include": ["src/**/*.ts"],
            "mode": "forbid",
            "scope": scope
        }]
    }));
    project
}

#[test]
fn inside_scope_finds_awaits_in_loops_and_skips_nested_functions() {
    let project = loop_project(serde_json::json!({
        "inside": ["ForInStatement", "ForStatement", "WhileStatement", "DoStatement"]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            common::entry("CTC3006", "no-await-in-loop", "src/a.ts", 4),
            common::entry("CTC3006", "no-await-in-loop", "src/a.ts", 9),
        ]
    );
}

#[test]
fn inside_scope_can_search_nested_functions() {
    let project = loop_project(serde_json::json!({
        "inside": ["ForInStatement", "WhileStatement"],
        "stopAtFunctions": false
    }));
    let (_, report) = project.run(&[]);
    let lines = summary(&report)
        .into_iter()
        .map(|(_, _, _, line)| line)
        .collect::<Vec<_>>();
    assert_eq!(lines, vec![4, 5, 9]);
}

#[test]
fn inside_scope_rejects_unknown_and_missing_kinds() {
    for (scope, message) in [
        (
            serde_json::json!({ "inside": ["NotAKind"] }),
            "unknown kind `NotAKind` in its `inside` scope",
        ),
        (
            serde_json::json!({ "inside": [] }),
            "at least one kind in its `inside` scope",
        ),
    ] {
        let project = loop_project(scope);
        let (code, report) = project.run(&[]);
        assert_eq!(code, 2);
        assert!(
            report["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains(message),
            "{report}"
        );
    }
}

#[test]
fn inside_is_not_a_scope_name() {
    let project = loop_project(Value::String("inside".to_string()));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 2);
    assert_eq!(report["diagnostics"][0]["code"], "CTC1010");
}

#[test]
fn explain_supports_the_inside_scope() {
    let project = loop_project(serde_json::json!({ "inside": ["ForInStatement"] }));
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "explain",
            "src/a.ts",
            "--rule",
            "no-await-in-loop",
            "--root",
            project.dir.path().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["scope"], "inside");
}
