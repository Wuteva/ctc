use std::path::Path;

use ctc_core::{
    canonical::{GlobalAccessKind, GlobalCall, GlobalFact},
    diagnostic::LineIndex,
};

use super::extract_globals;
use crate::canonicalize::{parse_source, parse_tree};

fn facts(source: &str) -> Vec<GlobalFact> {
    let parsed = parse_source(source, Path::new("a.lua")).expect("the sample parses");
    parsed.semantic_facts.globals
}

/// One short line for each fact: `R name path`, `W name path`, `DR`, `DW`.
/// A `via` suffix marks `_G` and `_ENV` access. A `?` marks a dynamic key and
/// a `(call)` or `(lit)` suffix marks the callee of a call.
fn summary(source: &str) -> Vec<String> {
    facts(source)
        .iter()
        .map(|fact| {
            let kind = match fact.kind {
                GlobalAccessKind::Read => "R",
                GlobalAccessKind::Write => "W",
                GlobalAccessKind::DynamicRead => "DR",
                GlobalAccessKind::DynamicWrite => "DW",
            };
            let mut text = format!("{kind} {}", fact.path);
            if fact.dynamic_key {
                text.push('?');
            }
            if fact.via_table {
                text.push_str(" via");
            }
            match fact.call {
                GlobalCall::NotCalled => {}
                GlobalCall::StringLiteral => text.push_str(" (lit)"),
                GlobalCall::Other => text.push_str(" (call)"),
            }
            text
        })
        .collect()
}

fn lines(items: &[&str]) -> Vec<String> {
    items.iter().map(ToString::to_string).collect()
}

#[test]
fn locals_and_parameters_are_not_globals() {
    let source =
        "local a = 1\nlocal function f(b, ...)\n  local c = a + b\n  return c\nend\nreturn f(a)\n";
    assert_eq!(summary(source), Vec::<String>::new());
}

#[test]
fn reads_and_writes_of_free_names() {
    let source = "count = count + 1\nprint(count)\n";
    assert_eq!(
        summary(source),
        lines(&["W count", "R count", "R print (call)", "R count"])
    );
}

#[test]
fn a_local_is_visible_only_after_its_declaration() {
    let source = "local function f()\n  return g()\nend\nlocal function g() end\nlocal x = x\n";
    assert_eq!(summary(source), lines(&["R g (call)", "R x"]));
}

#[test]
fn local_function_sees_itself_but_local_value_does_not() {
    let source = "local function f() return f() end\nlocal g = function() return g() end\n";
    assert_eq!(summary(source), lines(&["R g (call)"]));
}

#[test]
fn block_scopes_end_at_the_block() {
    let source = "do\n  local a = 1\nend\nreturn a\n";
    assert_eq!(summary(source), lines(&["R a"]));
}

#[test]
fn loop_variables_belong_to_the_body() {
    let source = "for i = 1, n do\n  print(i)\nend\nfor k, v in pairs(t) do\n  print(k, v)\nend\nreturn i, k\n";
    assert_eq!(
        summary(source),
        lines(&[
            "R n",
            "R print (call)",
            "R pairs (call)",
            "R t",
            "R print (call)",
            "R i",
            "R k"
        ])
    );
}

#[test]
fn repeat_condition_sees_the_body_locals() {
    let source = "repeat\n  local done = step()\nuntil done\nreturn done\n";
    assert_eq!(summary(source), lines(&["R step (call)", "R done"]));
}

#[test]
fn method_functions_declare_self() {
    let source = "local M = {}\nfunction M:run(a)\n  return self.x + a\nend\nfunction M.stop()\n  return self\nend\nreturn M\n";
    assert_eq!(summary(source), lines(&["R self"]));
}

#[test]
fn named_vararg_parameter_is_a_local() {
    let source = "local function f(a, ...rest)\n  return rest.n + a\nend\n";
    assert_eq!(summary(source), Vec::<String>::new());
}

