use std::path::Path;

use ctc_core::{
    diagnostic::Diagnostic,
    language::LanguageAdapter,
    matcher::{
        MatchMode, MatchResult, SearchScope, match_template, match_template_in_scope,
        match_template_in_scope_with_kinds,
    },
    template::{CompiledTemplate, TemplateNode, parse_placeholders},
};

use crate::LuaAdapter;

fn try_compile(source: &str, mode: MatchMode) -> Result<CompiledTemplate, Vec<Diagnostic>> {
    let adapter = LuaAdapter::new();
    let path = Path::new("rule.lua.ctmpl");
    let ranges = adapter.scan_placeholders(source, path)?;
    let placeholders = parse_placeholders(source, path, &ranges, &adapter)?;
    adapter.compile_template(source, path, &placeholders, mode)
}

fn compile(source: &str, mode: MatchMode) -> CompiledTemplate {
    try_compile(source, mode).unwrap()
}

fn error_codes(source: &str, mode: MatchMode) -> Vec<String> {
    try_compile(source, mode)
        .unwrap_err()
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn run(template: &CompiledTemplate, source: &str, scope: SearchScope) -> MatchResult {
    let adapter = LuaAdapter::new();
    let parsed = adapter.parse(source, Path::new("source.lua")).unwrap();
    let validate = |value: &str| adapter.validate_identifier(value);
    if template.mode == MatchMode::Exact {
        match_template("rule", template, &parsed.root, &validate)
    } else {
        match_template_in_scope("rule", template, &parsed.root, scope, &validate)
    }
}

const MODULE_SHAPE: &str = "local {{ Module }} = {}\n\n{{* Body }}\n\nreturn {{ Module }}\n";

#[test]
fn module_shape_matches_a_module_file() {
    let template = compile(MODULE_SHAPE, MatchMode::Exact);
    let source = "-- Parts.\nlocal M = {}\n\nlocal function helper() end\n\nfunction M.run(a) return helper(a) end\n\nreturn M\n";
    let result = run(&template, source, SearchScope::TopLevel);
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn module_shape_rejects_other_shapes() {
    let template = compile(MODULE_SHAPE, MatchMode::Exact);
    for source in [
        "local M = {}\nreturn N\n",
        "local M = {}\nfunction M.run() end\n",
        "M = {}\nreturn M\n",
        "local M = {}\nreturn M, 1\n",
    ] {
        let result = run(&template, source, SearchScope::TopLevel);
        assert!(!result.matches, "{source:?}");
    }
}

#[test]
fn forbid_template_finds_load_calls_anywhere() {
    let template = compile(
        r#"{{ Call | kind("CallExpression") | field("callee", "matches", "^(load|loadstring|loadfile|dofile)$") }}"#,
        MatchMode::Forbid,
    );
    let source = "local f = load(\"x\")\nlocal function g()\n  return dofile(\"a.lua\")\nend\nprint(loadstring(\"y\"))\nlocal t = {load = 1}\nt.loader(1)\n";
    let result = run(&template, source, SearchScope::Descendants);
    assert!(!result.matches);
    let lines = result
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.source.as_ref().map(|range| range.start.line))
        .collect::<Vec<_>>();
    assert_eq!(lines, vec![1, 3, 5]);
}

#[test]
fn forbid_template_matches_literal_call_syntax() {
    let template = compile("dofile({{* Args }})\n", MatchMode::Forbid);
    let result = run(
        &template,
        "local function f()\n  dofile(\"x.lua\")\nend\n",
        SearchScope::FunctionBody,
    );
    assert!(!result.matches);
    let result = run(&template, "print(\"x.lua\")\n", SearchScope::Descendants);
    assert!(result.matches);
}

#[test]
fn function_size_template_uses_line_count() {
    let template = compile(
        r#"{{ Long | kind("FunctionDeclaration", "FunctionExpression") | field("lineCount", "greaterThan", 3) }}"#,
        MatchMode::Forbid,
    );
    let short = "local function f()\n  return 1\nend\n";
    assert!(run(&template, short, SearchScope::Descendants).matches);
    let long = "local f = function()\n  local a = 1\n  return a\nend\n";
    assert!(!run(&template, long, SearchScope::Descendants).matches);
}

#[test]
fn every_template_checks_each_function_declaration() {
    let template = compile(
        "function {{ Table }}.{{ Name }}({{* Parameters }})\n{{* Body }}\nend\n",
        MatchMode::Every,
    );
    let adapter = LuaAdapter::new();
    let source =
        "function M.run(a)\n  return a\nend\nfunction M.stop() end\nfunction helper() end\n";
    let parsed = adapter.parse(source, Path::new("source.lua")).unwrap();
    let kinds = vec!["FunctionDeclaration".to_string()];
    let result = match_template_in_scope_with_kinds(
        "rule",
        &template,
        &parsed.root,
        SearchScope::Descendants,
        &kinds,
        &|value| adapter.validate_identifier(value),
    );
    assert!(!result.matches);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].source.as_ref().unwrap().start.line, 5);
}

