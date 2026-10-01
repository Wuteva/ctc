use serde_json::Value;

pub mod common;

use common::{Project, TRY_WITH_IGNORE, entry, git_in, guard, no_try_config, summary};

// guard

fn guard_project() -> Project {
    let project = Project::new();
    project.init_git();
    project.no_try_template();
    project.write("src/a.ts", "export const a = 1;\n");
    project.write("src/b.ts", "export const b = 1;\n");
    project.config(&no_try_config(false));
    project.commit_all("base");
    project
}

#[test]
fn guard_passes_when_nothing_weakens() {
    let project = guard_project();
    project.write("src/c.ts", "export const c = 1;\n");
    let (code, report) = guard(&project, &[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn guard_reports_added_suppression_comments() {
    let project = guard_project();
    project.config(&no_try_config(true));
    project.commit_all("allow ignore");
    project.write("src/a.ts", TRY_WITH_IGNORE);
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5201", "no-try", "src/a.ts", 2)]
    );
}

#[test]
fn guard_ignores_suppression_comments_that_already_existed() {
    let project = guard_project();
    project.config(&no_try_config(true));
    project.write("src/a.ts", TRY_WITH_IGNORE);
    project.commit_all("existing ignore");
    project.write("src/a.ts", &format!("// moved\n{TRY_WITH_IGNORE}"));
    let (code, report) = guard(&project, &[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn guard_reports_a_rule_that_selects_fewer_files() {
    let project = guard_project();
    let mut config = no_try_config(false);
    config["rules"][0]["exclude"] = serde_json::json!(["src/b.ts"]);
    project.config(&config);
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5202", "no-try", ".ctc.json", 1)]
    );
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("no longer selects 1 file(s)")
    );
}

#[test]
fn guard_reports_a_narrowed_include_but_not_a_rewritten_one() {
    let project = guard_project();
    let mut config = no_try_config(false);
    config["rules"][0]["include"] = serde_json::json!(["src/a.ts"]);
    project.config(&config);
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(summary(&report)[0].0, "CTC5202");

    config["rules"][0]["include"] = serde_json::json!(["src/*.ts", "src/**/*.ts"]);
    project.config(&config);
    let (code, report) = guard(&project, &[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn guard_reports_removed_rules_and_enabled_ignores() {
    let project = guard_project();
    project.config(&no_try_config(true));
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5203", "no-try", ".ctc.json", 1)]
    );

    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "other",
            "template": ".ctmpl/no-try.ts.ctmpl",
            "include": ["src/**/*.ts"],
            "mode": "forbid"
        }]
    }));
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5203", "no-try", ".ctc.json", 1)]
    );
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("was removed")
    );
}

#[test]
fn guard_reports_rule_definition_changes_unless_allowed() {
    let project = guard_project();
    project.write(
        ".ctmpl/no-try.ts.ctmpl",
        "{{ Forbidden | kind(\"ThrowStatement\") }}\n",
    );
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5204", "no-try", ".ctc.json", 1)]
    );
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("content of template")
    );

    let mut config = no_try_config(false);
    config["rules"][0]["mode"] = Value::String("contains".to_string());
    project.config(&config);
    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("`mode`")
    );

    let (code, report) = guard(&project, &["--allow-rule-changes"]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn guard_ignores_line_ending_differences_in_templates() {
    let project = guard_project();
    project.write(
        ".ctmpl/no-try.ts.ctmpl",
        "{{ Forbidden | kind(\"TryStatement\") }}\r\n",
    );
    let (code, report) = guard(&project, &[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn guard_reports_a_shared_template_submodule_that_moved() {
    let templates = Project::new();
    templates.init_git();
    templates.write(
        "no-try.ts.ctmpl",
        "{{ Forbidden | kind(\"TryStatement\") }}\n",
    );
    templates.commit_all("templates");

    let project = Project::new();
    project.init_git();
    project.write("src/a.ts", "export const a = 1;\n");
    project.config(&no_try_config(false));
    project.git(&[
        "submodule",
        "add",
        "--quiet",
        templates.dir.path().to_str().unwrap(),
        ".ctmpl",
    ]);
    project.commit_all("base");

    let (code, report) = guard(&project, &[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));

    templates.write(
        "no-try.ts.ctmpl",
        "{{ Forbidden | kind(\"ThrowStatement\") }}\n",
    );
    templates.commit_all("change template");
    git_in(&project.dir.path().join(".ctmpl"), &["pull", "--quiet"]);

    let (code, report) = guard(&project, &[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC5204", "no-try", ".ctc.json", 1)]
    );
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("shared template directory `.ctmpl`")
    );
    let (code, _) = guard(&project, &["--allow-rule-changes"]);
    assert_eq!(code, 0);
}

#[test]
fn guard_rejects_an_unknown_reference() {
    let project = guard_project();
    let (code, report) = project.run_subcommand("guard", &["--base", "no-such-ref"]);
    assert_eq!(code, 2);
    assert_eq!(report["diagnostics"][0]["code"], "CTC0001");
    let (code, _) = project.run_subcommand("guard", &["--base=--output=x"]);
    assert_eq!(code, 2);
}

#[test]
fn guard_needs_a_base_reference() {
    let project = guard_project();
    let (code, report) = project.run_subcommand("guard", &[]);
    assert_eq!(code, 2);
    assert_eq!(report["diagnostics"][0]["code"], "CTC0001");
}

#[test]
fn guard_treats_a_new_configuration_as_having_no_rules_to_weaken() {
    let project = Project::new();
    project.init_git();
    project.write("README.md", "hello\n");
    project.commit_all("base");
    project.no_try_template();
    project.write("src/a.ts", "export const a = 1;\n");
    project.config(&no_try_config(false));
    let (code, report) = guard(&project, &[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}