#[test]
fn function_statements_write_a_free_name_only_for_a_plain_name() {
    let source = "local M = {}\nfunction update() end\nfunction M.run() end\nfunction M.sub.run() end\nfunction other.run() end\nlocal function helper() end\nfunction helper() end\n";
    assert_eq!(summary(source), lines(&["W update", "R other.run"]));
}

#[test]
fn table_keys_are_not_variables_but_values_and_bracket_keys_are() {
    let source = "local t = { a = 1, [b] = c, d, e = f, global = g }\nreturn t\n";
    assert_eq!(summary(source), lines(&["R b", "R c", "R d", "R f", "R g"]));
}

#[test]
fn field_access_extends_the_path_with_string_keys() {
    let source = "local x = os.time()\nlocal y = string[\"dump\"]\nlocal z = t.a[k].b\nlocal w = io.stdout:write(\"x\")\n";
    assert_eq!(
        summary(source),
        lines(&[
            "R os.time (call)",
            "R string.dump",
            "R t.a?",
            "R k",
            "R io.stdout"
        ])
    );
}

#[test]
fn assignments_to_fields_read_the_base_name() {
    let source = "package.path = \"x\"\nconfig.limit = 3\nlocal t = {}\nt.x = 1\n";
    assert_eq!(
        summary(source),
        lines(&["R package.path", "R config.limit"])
    );
}

#[test]
fn global_table_access_names_the_global() {
    let source = "local a = _G.print\n_G.count = 1\n_ENV[\"total\"] = 2\nlocal b = _G.os.time\nlocal c = _G\n_G.os.x = 1\n";
    assert_eq!(
        summary(source),
        lines(&[
            "R print via",
            "W count via",
            "W total via",
            "R os.time via",
            "R _G",
            "R os.x via"
        ])
    );
}

#[test]
fn global_table_with_a_dynamic_key_is_its_own_fact() {
    let source = "local a = _G[name]\n_G[name] = 1\n_ENV[k].x = 2\n";
    assert_eq!(
        summary(source),
        lines(&[
            "DR ? via", "R name", "DW ? via", "R name", "DR ? via", "R k"
        ])
    );
}

#[test]
fn a_local_named_g_is_an_ordinary_table() {
    let source = "local _G = {}\n_G.count = 1\nreturn _G.os\n";
    assert_eq!(summary(source), Vec::<String>::new());
}

#[test]
fn a_local_env_hides_the_free_names() {
    let source = "local _ENV = sandbox\nprint(x)\nlocal function f(_ENV)\n  return y\nend\n";
    assert_eq!(summary(source), lines(&["R sandbox"]));
}

#[test]
fn global_declarations_bind_names_in_their_scope() {
    let source = "global score, LIMIT <const>\nscore = score + 1\nglobal function reset() end\nreset()\nreturn other\n";
    assert_eq!(summary(source), lines(&["R other"]));
}

#[test]
fn collective_global_declaration_binds_free_names_but_not_locals() {
    let source = "global<const> *\nprint(x)\nlocal y = 1\nreturn y\n";
    assert_eq!(summary(source), Vec::<String>::new());
    let inner = "local function f()\n  global *\n  return a\nend\nreturn b\n";
    assert_eq!(summary(inner), lines(&["R b"]));
}

#[test]
fn global_as_a_name_is_a_global_variable() {
    let source = "local x = global\nglobal = 1\nreturn global.y\n";
    assert_eq!(
        summary(source),
        lines(&["R global", "W global", "R global.y"])
    );
}

#[test]
fn require_calls_record_their_argument_shape() {
    let source = "local a = require(\"x\")\nlocal b = require \"y\"\nlocal c = require(name)\nlocal d = require(\"a\", \"b\")\nlocal e = _G.require(\"z\")\nlocal f = pcall(require, \"z\")\nlocal g = require{ 1 }\n";
    assert_eq!(
        summary(source),
        lines(&[
            "R require (lit)",
            "R require (lit)",
            "R require (call)",
            "R name",
            "R require (call)",
            "R require via (lit)",
            "R pcall (call)",
            "R require",
            "R require (call)"
        ])
    );
}

