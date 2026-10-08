use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::{Value, json};

pub mod common;

use common::{Project, entry, guard, summary};

// Lua project rules: module structure, function size, accidental globals,
// and unrestricted loading.

type Found = Vec<(String, String, String, u64)>;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/lua-rules")
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

fn found(arguments: &[&str]) -> Found {
    let (_, report) = run_fixture(arguments);
    let mut found = summary(&report);
    found.sort();
    found
}

fn sorted(mut expected: Found) -> Found {
    expected.sort();
    expected
}

fn entries(code: &str, rule: &str, path: &str, lines: &[u64]) -> Found {
    lines
        .iter()
        .map(|line| entry(code, rule, path, *line))
        .collect()
}

fn effects() -> Found {
    entries(
        "CTC3006",
        "module-no-top-level-effects",
        "packages/arena/effects.lua",
        &[5, 9, 13, 15, 19],
    )
}

fn returns() -> Found {
    vec![
        entry(
            "CTC3002",
            "module-returns-table",
            "packages/arena/no_return.lua",
            4,
        ),
        entry(
            "CTC3003",
            "module-returns-table",
            "packages/arena/return_two.lua",
            5,
        ),
    ]
}

fn requires() -> Found {
    let mut expected = entries(
        "CTC3006",
        "package-requires",
        "packages/arena/bad_require.lua",
        &[4, 6],
    );
    expected.extend(entries(
        "CTC3006",
        "package-requires",
        "packages/arena/loading.lua",
        &[16],
    ));
    expected
}

fn sizes() -> Found {
    vec![
        entry(
            "CTC3006",
            "short-lua-functions",
            "packages/arena/long_function.lua",
            4,
        ),
        entry(
            "CTC4301",
            "short-lua-files",
            "packages/arena/long_file.lua",
            121,
        ),
    ]
}

fn globals() -> Found {
    let rule = "no-accidental-globals";
    let mut expected = entries(
        "CTC4401",
        rule,
        "packages/arena/accidents.lua",
        &[6, 10, 11, 17],
    );
    expected.extend(entries(
        "CTC4402",
        rule,
        "packages/arena/accidents.lua",
        &[10, 15, 16, 17],
    ));
    expected.extend(entries("CTC4401", rule, "packages/arena/effects.lua", &[5]));
    expected.extend(entries(
        "CTC4402",
        rule,
        "packages/arena/effects.lua",
        &[13, 15],
    ));
    expected.extend(entries(
        "CTC4401",
        rule,
        "packages/arena/loading.lua",
        &[6, 7, 8, 9, 10, 11, 13, 14, 15],
    ));
    expected
}

fn test_globals() -> Found {
    let rule = "no-accidental-globals-in-tests";
    let mut expected = entries("CTC4401", rule, "tests/lua/leaky_test.lua", &[5, 6]);
    expected.extend(entries("CTC4402", rule, "tests/lua/leaky_test.lua", &[5]));
    expected
}

fn loading() -> Found {
    let rule = "no-unrestricted-loading";
    let mut expected = entries(
        "CTC4403",
        rule,
        "packages/arena/loading.lua",
        &[6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 18],
    );
    expected.extend(entries(
        "CTC4404",
        rule,
        "packages/arena/loading.lua",
        &[16],
    ));
    expected.extend(entries(
        "CTC4405",
        rule,
        "packages/arena/loading.lua",
        &[17],
    ));
    expected.extend(entries(
        "CTC4404",
        rule,
        "packages/arena/bad_require.lua",
        &[6],
    ));
    expected.extend(entries(
        "CTC4405",
        rule,
        "packages/arena/accidents.lua",
        &[17],
    ));
    expected.extend(entries("CTC4403", rule, "tests/lua/leaky_test.lua", &[5]));
    expected
}

#[test]
fn fixture_reports_each_rule() {
    let (code, _) = run_fixture(&[]);
    assert_eq!(code, 1);
    let mut expected = Vec::new();
    for part in [
        effects(),
        returns(),
        requires(),
        sizes(),
        globals(),
        test_globals(),
        loading(),
    ] {
        expected.extend(part);
    }
    assert_eq!(found(&[]), sorted(expected));
}

#[test]
fn module_structure_rules_find_top_level_work_and_missing_returns() {
    assert_eq!(
        found(&["--rule", "module-no-top-level-effects"]),
        sorted(effects())
    );
    assert_eq!(
        found(&["--rule", "module-returns-table"]),
        sorted(returns())
    );
}

