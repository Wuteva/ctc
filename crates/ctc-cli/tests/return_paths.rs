pub mod common;

use common::{Project, entry, summary};

fn return_paths_project(config_extra: serde_json::Value) -> Project {
    let project = Project::new();
    project.write(
        "src/a.ts",
        "type Result<T> = T | Error;\nexport function ok(flag: boolean): Result<number> {\n  if (flag) {\n    return 1;\n  }\n  return 2;\n}\nexport function falls(flag: boolean): Result<number> {\n  if (flag) {\n    return 1;\n  }\n}\nexport function bare(flag: boolean): Result<number> {\n  if (flag) {\n    return;\n  }\n  return 3;\n}\nexport function other(flag: boolean): number | undefined {\n  if (flag) {\n    return 1;\n  }\n}\n",
    );
    let mut rule = serde_json::json!({
        "kind": "returnPaths",
        "id": "return-paths",
        "include": ["src/**/*.ts"]
    });
    rule.as_object_mut()
        .unwrap()
        .extend(config_extra.as_object().unwrap().clone());
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "semanticRules": [rule]
    }));
    project
}

#[test]
fn return_paths_checks_functions_that_return_the_named_type() {
    let project = return_paths_project(serde_json::json!({ "typeName": "Result" }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC4001", "return-paths", "src/a.ts", 8),
            entry("CTC4001", "return-paths", "src/a.ts", 13),
            entry("CTC4002", "return-paths", "src/a.ts", 15),
        ]
    );
}

#[test]
fn return_paths_needs_a_type_name() {
    let project = return_paths_project(serde_json::json!({}));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 2);
    assert_eq!(report["diagnostics"][0]["code"], "CTC1010");
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("typeName")
    );
}

#[test]
fn the_old_rule_name_is_not_accepted() {
    let project = return_paths_project(serde_json::json!({
        "kind": "resultOrControlFlow",
        "typeName": "Result"
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 2);
    assert_eq!(report["diagnostics"][0]["code"], "CTC1010");
}
