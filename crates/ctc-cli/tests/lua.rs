use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::{Value, json};

pub mod common;

use common::{Project, entry, guard, summary};

// Lua

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/lua")
}

fn run_fixture(arguments: &[&str]) -> (i32, Value) {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(["--root", fixture().to_str().unwrap()])
        .args(arguments)
        .args(["--format", "json"])
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout).unwrap();
    (output.status.code().unwrap(), report)
}

#[test]
fn fixture_reports_each_lua_rule_and_the_parse_error() {
    let (code, report) = run_fixture(&[]);
    assert_eq!(code, 1);
    let mut actual = summary(&report);
    actual.sort();
    let mut expected = vec![
        entry("CTC3001", "", "packages/arena/broken.lua", 3),
        entry("CTC3002", "module-shape", "packages/arena/globals.lua", 1),
        entry(
            "CTC3006",
            "no-unrestricted-load",
            "packages/arena/loader.lua",
            6,
        ),
        entry(
            "CTC3006",
            "no-unrestricted-load",
            "packages/arena/loader.lua",
            11,
        ),
        entry(
            "CTC3006",
            "package-requires",
            "packages/arena/loader.lua",
            3,
        ),
        entry(
            "CTC3006",
            "short-functions",
            "packages/arena/loader.lua",
            14,
        ),
    ];
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn fixture_module_that_follows_the_rules_passes() {
    let (code, report) = run_fixture(&["packages/arena/parts.lua"]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn fixture_templates_validate() {
    for name in [
        "no-load",
        "module-shape",
        "long-function",
        "package-require",
    ] {
        let template = fixture().join(format!(".ctmpl/{name}.lua.ctmpl"));
        Command::cargo_bin("ctc")
            .unwrap()
            .args(["validate-template", template.to_str().unwrap()])
            .assert()
            .success();
    }
}

#[test]
fn invalid_lua_template_reports_template_syntax() {
    let project = Project::new();
    project.write(".ctmpl/bad.lua.ctmpl", "local x = = {{ Value }}\n");
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(["validate-template"])
        .arg(project.dir.path().join(".ctmpl/bad.lua.ctmpl"))
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "CTC2001");
}

fn load_project(allow_ignore: bool) -> Project {
    let project = Project::new();
    project.write(
        ".ctmpl/no-load.lua.ctmpl",
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"equal\", \"load\") }}\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "no-load",
            "template": ".ctmpl/no-load.lua.ctmpl",
            "include": ["scripts/**/*.lua"],
            "mode": "forbid",
            "scope": "descendants",
            "allowIgnore": allow_ignore
        }]
    }));
    project
}

const LOAD_WITH_IGNORE: &str = "local M = {}\n-- ctc-ignore-next-line no-load -- the sandbox loads trusted text\nM.chunk = load(\"return 1\")\n--[[ ctc-ignore-next-line no-load ]]\nM.other = load(\"return 2\")\nM.third = load(\"return 3\")\nreturn M\n";

#[test]
fn lua_comments_suppress_rules_that_allow_it() {
    let project = load_project(true);
    project.write("scripts/a.lua", LOAD_WITH_IGNORE);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3006", "no-load", "scripts/a.lua", 6)]
    );
}

#[test]
fn lua_comments_cannot_suppress_protected_rules() {
    let project = load_project(false);
    project.write("scripts/a.lua", LOAD_WITH_IGNORE);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    let codes = summary(&report)
        .into_iter()
        .map(|(code, _, _, line)| (code, line))
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        vec![
            ("CTC5002".to_string(), 2),
            ("CTC3006".to_string(), 3),
            ("CTC5002".to_string(), 4),
            ("CTC3006".to_string(), 5),
            ("CTC3006".to_string(), 6),
        ]
    );
}

#[test]
fn guard_reports_an_added_lua_suppression_comment() {
    let project = load_project(true);
    project.init_git();
    project.write("scripts/a.lua", "local M = {}\nreturn M\n");
    project.commit_all("base");
    project.write("scripts/a.lua", LOAD_WITH_IGNORE);
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC5201", "no-load", "scripts/a.lua", 2),
            entry("CTC5201", "no-load", "scripts/a.lua", 4),
        ]
    );
}

#[test]
fn coverage_reports_uncovered_lua_files() {
    let project = Project::new();
    project.no_try_template();
    project.write(
        ".ctmpl/no-load.lua.ctmpl",
        "{{ Call | kind(\"CallExpression\") | field(\"callee\", \"equal\", \"load\") }}\n",
    );
    project.write("scripts/init.lua", "return {}\n");
    project.write("scripts/data.lua", "return {}\n");
    project.write("src/a.ts", "export const a = 1;\n");
    project.write("src/b.lua", "return {}\n");
    project.write("tools/c.lua", "return {}\n");
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [
            {
                "id": "no-load",
                "template": ".ctmpl/no-load.lua.ctmpl",
                "include": ["scripts/**/init.lua"],
                "mode": "forbid",
                "scope": "descendants"
            },
            {
                "id": "no-try",
                "template": ".ctmpl/no-try.ts.ctmpl",
                "include": ["src/**"],
                "mode": "forbid",
                "scope": "descendants"
            }
        ]
    }));
    let (code, report) = project.run_subcommand("coverage", &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC5101", "", "scripts/data.lua", 1),
            entry("CTC5101", "", "src/b.lua", 1),
        ]
    );
}

#[test]
fn file_length_rule_selects_lua_files() {
    let project = Project::new();
    project.write(
        "scripts/a.lua",
        "local a = 1\nlocal b = 2\nlocal c = 3\nreturn a\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "semanticRules": [{
            "kind": "fileLength",
            "id": "short-files",
            "include": ["scripts/**/*.lua"],
            "maxLines": 3
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC4301", "short-files", "scripts/a.lua", 4)]
    );
}

#[test]
fn lua_rule_that_selects_another_language_reports_a_mismatch() {
    let project = load_project(false);
    project.write("scripts/a.lua", "return load(\"x\")\n");
    project.write("scripts/b.ts", "export const load = 1;\n");
    let mut config = serde_json::from_str::<Value>(
        &std::fs::read_to_string(project.dir.path().join(".ctc.json")).unwrap(),
    )
    .unwrap();
    config["rules"][0]["include"] = json!(["scripts/**"]);
    project.config(&config);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 2);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC3006", "no-load", "scripts/a.lua", 1),
            entry("CTC1008", "", "scripts/b.ts", 1),
        ]
    );
}