#[test]
fn require_rule_finds_names_that_are_not_package_module_names() {
    assert_eq!(found(&["--rule", "package-requires"]), sorted(requires()));
}

#[test]
fn size_rules_find_a_long_function_and_a_long_file() {
    assert_eq!(
        found(&["--rule", "short-lua-functions"]),
        sorted(vec![sizes()[0].clone()])
    );
    assert_eq!(
        found(&["--rule", "short-lua-files"]),
        sorted(vec![sizes()[1].clone()])
    );
}

#[test]
fn accidental_globals_rule_finds_reads_and_assignments() {
    assert_eq!(
        found(&["--rule", "no-accidental-globals"]),
        sorted(globals())
    );
    assert_eq!(
        found(&["--rule", "no-accidental-globals-in-tests"]),
        sorted(test_globals())
    );
}

#[test]
fn restricted_globals_rule_finds_unrestricted_loading() {
    assert_eq!(
        found(&["--rule", "no-unrestricted-loading"]),
        sorted(loading())
    );
}

#[test]
fn fixture_modules_that_follow_the_rules_pass() {
    for path in [
        "packages/arena/parts.lua",
        "packages/arena/rules.lua",
        "packages/arena/strict.lua",
        "packages/arena/limit.lua",
        "tests/lua/parts_test.lua",
    ] {
        let (code, report) = run_fixture(&[path]);
        assert_eq!((code, summary(&report)), (0, Vec::new()), "{path}");
    }
}

#[test]
fn fixture_templates_validate() {
    for name in [
        "module-top-level-statement",
        "module-global-assignment",
        "module-global-function",
        "module-return-table",
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
fn every_lua_file_of_the_fixture_has_a_rule() {
    let output = Command::cargo_bin("ctc")
        .unwrap()
        .args(["coverage", "--root", fixture().to_str().unwrap()])
        .args(["--format", "json"])
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        (output.status.code(), summary(&report)),
        (Some(0), Vec::new())
    );
}

#[test]
fn global_diagnostics_carry_the_category_the_values_and_the_rule_message() {
    let (_, report) = run_fixture(&[
        "--rule",
        "no-unrestricted-loading",
        "packages/arena/loading.lua",
    ]);
    let diagnostics = report["diagnostics"].as_array().unwrap();
    let load = &diagnostics[0];
    assert_eq!(load["code"], "CTC4403");
    assert_eq!(load["category"], "restricted-global");
    assert_eq!(load["expected"], "no use of `load`");
    assert_eq!(load["actual"], "`load`");
    assert_eq!(load["source"]["start"]["column"], 17);
    assert!(
        load["ruleMessage"]
            .as_str()
            .unwrap()
            .starts_with("Scripts cannot load code")
    );
    let categories = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["category"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        categories.into_iter().collect::<Vec<_>>(),
        vec![
            "dynamic-global-access",
            "dynamic-require",
            "restricted-global"
        ]
    );
}

#[test]
fn accidental_global_categories_in_json() {
    let (_, report) = run_fixture(&[
        "--rule",
        "no-accidental-globals",
        "packages/arena/accidents.lua",
    ]);
    let categories = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["category"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        categories.into_iter().collect::<Vec<_>>(),
        vec!["global-assignment", "undeclared-global-read"]
    );
}

fn globals_project(allow_ignore: bool) -> Project {
    let project = Project::new();
    project.config(&json!({
        "schemaVersion": 1,
        "semanticRules": [
            {
                "kind": "accidentalGlobals",
                "id": "no-accidental-globals",
                "include": ["scripts/**/*.lua"],
                "allow": ["print", "os"],
                "allowIgnore": allow_ignore
            },
            {
                "kind": "restrictedGlobals",
                "id": "no-unrestricted-loading",
                "include": ["scripts/**/*.lua"],
                "forbid": ["os.execute"],
                "forbidDynamicRequire": false,
                "allowIgnore": allow_ignore
            }
        ]
    }));
    project
}

const IGNORED_GLOBALS: &str = "local M = {}\n-- ctc-ignore-next-line no-accidental-globals -- the runner sets it\nM.v = runnerValue\n--[[ ctc-ignore-next-line no-accidental-globals, no-unrestricted-loading ]]\nM.w = os.execute(\"x\") and other\nM.x = os.execute(\"y\")\nreturn M\n";

#[test]
fn global_rules_accept_suppression_comments_when_the_rule_allows_them() {
    let project = globals_project(true);
    project.write("scripts/a.lua", IGNORED_GLOBALS);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry(
            "CTC4403",
            "no-unrestricted-loading",
            "scripts/a.lua",
            6
        )]
    );
}

