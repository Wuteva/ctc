use std::{path::Path, sync::Arc};

use ctc_core::canonical::{CanonicalNode, CanonicalScalar};

use super::{canonical_kind, parse_source, parse_tree};

fn parse(source: &str) -> CanonicalNode {
    parse_source(source, Path::new("source.lua")).unwrap().root
}

fn find<'a>(node: &'a CanonicalNode, kind: &str) -> Vec<&'a CanonicalNode> {
    let mut found = Vec::new();
    if node.kind.as_ref() == kind {
        found.push(node);
    }
    for child in &node.children {
        found.extend(find(child, kind));
    }
    found
}

fn field(node: &CanonicalNode, name: &str) -> Option<CanonicalScalar> {
    node.fields.get(name).cloned()
}

fn text(value: &str) -> Option<CanonicalScalar> {
    Some(CanonicalScalar::String(Arc::from(value)))
}

/// Each sample is valid Lua 5.5.1: `luac -p` accepts it.
const LUA_55_SAMPLES: &[&str] = &[
    "global x\n",
    "global x, y = 1, 2\n",
    "global function f() end\n",
    "global<const> *\n",
    "global *\n",
    "global<const> x, y\n",
    "global x <const>, _G\n",
    "local x <const> = 1\n",
    "local f <close> = setmetatable({}, {__close = function() end})\n",
    "local<const> a, b = 1, 2\n",
    "local function f(...t) return t.n end\n",
    "local function f(a, ...args) return a, args[1] end\n",
    "for i = 1, 10, 2 do print(i) end\n",
    "for k, v in pairs({}) do print(k, v) end\n",
    "for i = 1, 3 do if i == 2 then goto continue end ::continue:: end\n",
    "local x = 7 // 2\n",
    "local r = 1 & 2 | 3 ~ 4 << 1 >> 2\nlocal n = ~r\n",
    "local s = \"\\u{1F600}\\z\n   x\\x41\\65\\n\"\n",
    "local s = [==[ a ]] b ]==]\n--[==[ comment ]] still ]==]\n",
    "local v = 0x1p4 + 0xA.8p0 + 1e10 + 3. + .5\n",
    "local o = {m = function() end}\no:m(1)\nprint\"str\"\nprint{1, 2}\n",
    "#!/usr/bin/env lua\nprint(1)\n",
    ";;local x = 1;\n",
    "global<const> *\nglobal X\nX = 1\n",
    "local global = 1\nprint(global)\n",
    "local t = {global = 1}\nprint(t.global)\n",
    "local s = \"a\\\r\nb\"\r\n",
    "\u{feff}print(1)\n",
    "",
    "-- only a comment",
];

#[test]
fn parses_lua_55_syntax() {
    for source in LUA_55_SAMPLES {
        assert!(
            parse_source(source, Path::new("source.lua")).is_ok(),
            "{source:?}"
        );
    }
}

#[test]
fn reports_invalid_syntax_with_source_ranges() {
    for source in [
        "local function f( end\n",
        "local function f(...t, a) end\n",
        "global function a.b() end\n",
        "x += 1\n",
        "for i = 1, 2 do continue end\n",
        "local x = 1 --[[ open",
        "local s = \"open",
    ] {
        let diagnostics = parse_source(source, Path::new("source.lua")).unwrap_err();
        assert!(!diagnostics.is_empty(), "{source:?}");
        for diagnostic in diagnostics {
            assert_eq!(diagnostic.code, "CTC3001", "{source:?}");
            assert!(diagnostic.source.is_some(), "{source:?}");
        }
    }
}

#[test]
fn maps_shared_canonical_kinds() {
    let tree = parse_tree("local function f() return function() end end\nf()\n").unwrap();
    let root = tree.root_node();
    assert_eq!(canonical_kind(root), "SourceFile");
    let declaration = root.named_child(0).unwrap();
    assert_eq!(canonical_kind(declaration), "FunctionDeclaration");
    let body = declaration.child_by_field_name("body").unwrap();
    assert_eq!(canonical_kind(body), "StatementBlock");
    assert_eq!(
        canonical_kind(root.named_child(1).unwrap()),
        "CallExpression"
    );
}