#[test]
fn optional_keywords_match_present_and_absent_forms() {
    let template = compile(
        "{{? keyword:local }} function {{ Name }}()\n{{* Body }}\nend\n",
        MatchMode::Exact,
    );
    for source in ["local function f() end\n", "function f() return 1 end\n"] {
        let result = run(&template, source, SearchScope::TopLevel);
        assert!(result.matches, "{source:?}: {:?}", result.diagnostics);
    }
    let result = run(
        &template,
        "global function f() end\n",
        SearchScope::TopLevel,
    );
    assert!(!result.matches);
}

#[test]
fn empty_body_templates_and_sources_match() {
    let template = compile("local function {{ Name }}() end\n", MatchMode::Exact);
    assert!(run(&template, "local function f() end\n", SearchScope::TopLevel).matches);
    assert!(
        !run(
            &template,
            "local function f() return end\n",
            SearchScope::TopLevel
        )
        .matches
    );
    let template = compile(
        "if {{ Condition }} then\n{{* Body }}\nend\n",
        MatchMode::Exact,
    );
    assert!(run(&template, "if ready then end\n", SearchScope::TopLevel).matches);
    assert!(run(&template, "if ready then go() end\n", SearchScope::TopLevel).matches);
}

#[test]
fn compiles_supported_sequence_slots() {
    let template = compile(
        "local {{ Name }} = { {{* Fields }} }\n{{ Name }}.run({{* Arguments }})\nlocal function f({{* Parameters }})\n{{* Statements }}\nend\nreturn {{* Values }}\n",
        MatchMode::Exact,
    );
    assert!(matches!(template.root, TemplateNode::Literal { .. }));
    let source = "local t = {1, b = 2}\nt.run(1, 2)\nlocal function f(a, ...)\n  print(a)\n  return a\nend\nreturn t, f\n";
    let result = run(&template, source, SearchScope::TopLevel);
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn reports_unsupported_and_invalid_templates() {
    assert!(
        error_codes("local x = 1 + {{* Values }}\n", MatchMode::Exact)
            .contains(&"CTC2002".to_string())
    );
    assert!(
        error_codes("local {{ type:Name }} = 1\n", MatchMode::Exact)
            .contains(&"CTC2002".to_string())
    );
    assert!(
        error_codes(
            "local function {{ expression:Name }}() end\n",
            MatchMode::Exact
        )
        .contains(&"CTC2002".to_string())
    );
    assert_eq!(
        error_codes("local x = = 1\n", MatchMode::Exact)[0],
        "CTC2001"
    );
    assert_eq!(
        error_codes("local x = 1LL\n", MatchMode::Exact),
        vec!["CTC2001"]
    );
    assert_eq!(
        error_codes("{{* Body }}\n", MatchMode::Forbid),
        vec!["CTC2010"]
    );
}

#[test]
fn template_strings_match_by_decoded_value() {
    let template = compile("local x = require(\"app.parts\")\n", MatchMode::Exact);
    assert!(
        run(
            &template,
            "local x = require 'app.parts'\n",
            SearchScope::TopLevel
        )
        .matches
    );
    assert!(
        run(
            &template,
            "local x = require(\"app.\\112arts\")\n",
            SearchScope::TopLevel
        )
        .matches
    );
}

fn lines(template: &CompiledTemplate, source: &str) -> Vec<u32> {
    run(template, source, SearchScope::Descendants)
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.source.as_ref().map(|range| range.start.line))
        .collect()
}

#[test]
fn bare_assignment_template_skips_local_declarations() {
    let template = compile(
        "{{ Name | kind(\"Identifier\") }} = {{ Value }}\n",
        MatchMode::Forbid,
    );
    let source = "local a = 1\nb = 2\nlocal t = {}\nt.c = 3\nglobal d = 4\nlocal function f()\n  e = 5\nend\n";
    assert_eq!(lines(&template, source), vec![2, 7]);
}

#[test]
fn declaration_fields_find_global_declarations() {
    let template = compile(
        "{{ Declaration | field(\"global\", \"equal\", true) }}\n",
        MatchMode::Forbid,
    );
    let source = "global<const> *\nlocal a = 1\nglobal b\nlocal function f()\n  global function g() end\nend\nfunction M.h() end\n";
    assert_eq!(lines(&template, source), vec![1, 3, 5]);
}

#[test]
fn function_name_template_finds_global_functions() {
    let template = compile(
        "function {{ Name | kind(\"Identifier\") }}({{* Parameters }})\n{{* Body }}\nend\n",
        MatchMode::Forbid,
    );
    let source = "local function a() end\nfunction b() end\nfunction M.c() end\nfunction M:d() end\nglobal function e() end\n";
    assert_eq!(lines(&template, source), vec![2]);
}

#[test]
fn local_declarations_hold_their_names_and_values() {
    let template = compile(
        "local {{ Name }} = require({{ Module }})\n",
        MatchMode::Exact,
    );
    let result = run(
        &template,
        "local Parts = require(\"app.parts\")\n",
        SearchScope::TopLevel,
    );
    assert!(result.matches, "{:?}", result.diagnostics);
    let declaration_only = compile("local {{ Name }}\n", MatchMode::Exact);
    assert!(run(&declaration_only, "local x\n", SearchScope::TopLevel).matches);
    assert!(!run(&declaration_only, "local x = 1\n", SearchScope::TopLevel).matches);
}