#[test]
fn global_rules_refuse_suppression_comments_by_default() {
    let project = globals_project(false);
    project.write("scripts/a.lua", IGNORED_GLOBALS);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    let found = summary(&report)
        .into_iter()
        .map(|(code, rule, _, line)| (code, rule, line))
        .collect::<Vec<_>>();
    let rule = |code: &str, rule: &str, line: u64| (code.to_string(), rule.to_string(), line);
    assert_eq!(
        found,
        vec![
            rule("CTC5002", "no-accidental-globals", 2),
            rule("CTC4401", "no-accidental-globals", 3),
            rule("CTC5002", "no-accidental-globals", 4),
            rule("CTC4401", "no-accidental-globals", 5),
            rule("CTC5002", "no-unrestricted-loading", 4),
            rule("CTC4403", "no-unrestricted-loading", 5),
            rule("CTC4403", "no-unrestricted-loading", 6),
        ]
    );
}

#[test]
fn guard_reports_a_suppression_comment_for_a_global_rule() {
    let project = globals_project(true);
    project.init_git();
    project.write("scripts/a.lua", "local M = {}\nreturn M\n");
    project.commit_all("base");
    project.write("scripts/a.lua", IGNORED_GLOBALS);
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC5201", "no-accidental-globals", "scripts/a.lua", 2),
            entry("CTC5201", "no-accidental-globals", "scripts/a.lua", 4),
            entry("CTC5201", "no-unrestricted-loading", "scripts/a.lua", 4),
        ]
    );
}

#[test]
fn guard_reports_weaker_global_rules() {
    let project = globals_project(false);
    project.init_git();
    project.write("scripts/a.lua", "local M = {}\nreturn M\n");
    project.write("scripts/b.lua", "local M = {}\nreturn M\n");
    project.commit_all("base");
    project.config(&json!({
        "schemaVersion": 1,
        "semanticRules": [{
            "kind": "accidentalGlobals",
            "id": "no-accidental-globals",
            "include": ["scripts/a.lua"],
            "allow": ["print", "os", "io"]
        }]
    }));
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    let found = summary(&report)
        .into_iter()
        .map(|(code, rule, _, _)| (code, rule))
        .collect::<Vec<_>>();
    let rule = |code: &str, rule: &str| (code.to_string(), rule.to_string());
    assert_eq!(
        found,
        vec![
            rule("CTC5202", "no-accidental-globals"),
            rule("CTC5204", "no-accidental-globals"),
            rule("CTC5203", "no-unrestricted-loading"),
        ]
    );
}

#[test]
fn restricted_rule_without_a_list_and_without_dynamic_require_checks_only_the_table_key() {
    let project = Project::new();
    project.config(&json!({
        "schemaVersion": 1,
        "semanticRules": [{
            "kind": "restrictedGlobals",
            "id": "table-keys",
            "include": ["scripts/**/*.lua"],
            "forbid": [],
            "forbidDynamicRequire": false
        }]
    }));
    project.write(
        "scripts/a.lua",
        "local a = os.time()\nlocal b = require(name)\nlocal c = _G[name]\nreturn a, b, c\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC4405", "table-keys", "scripts/a.lua", 3)]
    );
}

#[test]
fn global_rules_skip_files_of_other_languages_and_report_a_syntax_error_once() {
    let project = globals_project(false);
    project.write("scripts/a.lua", "local M = {\nreturn M\n");
    project.write("scripts/b.ts", "export const os = 1;\n");
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3001", "", "scripts/a.lua", 1)]
    );
}