#[test]
fn method_call_reads_the_object() {
    let source = "obj:run(a)\nlocal s = (\"x\"):rep(3)\nf():g()\n";
    assert_eq!(summary(source), lines(&["R obj", "R a", "R f (call)"]));
}

#[test]
fn ranges_cover_the_whole_path() {
    let found = facts("local t = os.time()\nlocal u = _G.io.open\n");
    assert_eq!(found[0].range.start.column, 11);
    assert_eq!(found[0].range.end.column, 18);
    assert_eq!(found[1].range.start.line, 2);
    assert_eq!(found[1].range.end.column, 21);
}

#[test]
fn goto_labels_and_attributes_are_not_variables() {
    let source = "local x <const> = 1\n::top::\ngoto top\nlocal y <close> = nil\n";
    assert_eq!(summary(source), Vec::<String>::new());
}

#[test]
fn a_loop_variable_can_shadow_and_the_outer_name_returns_after_the_loop() {
    let source = "local i = 0\nfor i = 1, 3 do\n  local j = i\nend\nfor _, j in ipairs(i) do\n  print(j)\nend\nreturn j\n";
    assert_eq!(
        summary(source),
        lines(&["R ipairs (call)", "R print (call)", "R j"])
    );
}

#[test]
fn closures_see_the_locals_of_the_enclosing_functions() {
    let source = "local function outer(a)\n  local b = a\n  return function(c)\n    return function()\n      return a + b + c + d\n    end\n  end\nend\n";
    assert_eq!(summary(source), lines(&["R d"]));
}

#[test]
fn every_branch_of_a_conditional_has_its_own_scope() {
    let source = "if a then\n  local x = 1\nelseif b then\n  local y = x\nelse\n  local z = y\nend\nwhile c do\n  local w = z\nend\n";
    assert_eq!(
        summary(source),
        lines(&["R a", "R b", "R x", "R y", "R c", "R z"])
    );
}

#[test]
fn a_parenthesized_callee_reads_its_names() {
    let source = "local r = (f or g)(1)\nlocal s = (function() return h end)()\nlocal t = f{ x = i }\nlocal u = f[[text]]\n";
    assert_eq!(
        summary(source),
        lines(&["R f", "R g", "R h", "R f (call)", "R i", "R f (lit)"])
    );
}

#[test]
fn comments_between_the_parts_of_a_statement_are_ignored() {
    let source = "local --[[a]] x --[[b]] = --[[c]] y --[[d]] + --[[e]] z\nw --[[f]] = x --[[g]]\nf(--[[h]] 1 --[[i]])\n";
    assert_eq!(summary(source), lines(&["R y", "R z", "W w", "R f (call)"]));
}

#[test]
fn attributes_and_several_names_in_one_declaration() {
    let source = "local a <const>, b <close> = 1, nil\nlocal<const> c, d = a, b\nreturn c, d, e\n";
    assert_eq!(summary(source), lines(&["R e"]));
}

#[test]
fn a_byte_order_mark_and_windows_line_endings_keep_the_positions() {
    let found = facts("\u{feff}local a = 1\r\nreturn os.time()\r\n");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].range.start.line, 2);
    assert_eq!(found[0].range.start.column, 8);
    assert_eq!(found[0].range.end.column, 15);
}

#[test]
fn a_long_chain_of_operators_and_keys_does_not_use_the_call_stack() {
    let sum = vec!["a"; 20_000].join(" + ");
    let keys = ".b".repeat(5_000);
    let source = format!("local x = {sum}\nlocal y = c{keys}\n");
    let tree = parse_tree(&source).expect("the sample parses");
    let lines = LineIndex::new(Path::new("a.lua"), &source);
    // The canonical tree needs one stack frame for each level, so this test
    // calls the analyzer alone.
    let found = extract_globals(tree.root_node(), &source, &lines);
    assert_eq!(found.len(), 20_001);
}