#[test]
fn marks_local_and_global_declarations() {
    let root = parse(
        "local a = 1\nglobal b\nglobal<const> *\nlocal function f() end\nglobal function g() end\nfunction M.h() end\n",
    );
    let declarations = root.children.iter().collect::<Vec<_>>();
    let flags = declarations
        .iter()
        .map(|node| {
            (
                node.kind.to_string(),
                field(node, "local"),
                field(node, "global"),
            )
        })
        .collect::<Vec<_>>();
    let yes = Some(CanonicalScalar::Bool(true));
    let no = Some(CanonicalScalar::Bool(false));
    assert_eq!(
        flags,
        vec![
            ("VariableDeclaration".to_string(), yes.clone(), no.clone()),
            ("VariableDeclaration".to_string(), no.clone(), yes.clone()),
            (
                "ImplicitVariableDeclaration".to_string(),
                no.clone(),
                yes.clone()
            ),
            ("FunctionDeclaration".to_string(), yes.clone(), no.clone()),
            ("FunctionDeclaration".to_string(), no.clone(), yes),
            ("FunctionDeclaration".to_string(), no.clone(), no),
        ]
    );
}

#[test]
fn sets_callee_and_module_on_calls() {
    let root = parse(
        "require(\"app.parts\")\nrequire 'app.bots'\nrequire [[app.rules]]\nrequire(name)\nstring.format(\"%d\", 1)\nself:emit(1)\n_G[\"load\"](\"x\")\n",
    );
    let calls = find(&root, "CallExpression");
    let callees = calls
        .iter()
        .map(|call| field(call, "callee"))
        .collect::<Vec<_>>();
    assert_eq!(
        callees,
        vec![
            text("require"),
            text("require"),
            text("require"),
            text("require"),
            text("string.format"),
            text("self:emit"),
            text("_G[\"load\"]"),
        ]
    );
    let modules = calls
        .iter()
        .map(|call| field(call, "module"))
        .collect::<Vec<_>>();
    assert_eq!(
        modules,
        vec![
            text("app.parts"),
            text("app.bots"),
            text("app.rules"),
            None,
            None,
            None,
            None,
        ]
    );
}

#[test]
fn decodes_equal_strings_to_the_same_value() {
    let root = parse("local a, b, c = \"x\\65\", 'xA', [[xA]]\n");
    let values = find(&root, "StringLiteral")
        .iter()
        .map(|node| node.value.clone())
        .collect::<Vec<_>>();
    assert_eq!(values, vec![text("xA"), text("xA"), text("xA")]);
}

#[test]
fn ignores_comments_semicolons_and_separators() {
    let plain = parse("local t = {1, 2}\nreturn t\n");
    let noisy = parse("-- comment\n;local t = {1; 2;} --[[ c ]];\nreturn t;\n");
    assert!(plain.structurally_eq(&noisy));
}

#[test]
fn adds_an_empty_block_to_empty_bodies() {
    for source in [
        "function f() end\n",
        "local g = function() end\n",
        "do end\n",
        "while x do end\n",
        "repeat until x\n",
        "for i = 1, 2 do end\n",
        "if x then elseif y then else end\n",
        "function f() -- comment\nend\n",
    ] {
        let root = parse(source);
        let blocks = find(&root, "StatementBlock");
        assert!(!blocks.is_empty(), "{source:?}");
        assert!(
            blocks.iter().all(|block| block.children.is_empty()),
            "{source:?}"
        );
    }
    let if_statement = parse("if x then elseif y then else end\n");
    assert_eq!(find(&if_statement, "StatementBlock").len(), 3);
}

#[test]
fn reads_global_as_a_name_outside_declarations() {
    let root = parse("global = 1\n");
    let names = find(&root, "Identifier");
    assert_eq!(names.len(), 1);
    assert_eq!(names[0].value, text("global"));
}

#[test]
fn exposes_comment_ranges_for_suppressions() {
    let source = "-- one\nlocal x = 1 --[[ two ]]\n";
    let parsed = parse_source(source, Path::new("source.lua")).unwrap();
    let comments = parsed
        .comments
        .iter()
        .map(|range| &source[range.clone()])
        .collect::<Vec<_>>();
    assert_eq!(comments, vec!["-- one", "--[[ two ]]"]);
}
