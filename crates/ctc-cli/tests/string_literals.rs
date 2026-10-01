pub mod common;

use common::{Project, entry, summary};

// string literals in templates

fn string_project(template_name: &str, template: &str, source_name: &str, source: &str) -> Project {
    let project = Project::new();
    project.write(&format!(".ctmpl/{template_name}"), template);
    project.write(&format!("src/{source_name}"), source);
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "string-rule",
            "template": format!(".ctmpl/{template_name}"),
            "include": [format!("src/{source_name}")],
            "mode": "exact"
        }]
    }));
    project
}

#[test]
fn template_string_literals_match_equal_source_strings() {
    for (template, source) in [
        (
            "export const {{ Name }} = \"x\";\n",
            "export const b = \"x\";\n",
        ),
        (
            "export const {{ Name }} = 'x';\n",
            "export const b = \"x\";\n",
        ),
        (
            "export const {{ Name }} = \"x y\";\n",
            "export const b = 'x y';\n",
        ),
        (
            "export const {{ Name }} = \"a\\nb\";\n",
            "export const b = \"a\\nb\";\n",
        ),
        (
            "import { {{ Name }} } from \"./a\";\n",
            "import { b } from \"./a\";\n",
        ),
        (
            "export const {{ Name }} = \"\";\n",
            "export const b = '';\n",
        ),
    ] {
        let project = string_project("rule.ts.ctmpl", template, "a.ts", source);
        let (code, report) = project.run(&[]);
        assert_eq!(
            (code, summary(&report)),
            (0, Vec::new()),
            "template {template:?} source {source:?}"
        );
    }
}

#[test]
fn template_string_literals_reject_different_source_strings() {
    for (template, source) in [
        (
            "export const {{ Name }} = \"x\";\n",
            "export const b = \"y\";\n",
        ),
        (
            "export const {{ Name }} = \"x\";\n",
            "export const b = \"xx\";\n",
        ),
        (
            "export const {{ Name }} = \"x\";\n",
            "export const b = \"\";\n",
        ),
        (
            "export const {{ Name }} = \"\";\n",
            "export const b = \"x\";\n",
        ),
    ] {
        let project = string_project("rule.ts.ctmpl", template, "a.ts", source);
        let (code, report) = project.run(&[]);
        assert_eq!(code, 1, "template {template:?} source {source:?}");
        assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn cpp_template_string_literals_match_equal_source_strings() {
    let matching = string_project(
        "rule.cpp.ctmpl",
        "const char* {{ Name }} = \"x\";\n",
        "a.cpp",
        "const char* b = \"x\";\n",
    );
    let (code, report) = matching.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));

    let different = string_project(
        "rule.cpp.ctmpl",
        "const char* {{ Name }} = \"x\";\n",
        "a.cpp",
        "const char* b = \"y\";\n",
    );
    let (code, _) = different.run(&[]);
    assert_eq!(code, 1);
}

#[test]
fn forbid_template_can_name_an_imported_module() {
    let project = Project::new();
    project.write(
        ".ctmpl/no-jquery.ts.ctmpl",
        "import {{ Bound }} from \"jquery\";\n",
    );
    project.write(
        "src/a.ts",
        "import $ from \"jquery\";\nimport { b } from \"lodash\";\n",
    );
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-jquery",
            "template": ".ctmpl/no-jquery.ts.ctmpl",
            "include": ["src/**/*.ts"],
            "mode": "forbid"
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3006", "no-jquery", "src/a.ts", 1)]
    );
}
