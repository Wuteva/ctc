use std::fs;

use assert_cmd::Command;
use serde_json::Value;

pub mod common;

use common::{Project, entry, summary};

// regex filters

fn interface_name_project(mode: &str, template: &str) -> Project {
    let project = Project::new();
    project.write(".ctmpl/interface.ts.ctmpl", template);
    project.write(
        "src/a.ts",
        "export interface ILogger {}\nexport interface Logger {\n  a(): void;\n}\ninterface Thing {}\n",
    );
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "interface-name",
            "template": ".ctmpl/interface.ts.ctmpl",
            "include": ["src/**/*.ts"],
            "mode": mode,
            "scope": "descendants"
        }]
    }));
    project
}

#[test]
fn not_matches_reports_names_that_match_the_pattern() {
    let project = interface_name_project(
        "forbid",
        "interface {{ Name | notMatches(\"^I[A-Z]\") }} {\n  {{* Members }}\n}\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC3006", "interface-name", "src/a.ts", 2),
            entry("CTC3006", "interface-name", "src/a.ts", 5),
        ]
    );
}

#[test]
fn matches_requires_the_pattern_and_reports_the_failed_capture() {
    let project = interface_name_project(
        "contains",
        "interface {{ Name | matches(\"^X\") }} {\n  {{* Members }}\n}\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["code"], "CTC3010");
    assert_eq!(diagnostic["category"], "identifier-pattern-mismatch");
    assert_eq!(
        diagnostic["message"],
        "Capture `Name` must match the pattern `^X`."
    );
    assert_eq!(diagnostic["expected"], "match ^X");

    project.write("src/b.ts", "interface Xylophone {}\ninterface ILogger {}\n");
    let mut config: Value =
        serde_json::from_str(&fs::read_to_string(project.dir.path().join(".ctc.json")).unwrap())
            .unwrap();
    config["rules"][0]["include"] = serde_json::json!(["src/b.ts"]);
    project.config(&config);
    let (code, report) = project.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn regex_filters_run_after_name_filters() {
    let project = interface_name_project(
        "forbid",
        "interface {{ Name | removePrefix(\"I\") | notMatches(\"^[A-Z]\") }} {\n  {{* Members }}\n}\n",
    );
    project.write("src/a.ts", "interface ILogger {}\ninterface Ilogger {}\n");
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3006", "interface-name", "src/a.ts", 2)]
    );
}

#[test]
fn invalid_regex_filters_are_template_errors() {
    let project = Project::new();
    project.write(
        "rule.ts.ctmpl",
        "interface {{ Name | matches(\"(\") }} {}\n",
    );
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
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "CTC2008");
}
