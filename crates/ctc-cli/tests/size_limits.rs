use assert_cmd::Command;

pub mod common;

use common::{Project, entry, summary};

// size limits

#[test]
fn line_count_field_reports_functions_that_are_too_long() {
    let project = Project::new();
    project.write(
        ".ctmpl/long-function.ts.ctmpl",
        "{{ Long | kind(\"FunctionDeclaration\") | field(\"lineCount\", \"greaterThan\", 4) }}\n",
    );
    project.write(
        "src/a.ts",
        "export function short() {\n  return 1;\n}\n\nexport function long() {\n  const a = 1;\n  const b = 2;\n  const c = 3;\n  return a + b + c;\n}\n",
    );
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "short-functions",
            "template": ".ctmpl/long-function.ts.ctmpl",
            "include": ["src/**/*.ts"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3006", "short-functions", "src/a.ts", 5)]
    );
}

#[test]
fn line_count_ordering_operators_include_their_limits() {
    for (operator, limit, expected) in [
        ("lessThan", 3, false),
        ("lessThanOrEqual", 3, true),
        ("greaterThan", 3, false),
        ("greaterThanOrEqual", 3, true),
        ("lessThan", 4, true),
        ("greaterThan", 2, true),
    ] {
        let project = Project::new();
        project.write(
            ".ctmpl/f.ts.ctmpl",
            &format!(
                "{{{{ F | kind(\"FunctionDeclaration\") | field(\"lineCount\", \"{operator}\", {limit}) }}}}\n"
            ),
        );
        project.write("src/a.ts", "export function f() {\n  return 1;\n}\n");
        project.config(&serde_json::json!({
            "schemaVersion": 1,
            "rules": [{
                "id": "size",
                "template": ".ctmpl/f.ts.ctmpl",
                "include": ["src/**/*.ts"],
                "mode": "forbid",
                "scope": "descendants"
            }]
        }));
        let (code, _) = project.run(&[]);
        assert_eq!(code == 1, expected, "{operator} {limit}");
    }
}

#[test]
fn ordering_operators_need_a_number() {
    let project = Project::new();
    for template in [
        "{{ F | kind(\"FunctionDeclaration\") | field(\"lineCount\", \"lessThan\") }}\n",
        "{{ F | kind(\"FunctionDeclaration\") | field(\"lineCount\", \"lessThan\", \"ten\") }}\n",
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

fn file_length_project(max_lines: u64) -> Project {
    let project = Project::new();
    project.write(
        "src/three.ts",
        "export const a = 1;\nexport const b = 2;\nexport const c = 3;\n",
    );
    project.write(
        "src/five.ts",
        "export const a = 1;\nexport const b = 2;\nexport const c = 3;\nexport const d = 4;\nexport const e = 5;\n",
    );
    project.write(
        "src/six.cpp",
        "int a;\nint b;\nint c;\nint d;\nint e;\nint f;\n",
    );
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "semanticRules": [{
            "kind": "fileLength",
            "id": "short-files",
            "include": ["src/**/*.ts", "src/**/*.cpp"],
            "maxLines": max_lines,
            "message": "Split this file."
        }]
    }));
    project
}

#[test]
fn file_length_rule_points_at_the_first_line_over_the_limit() {
    let project = file_length_project(3);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC4301", "short-files", "src/five.ts", 4),
            entry("CTC4301", "short-files", "src/six.cpp", 4),
        ]
    );
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["expected"], "at most 3 lines");
    assert_eq!(diagnostic["actual"], "5 lines");
    assert_eq!(diagnostic["ruleMessage"], "Split this file.");
}

#[test]
fn file_length_rule_accepts_files_at_the_limit() {
    let project = file_length_project(6);
    let (code, report) = project.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn file_length_rule_needs_a_positive_limit() {
    let project = file_length_project(0);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 2);
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("maxLines of 0")
    );
}
