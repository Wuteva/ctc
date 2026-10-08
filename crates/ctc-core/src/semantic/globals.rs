//! Rules about the global variables of Lua code. The Lua adapter resolves
//! names and reports one fact for each use of a global. These rules decide
//! which uses are allowed.

use std::collections::BTreeSet;

use crate::{
    canonical::{GlobalAccessKind, GlobalCall, GlobalFact, SemanticFacts},
    diagnostic::{Diagnostic, DiagnosticCategory},
};

/// The names that `restrictedGlobals` forbids when a rule gives no list. Each
/// name also forbids everything below it: `debug` forbids `debug.getinfo`.
pub const DEFAULT_RESTRICTED_GLOBALS: [&str; 11] = [
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
];

/// Reports reads of undeclared globals and assignments to globals.
pub fn evaluate_accidental_globals(
    rule_id: &str,
    allow: &BTreeSet<String>,
    allow_write: &BTreeSet<String>,
    facts: &SemanticFacts,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for fact in &facts.globals {
        let diagnostic = match fact.kind {
            GlobalAccessKind::Read if !allow.contains(&fact.name) => Some(undeclared_read(fact)),
            GlobalAccessKind::Write if !allow_write.contains(&fact.name) => {
                Some(global_assignment(fact))
            }
            GlobalAccessKind::DynamicWrite => Some(dynamic_assignment(fact)),
            _ => None,
        };
        diagnostics.extend(diagnostic.map(|diagnostic| {
            diagnostic
                .with_rule(rule_id)
                .with_source(fact.range.clone())
        }));
    }
    diagnostics
}

fn undeclared_read(fact: &GlobalFact) -> Diagnostic {
    Diagnostic::new(
        "CTC4401",
        DiagnosticCategory::UndeclaredGlobalRead,
        format!(
            "The global `{}` is read, but no declaration binds it. Declare it with `local`, or add it to `allow`.",
            shown_name(fact)
        ),
        1,
    )
    .with_values(
        "a local variable or an allowed global",
        format!("undeclared global `{}`", shown_name(fact)),
    )
}

fn global_assignment(fact: &GlobalFact) -> Diagnostic {
    Diagnostic::new(
        "CTC4402",
        DiagnosticCategory::GlobalAssignment,
        format!(
            "The global `{}` is assigned. Use a local variable, or add the name to `allowWrite`.",
            shown_name(fact)
        ),
        1,
    )
    .with_values(
        "an assignment to a local variable",
        format!("assignment to global `{}`", shown_name(fact)),
    )
}

fn dynamic_assignment(fact: &GlobalFact) -> Diagnostic {
    Diagnostic::new(
        "CTC4402",
        DiagnosticCategory::GlobalAssignment,
        "A global is assigned through a table key that is not a string literal. Use a local variable.",
        1,
    )
    .with_values(
        "an assignment to a local variable",
        format!("assignment to `{}`", shown_path(fact)),
    )
}

/// Reports uses of the forbidden names, calls of `require` that do not pass
/// one string literal, and keys of `_G` that are not string literals.
pub fn evaluate_restricted_globals(
    rule_id: &str,
    forbid: &[String],
    forbid_dynamic_require: bool,
    facts: &SemanticFacts,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for fact in &facts.globals {
        let diagnostic = match fact.kind {
            GlobalAccessKind::DynamicRead | GlobalAccessKind::DynamicWrite => {
                Some(dynamic_table_key(fact))
            }
            _ => restricted_name(forbid, fact)
                .or_else(|| dynamic_require(fact).filter(|_| forbid_dynamic_require)),
        };
        diagnostics.extend(diagnostic.map(|diagnostic| {
            diagnostic
                .with_rule(rule_id)
                .with_source(fact.range.clone())
        }));
    }
    diagnostics
}

fn restricted_name(forbid: &[String], fact: &GlobalFact) -> Option<Diagnostic> {
    forbid.iter().find_map(|pattern| {
        let message = match relation(fact, pattern) {
            Relation::None => return None,
            Relation::Within => format!(
                "`{}` is restricted. This rule forbids `{pattern}`.",
                shown_path(fact)
            ),
            Relation::Contains => format!(
                "`{}` can reach `{pattern}`, which this rule forbids. Use only the names that you need.",
                shown_path(fact)
            ),
        };
        Some(
            Diagnostic::new("CTC4403", DiagnosticCategory::RestrictedGlobal, message, 1)
                .with_values(format!("no use of `{pattern}`"), format!("`{}`", shown_path(fact))),
        )
    })
}

fn dynamic_require(fact: &GlobalFact) -> Option<Diagnostic> {
    if fact.kind != GlobalAccessKind::Read
        || fact.path != "require"
        || fact.call == GlobalCall::StringLiteral
    {
        return None;
    }
    let message = match fact.call {
        GlobalCall::NotCalled => {
            "`require` is used as a value. Call it with one string literal as its argument."
        }
        _ => "`require` needs one string literal as its argument.",
    };
    Some(
        Diagnostic::new("CTC4404", DiagnosticCategory::DynamicRequire, message, 1).with_values(
            "require(\"module.name\")",
            "a module name that is not a literal",
        ),
    )
}

fn dynamic_table_key(fact: &GlobalFact) -> Diagnostic {
    Diagnostic::new(
        "CTC4405",
        DiagnosticCategory::DynamicGlobalAccess,
        "The global table is indexed with a key that is not a string literal. The code can reach a restricted function.",
        1,
    )
    .with_values("a string literal as the key", format!("`{}`", shown_path(fact)))
}

enum Relation {
    None,
    /// The path is the pattern or is below it.
    Within,
    /// The path is above the pattern and the code uses it as a whole value or
    /// with a key that is not a string literal, so it can reach the pattern.
    Contains,
}

fn relation(fact: &GlobalFact, pattern: &str) -> Relation {
    let path = fact.path.as_str();
    if path == pattern || is_below(path, pattern) {
        return Relation::Within;
    }
    let called = fact.call != GlobalCall::NotCalled;
    if is_below(pattern, path) && (fact.dynamic_key || !called) {
        return Relation::Contains;
    }
    Relation::None
}

/// True when `path` is `parent` followed by one or more `.name` parts.
fn is_below(path: &str, parent: &str) -> bool {
    path.strip_prefix(parent)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// The name as the code wrote it: `_G.print` for a name that `_G` reached.
fn shown_name(fact: &GlobalFact) -> String {
    if fact.via_table {
        format!("_G.{}", fact.name)
    } else {
        fact.name.clone()
    }
}

fn shown_path(fact: &GlobalFact) -> String {
    let mut shown = if fact.name.is_empty() {
        "_G[...]".to_string()
    } else if fact.via_table {
        format!("_G.{}", fact.path)
    } else {
        fact.path.clone()
    };
    if fact.dynamic_key && !fact.name.is_empty() {
        shown.push_str("[...]");
    }
    shown
}

#[cfg(test)]
mod tests;
