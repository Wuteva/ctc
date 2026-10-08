use std::path::Path;

use crate::{
    canonical::{GlobalAccessKind, GlobalCall, GlobalFact, SemanticFacts},
    diagnostic::TextRange,
    semantic::{SemanticRule, evaluate_semantic_rule},
};

use super::*;

struct Spec {
    kind: GlobalAccessKind,
    name: &'static str,
    path: &'static str,
    dynamic_key: bool,
    via_table: bool,
    call: GlobalCall,
}

const fn read(path: &'static str) -> Spec {
    Spec {
        kind: GlobalAccessKind::Read,
        name: "",
        path,
        dynamic_key: false,
        via_table: false,
        call: GlobalCall::NotCalled,
    }
}

fn facts(specs: Vec<Spec>) -> SemanticFacts {
    let globals = specs
        .into_iter()
        .enumerate()
        .map(|(index, spec)| {
            let mut range = TextRange::from_offsets(Path::new("a.lua"), "", 0, 0);
            range.start.line = index as u32 + 1;
            let name = if spec.name.is_empty() {
                spec.path.split('.').next().unwrap_or_default()
            } else {
                spec.name
            };
            GlobalFact {
                kind: spec.kind,
                name: name.to_string(),
                path: spec.path.to_string(),
                dynamic_key: spec.dynamic_key,
                via_table: spec.via_table,
                call: spec.call,
                range,
            }
        })
        .collect();
    SemanticFacts {
        globals,
        ..SemanticFacts::default()
    }
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<(&str, u32)> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.as_str(),
                diagnostic
                    .source
                    .as_ref()
                    .map_or(0, |range| range.start.line),
            )
        })
        .collect()
}

fn accidental(allow: &[&str], allow_write: &[&str], facts: &SemanticFacts) -> Vec<Diagnostic> {
    let rule = SemanticRule::AccidentalGlobals {
        allow: allow.iter().map(ToString::to_string).collect(),
        allow_write: allow_write.iter().map(ToString::to_string).collect(),
    };
    evaluate_semantic_rule("globals", &rule, facts)
}

fn restricted(forbid: &[&str], dynamic_require: bool, facts: &SemanticFacts) -> Vec<Diagnostic> {
    let rule = SemanticRule::RestrictedGlobals {
        forbid: forbid.iter().map(ToString::to_string).collect(),
        forbid_dynamic_require: dynamic_require,
    };
    evaluate_semantic_rule("loading", &rule, facts)
}

#[test]
fn accidental_globals_allow_listed_names_only() {
    let found = facts(vec![read("print"), read("count"), read("math.floor")]);
    let diagnostics = accidental(&["print", "math"], &[], &found);
    assert_eq!(codes(&diagnostics), vec![("CTC4401", 2)]);
    assert!(diagnostics[0].message.contains("`count`"));
}

#[test]
fn accidental_globals_report_writes_unless_the_name_may_be_written() {
    let write = |path| Spec {
        kind: GlobalAccessKind::Write,
        ..read(path)
    };
    let found = facts(vec![write("score"), write("debugMode")]);
    let diagnostics = accidental(&["score"], &["debugMode"], &found);
    assert_eq!(codes(&diagnostics), vec![("CTC4402", 1)]);
    assert_eq!(
        diagnostics[0].category,
        DiagnosticCategory::GlobalAssignment
    );
}

#[test]
fn accidental_globals_report_a_dynamic_write_but_not_a_dynamic_read() {
    let dynamic = |kind| Spec {
        kind,
        name: "",
        path: "",
        dynamic_key: true,
        via_table: true,
        call: GlobalCall::NotCalled,
    };
    let found = facts(vec![
        dynamic(GlobalAccessKind::DynamicRead),
        dynamic(GlobalAccessKind::DynamicWrite),
    ]);
    assert_eq!(codes(&accidental(&[], &[], &found)), vec![("CTC4402", 2)]);
}

#[test]
fn messages_show_a_name_that_the_global_table_reached() {
    let found = facts(vec![Spec {
        via_table: true,
        ..read("cache")
    }]);
    let diagnostics = accidental(&[], &[], &found);
    assert!(diagnostics[0].message.contains("`_G.cache`"));
}

#[test]
fn restricted_names_forbid_everything_below_them() {
    let found = facts(vec![
        read("os.time"),
        read("os"),
        read("osx"),
        read("string.dump"),
        read("string.format"),
        read("debug.getinfo"),
    ]);
    let diagnostics = restricted(&["os", "string.dump", "debug"], true, &found);
    assert_eq!(
        codes(&diagnostics),
        vec![
            ("CTC4403", 1),
            ("CTC4403", 2),
            ("CTC4403", 4),
            ("CTC4403", 6)
        ]
    );
}

#[test]
fn a_namespace_value_can_reach_a_restricted_name_below_it() {
    let called = Spec {
        call: GlobalCall::Other,
        ..read("string")
    };
    let dynamic = Spec {
        dynamic_key: true,
        ..read("string")
    };
    let found = facts(vec![read("string"), called, dynamic, read("string.format")]);
    let diagnostics = restricted(&["string.dump"], true, &found);
    assert_eq!(codes(&diagnostics), vec![("CTC4403", 1), ("CTC4403", 3)]);
    assert!(diagnostics[0].message.contains("can reach `string.dump`"));
}

#[test]
fn require_needs_one_string_literal() {
    let call = |call| Spec {
        call,
        ..read("require")
    };
    let found = facts(vec![
        call(GlobalCall::StringLiteral),
        call(GlobalCall::Other),
        call(GlobalCall::NotCalled),
    ]);
    let diagnostics = restricted(&[], true, &found);
    assert_eq!(codes(&diagnostics), vec![("CTC4404", 2), ("CTC4404", 3)]);
    assert!(restricted(&[], false, &found).is_empty());
}

#[test]
fn a_forbidden_require_is_reported_once() {
    let found = facts(vec![Spec {
        call: GlobalCall::Other,
        ..read("require")
    }]);
    let diagnostics = restricted(&["require"], true, &found);
    assert_eq!(codes(&diagnostics), vec![("CTC4403", 1)]);
}

#[test]
fn dynamic_table_keys_are_always_reported_by_the_restricted_rule() {
    let found = facts(vec![Spec {
        kind: GlobalAccessKind::DynamicRead,
        name: "",
        path: "",
        dynamic_key: true,
        via_table: true,
        call: GlobalCall::NotCalled,
    }]);
    let diagnostics = restricted(&[], false, &found);
    assert_eq!(codes(&diagnostics), vec![("CTC4405", 1)]);
}

#[test]
fn default_list_covers_the_unrestricted_loading_names() {
    for name in [
        "load",
        "loadfile",
        "loadstring",
        "dofile",
        "setfenv",
        "collectgarbage",
        "string.dump",
        "debug",
        "io",
        "os",
        "package",
    ] {
        assert!(DEFAULT_RESTRICTED_GLOBALS.contains(&name), "{name}");
    }
}
