use serde_json::Value;

pub mod common;

use common::{Project, entry, summary};

// count

fn class_count_config(count: Value) -> Value {
    serde_json::json!({
        "schemaVersion": 1,
        "rules": [{
            "id": "one-class",
            "template": ".ctmpl/class.ts.ctmpl",
            "include": ["src/**/*.ts"],
            "mode": "contains",
            "count": count
        }]
    })
}

fn class_project(count: Value) -> Project {
    let project = Project::new();
    project.write(
        ".ctmpl/class.ts.ctmpl",
        "{{ Class | kind(\"ClassDeclaration\") }}\n",
    );
    project.config(&class_count_config(count));
    project
}

#[test]
fn count_accepts_exactly_one_match() {
    let project = class_project(serde_json::json!({ "min": 1, "max": 1 }));
    project.write("src/a.ts", "class A {}\nexport const a = 1;\n");
    let (code, report) = project.run(&[]);
    assert_eq!((code, summary(&report)), (0, Vec::new()));
}

#[test]
fn count_reports_each_match_over_the_maximum() {
    let project = class_project(serde_json::json!({ "min": 1, "max": 1 }));
    project.write("src/a.ts", "class A {}\nclass B {}\n\nclass C {}\n");
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![
            entry("CTC3009", "one-class", "src/a.ts", 2),
            entry("CTC3009", "one-class", "src/a.ts", 4),
        ]
    );
    assert_eq!(report["diagnostics"][0]["expected"], "at most 1");
    assert_eq!(report["diagnostics"][0]["actual"], "3");
}

#[test]
fn count_reports_too_few_matches() {
    let project = class_project(serde_json::json!({ "min": 2 }));
    project.write("src/a.ts", "class A {}\n");
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3008", "one-class", "src/a.ts", 1)]
    );
    assert_eq!(report["diagnostics"][0]["expected"], "at least 2");
    assert_eq!(report["diagnostics"][0]["actual"], "1");
}

#[test]
fn count_reports_a_file_without_matches() {
    let project = class_project(serde_json::json!({ "max": 1 }));
    project.write("src/a.ts", "export const a = 1;\n");
    let (code, report) = project.run(&[]);
    assert_eq!(code, 1);
    assert_eq!(
        summary(&report),
        vec![entry("CTC3008", "one-class", "src/a.ts", 1)]
    );
}

#[test]
fn count_needs_contains_mode_and_a_sane_range() {
    for (mode, count, message) in [
        (
            "forbid",
            serde_json::json!({ "max": 1 }),
            "without contains mode",
        ),
        ("contains", serde_json::json!({ "min": 0 }), "minimum of 0"),
        (
            "contains",
            serde_json::json!({ "min": 3, "max": 2 }),
            "maximum below its minimum",
        ),
    ] {
        let project = class_project(count.clone());
        project.write("src/a.ts", "class A {}\n");
        let mut config = class_count_config(count);
        config["rules"][0]["mode"] = Value::String(mode.to_string());
        project.config(&config);
        let (code, report) = project.run(&[]);
        assert_eq!(code, 2, "{mode} {message}");
        assert_eq!(report["diagnostics"][0]["code"], "CTC1010");
        assert!(
            report["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains(message),
            "{report}"
        );
    }
}
