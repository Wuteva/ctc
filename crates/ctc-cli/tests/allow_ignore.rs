use serde_json::Value;

pub mod common;

use common::{Project, TRY_WITH_IGNORE, entry, no_try_config, summary};

// allowIgnore

#[test]
fn rules_do_not_allow_ignore_comments_by_default() {
    let project = Project::new();
    project.no_try_template();
    project.write("src/a.ts", TRY_WITH_IGNORE);
    let mut config = no_try_config(false);
    config["rules"][0]
        .as_object_mut()
        .unwrap()
        .remove("allowIgnore");
    project.config(&config);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC5002", "no-try", "src/a.ts", 2),
            entry("CTC3006", "no-try", "src/a.ts", 3),
        ]
    );
    assert_eq!(
        report["diagnostics"][0]["message"],
        "Rule `no-try` does not allow suppression comments. Set `allowIgnore` to true on the rule to allow them."
    );
}

#[test]
fn allow_ignore_true_lets_a_comment_suppress_the_rule() {
    let project = Project::new();
    project.no_try_template();
    project.write("src/a.ts", TRY_WITH_IGNORE);
    project.config(&no_try_config(true));
    let (code, report) = project.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn allow_ignore_must_be_a_boolean() {
    let project = Project::new();
    project.no_try_template();
    project.write("src/a.ts", "export const a = 1;\n");
    let mut config = no_try_config(false);
    config["rules"][0]["allowIgnore"] = Value::String("yes".to_string());
    project.config(&config);
    let (code, report) = project.run(&[]);
    assert_eq!(code, 2);
    assert_eq!(report["diagnostics"][0]["code"], "CTC1010");
}

#[test]
fn allow_ignore_applies_to_semantic_rules() {
    let project = Project::new();
    project.write(
        "src/a.ts",
        "// ctc-ignore-next-line no-throw\nexport function f() {\n  throw new Error(\"x\");\n}\n",
    );
    project.config(&serde_json::json!({
        "schemaVersion": 1,
        "semanticRules": [{
            "kind": "exceptionPolicy",
            "id": "no-throw",
            "include": ["src/**/*.ts"],
            "forbidThrow": true
        }]
    }));
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC5002", "no-throw", "src/a.ts", 1),
            entry("CTC4102", "no-throw", "src/a.ts", 3),
        ]
    );
}
