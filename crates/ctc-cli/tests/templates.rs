use std::fs;

use assert_cmd::Command;
use serde_json::Value;

pub mod common;

use common::{Project, entry, guard, summary};

// several templates

fn alternatives_project(mode: &str, templates: &[(&str, &str)]) -> Project {
    let project = Project::new();
    let mut paths = Vec::new();
    for (name, text) in templates {
        project.write(&format!(".ctmpl/{name}.ts.ctmpl"), text);
        paths.push(format!(".ctmpl/{name}.ts.ctmpl"));
    }
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "either",
            "template": paths,
            "include": ["src/**/*.ts"],
            "mode": mode,
            "scope": if mode == "exact" { "topLevel" } else { "descendants" }
        }]
    }));
    project
}

#[test]
fn exact_rule_accepts_a_file_that_any_template_accepts() {
    let project = alternatives_project(
        "exact",
        &[
            ("one", "export const {{ Name }} = 1;\n"),
            ("two", "export const {{ Name }} = 2;\n"),
        ],
    );
    project.write("src/one.ts", "export const a = 1;\n");
    project.write("src/two.ts", "export const b = 2;\n");
    let (code, report) = project.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));

    project.write("src/three.ts", "export const c = true;\n");
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(summary(&report).len(), 1, "{report}");
    assert_eq!(report["diagnostics"][0]["source"]["path"], "src/three.ts");
}

#[test]
fn several_templates_report_the_alternative_that_got_furthest() {
    let project = alternatives_project(
        "exact",
        &[
            (
                "first",
                "import {{ Name }} from \"x\";\nexport const a = 1;\n",
            ),
            (
                "second",
                "export const a = 1;\nexport const b = 2;\nexport const c = 3;\n",
            ),
        ],
    );
    project.write(
        "src/a.ts",
        "export const a = 1;\nexport const b = 2;\nexport const c = 4;\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["diagnostics"][0]["template"]["path"],
        ".ctmpl/second.ts.ctmpl"
    );
}

#[test]
fn contains_rule_needs_any_template_to_be_found() {
    let project = alternatives_project(
        "contains",
        &[
            ("class", "{{ C | kind(\"ClassDeclaration\") }}\n"),
            ("interface", "{{ I | kind(\"InterfaceDeclaration\") }}\n"),
        ],
    );
    project.write("src/a.ts", "export interface A {}\n");
    project.write("src/b.ts", "class B {}\n");
    let (code, report) = project.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));

    project.write("src/c.ts", "export const c = 1;\n");
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(report["diagnostics"][0]["source"]["path"], "src/c.ts");
}

#[test]
fn forbid_rule_reports_the_matches_of_every_template() {
    let project = alternatives_project(
        "forbid",
        &[
            ("try", "{{ Forbidden | kind(\"TryStatement\") }}\n"),
            ("throw", "{{ Forbidden | kind(\"ThrowStatement\") }}\n"),
        ],
    );
    project.write(
        "src/a.ts",
        "export function run() {\n  try {\n    throw new Error(\"x\");\n  } catch {}\n}\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC3006", "either", "src/a.ts", 2),
            entry("CTC3006", "either", "src/a.ts", 3),
        ]
    );
    let templates = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["template"]["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        templates,
        vec![".ctmpl/try.ts.ctmpl", ".ctmpl/throw.ts.ctmpl"]
    );
}

#[test]
fn several_templates_have_configuration_limits() {
    let project = alternatives_project(
        "contains",
        &[
            ("class", "{{ C | kind(\"ClassDeclaration\") }}\n"),
            ("interface", "{{ I | kind(\"InterfaceDeclaration\") }}\n"),
        ],
    );
    project.write("src/a.ts", "class A {}\n");
    let base: Value =
        serde_json::from_str(&fs::read_to_string(project.dir.path().join(".ctc.json")).unwrap())
            .unwrap();
    for (patch, message) in [
        (
            serde_json::json!({ "count": { "max": 1 } }),
            "uses `count` with several templates",
        ),
        (
            serde_json::json!({ "mode": "every" }),
            "uses every mode with several templates but no `kinds`",
        ),
        (
            serde_json::json!({ "template": [] }),
            "must list at least one template",
        ),
        (
            serde_json::json!({ "template": [".ctmpl/class.ts.ctmpl", ".ctmpl/missing.ts.ctmpl"] }),
            "cannot resolve template",
        ),
    ] {
        let mut config = base.clone();
        for (key, value) in patch.as_object().unwrap() {
            config["rules"][0][key] = value.clone();
        }
        project.config(&config);
        let (code, report) = project.run(&[]);
        assert_eq!(code, 2, "{message}");
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
fn templates_of_different_languages_cannot_be_mixed() {
    let project = Project::new();
    project.write(
        ".ctmpl/a.ts.ctmpl",
        "{{ C | kind(\"ClassDeclaration\") }}\n",
    );
    project.write(
        ".ctmpl/b.cpp.ctmpl",
        "{{ C | kind(\"ClassDeclaration\") }}\n",
    );
    project.write("src/a.ts", "class A {}\n");
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "mixed",
            "template": [".ctmpl/a.ts.ctmpl", ".ctmpl/b.cpp.ctmpl"],
            "include": ["src/**/*.ts"],
            "mode": "contains"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 2);
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("different languages")
    );
}

#[test]
fn explain_needs_a_rule_with_one_template() {
    let project = alternatives_project(
        "contains",
        &[
            ("class", "{{ C | kind(\"ClassDeclaration\") }}\n"),
            ("interface", "{{ I | kind(\"InterfaceDeclaration\") }}\n"),
        ],
    );
    project.write("src/a.ts", "class A {}\n");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "explain",
            "src/a.ts",
            "--rule",
            "either",
            "--root",
            project.dir.path().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("lists several templates")
    );
}

#[test]
fn guard_reports_a_changed_template_in_a_list() {
    let project = alternatives_project(
        "contains",
        &[
            ("class", "{{ C | kind(\"ClassDeclaration\") }}\n"),
            ("interface", "{{ I | kind(\"InterfaceDeclaration\") }}\n"),
        ],
    );
    project.write("src/a.ts", "class A {}\n");
    project.init_git();
    project.commit_all("base");
    project.write(
        ".ctmpl/interface.ts.ctmpl",
        "{{ I | kind(\"EnumDeclaration\") }}\n",
    );
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5204", "either", ".ctc.json", 1)]
    );
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("content of template `.ctmpl/interface.ts.ctmpl`")
    );
}