#[test]
fn global_rule_configuration_errors_exit_with_2() {
    for (rule, expected) in [
        (
            json!({"kind": "accidentalGlobals", "id": "g", "include": ["a/**"], "allow": ["not-a-name"]}),
            "invalid Lua name `not-a-name` in `allow`",
        ),
        (
            json!({"kind": "restrictedGlobals", "id": "g", "include": ["a/**"], "forbid": ["os."]}),
            "invalid Lua name path `os.` in `forbid`",
        ),
        (
            json!({"kind": "accidentalGlobals", "id": "g", "include": ["a/**"], "forbid": ["os"]}),
            "unknown field `forbid`",
        ),
    ] {
        let project = Project::new();
        project.write("a/x.lua", "return {}\n");
        project.config(&json!({"schemaVersion": 1, "semanticRules": [rule]}));
        let (code, report) = project.run(&[]);
        assert_eq!(code, 2);
        assert_eq!(report["diagnostics"][0]["code"], "CTC1010");
        assert!(
            report["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains(expected),
            "{expected}"
        );
    }
}

#[test]
fn hook_template_requires_a_function_with_two_parameters() {
    let project = Project::new();
    project.write(
        ".ctmpl/pre-step.lua.ctmpl",
        "function {{ Module }}.pre_step({{ Ctx }}, {{ Part }})\n{{* Body }}\nend\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "part-has-pre-step",
            "template": ".ctmpl/pre-step.lua.ctmpl",
            "include": ["packages/*/scripts/server/parts/*.lua"],
            "mode": "contains",
            "scope": "topLevel"
        }]
    }));
    project.write(
        "packages/core/scripts/server/parts/wheel.lua",
        "local wheel = {}\nfunction wheel.pre_step(ctx, part)\n  ctx:motor(part, {})\nend\nreturn wheel\n",
    );
    project.write(
        "packages/core/scripts/server/parts/bad.lua",
        "local bad = {}\nfunction bad.pre_step(ctx)\nend\nreturn bad\n",
    );
    project.write(
        "packages/core/scripts/server/parts/none.lua",
        "local none = {}\nreturn none\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry(
                "CTC3002",
                "part-has-pre-step",
                "packages/core/scripts/server/parts/bad.lua",
                2
            ),
            entry(
                "CTC3002",
                "part-has-pre-step",
                "packages/core/scripts/server/parts/none.lua",
                2
            ),
        ]
    );
    assert_eq!(report["diagnostics"][0]["expected"], "capture `Part`");
}

#[test]
fn guard_reports_a_changed_lua_template() {
    let project = Project::new();
    project.write(
        ".ctmpl/long-function.lua.ctmpl",
        "{{ Long | kind(\"FunctionDeclaration\") | field(\"lineCount\", \"greaterThan\", 100) }}\n",
    );
    project.config(&json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "short-lua-functions",
            "template": ".ctmpl/long-function.lua.ctmpl",
            "include": ["packages/**/*.lua"],
            "mode": "forbid",
            "scope": "descendants"
        }]
    }));
    project.write("packages/a.lua", "local a = {}\nreturn a\n");
    project.init_git();
    project.commit_all("base");
    project.write(
        ".ctmpl/long-function.lua.ctmpl",
        "{{ Long | kind(\"FunctionDeclaration\") | field(\"lineCount\", \"greaterThan\", 500) }}\n",
    );
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5204", "short-lua-functions", ".ctc.json", 1)]
    );
    let (code, _) = guard(&project, &["--allow-rule-changes"]);
    assert_eq!(code, 0);
}

#[test]
fn lua_guide_shows_the_template_files_of_the_fixture() {
    let guide = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/languages/lua.md"),
    )
    .unwrap();
    let templates = std::fs::read_dir(fixture().join(".ctmpl")).unwrap();
    for template in templates {
        let path = template.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            guide.contains(text.trim_end()),
            "docs/languages/lua.md does not show {}",
            path.display()
        );
    }
}

#[test]
fn coverage_counts_a_lua_global_rule_for_lua_files_only() {
    let project = Project::new();
    project.config(&json!({
        "schemaVersion": 1,
        "semanticRules": [{
            "kind": "accidentalGlobals",
            "id": "no-accidental-globals",
            "include": ["scripts/**"]
        }]
    }));
    project.write("scripts/a.lua", "return {}\n");
    project.write("scripts/b.ts", "export const a = 1;\n");
    let (code, report) = project.run_subcommand("coverage", &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5101", "", "scripts/b.ts", 1)]
    );
}

#[test]
fn file_suppression_comment_skips_a_global_rule_for_the_whole_file() {
    let project = globals_project(true);
    project.write(
        "scripts/a.lua",
        "-- ctc-ignore-file no-accidental-globals -- the runner gives these names\nlocal M = {}\nM.a = first\nM.b = second\nreturn M\n",
    );
    let (code, report) = project.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}
