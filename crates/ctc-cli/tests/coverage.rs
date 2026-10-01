use std::fs;

use assert_cmd::Command;
use serde_json::Value;

pub mod common;

use common::{Project, entry, summary};

// coverage

fn coverage_project() -> Project {
    let project = Project::new();
    project.no_try_template();
    project.write("src/domain/a.ts", "export const a = 1;\n");
    project.write("src/domain/nested/b.ts", "export const b = 1;\n");
    project.write("src/domain/native.cpp", "int main() { return 0; }\n");
    project.write("src/gen/generated.ts", "export const g = 1;\n");
    project.write("lib/outside.ts", "export const o = 1;\n");
    project.write("node_modules/pkg/index.ts", "export const n = 1;\n");
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-try",
            "template": ".ctmpl/no-try.ts.ctmpl",
            "include": ["src/domain/**/*.ts", "src/gen/**/*.ts"],
            "exclude": ["src/gen/**"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    project
}

#[test]
fn coverage_reports_files_in_the_watched_area_that_no_rule_selects() {
    let project = coverage_project();
    let (code, report) = project.run_subcommand("coverage", &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5101", "", "src/domain/native.cpp", 1)]
    );
    assert_eq!(
        report["diagnostics"][0]["message"],
        "The file is not selected by any rule."
    );
}

#[test]
fn coverage_passes_when_every_watched_file_is_selected() {
    let project = coverage_project();
    fs::remove_file(project.dir.path().join("src/domain/native.cpp")).unwrap();
    let (code, report) = project.run_subcommand("coverage", &[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn coverage_counts_a_rule_exclude_as_covered() {
    let project = coverage_project();
    fs::remove_file(project.dir.path().join("src/domain/native.cpp")).unwrap();
    let (_, report) = project.run_subcommand("coverage", &[]);
    assert!(
        summary(&report)
            .iter()
            .all(|(_, _, path, _)| path != "src/gen/generated.ts")
    );
}

#[test]
fn coverage_watches_the_whole_root_for_a_root_wide_include() {
    let project = coverage_project();
    let mut config: Value =
        serde_json::from_str(&fs::read_to_string(project.dir.path().join(".ctc.json")).unwrap())
            .unwrap();
    config["rules"][0]["include"] = serde_json::json!(["**/*.ts"]);
    config["rules"][0]["exclude"] = serde_json::json!([]);
    project.config(&config);
    project.write("lib/other.cpp", "int x = 1;\n");
    let (code, report) = project.run_subcommand("coverage", &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC5101", "", "lib/other.cpp", 1),
            entry("CTC5101", "", "src/domain/native.cpp", 1),
        ]
    );
}

#[test]
fn coverage_human_output_lists_one_line_per_file() {
    let project = coverage_project();
    Command::cargo_bin("ctc")
        .unwrap()
        .args([
            "coverage",
            "--root",
            project.dir.path().to_str().unwrap(),
            "--color",
            "never",
        ])
        .assert()
        .code(1)
        .stdout(
            "src/domain/native.cpp:1:1 The file is not selected by any rule.\n  Code: CTC5101\n",
        );
}
