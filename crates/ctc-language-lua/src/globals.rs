//! Name resolution for Lua. It finds the uses of global variables, so that a
//! rule can tell an accidental global from a local.

use std::collections::BTreeMap;

use ctc_core::{
    canonical::{GlobalAccessKind, GlobalCall, GlobalFact},
    diagnostic::LineIndex,
};
use tree_sitter::Node;

use crate::{fields::is_declaration_keyword, strings::decode_string};

const GLOBAL_TABLES: [&str; 2] = ["_G", "_ENV"];
const ENV_NAME: &str = "_ENV";

/// The global variables that a chunk uses, in source order.
pub(crate) fn extract_globals(
    root: Node<'_>,
    source: &str,
    lines: &LineIndex<'_>,
) -> Vec<GlobalFact> {
    let mut analyzer = Analyzer {
        source,
        lines,
        scopes: vec![Scope::default()],
        facts: Vec::new(),
    };
    for child in children(root) {
        analyzer.statement(child);
    }
    let mut facts = analyzer.facts;
    facts.sort_by_key(|fact| (fact.range.start.offset, fact.range.end.offset));
    facts
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Binding {
    Local,
    Global,
}

/// The names that one block binds. `global *` makes every name that no other
/// declaration binds a declared global.
#[derive(Default)]
struct Scope {
    names: BTreeMap<String, Binding>,
    all_globals: bool,
}

/// How an access chain is used.
#[derive(Clone, Copy)]
enum Usage {
    Read,
    Write,
    Call(GlobalCall),
}

/// One key of an access chain such as `a.b[c]`.
struct Key<'tree> {
    /// The string key, or `None` for a key that is not a string literal.
    text: Option<String>,
    /// The expression of a bracket key. A dot key has none.
    expression: Option<Node<'tree>>,
    /// The index expression that ends with this key.
    end: Node<'tree>,
}

struct Analyzer<'a> {
    source: &'a str,
    lines: &'a LineIndex<'a>,
    scopes: Vec<Scope>,
    facts: Vec<GlobalFact>,
}

impl<'a> Analyzer<'a> {
    fn statement(&mut self, node: Node<'a>) {
        match node.kind() {
            "assignment_statement" => self.assignment(node),
            "function_call" => self.expression(node),
            "do_statement" => self.scoped_body(node.child_by_field_name("body")),
            "while_statement" => {
                self.optional_expression(node.child_by_field_name("condition"));
                self.scoped_body(node.child_by_field_name("body"));
            }
            "repeat_statement" => self.repeat(node),
            "if_statement" => self.conditional(node),
            "for_statement" => self.for_loop(node),
            "function_declaration" => self.function_declaration(node),
            "variable_declaration" => self.variable_declaration(node),
            "implicit_variable_declaration" => self.declare_all_globals(),
            "return_statement" => self.drain(children(node).collect()),
            _ => {}
        }
    }

    fn expression(&mut self, node: Node<'a>) {
        self.drain(vec![node]);
    }

    /// Visits expressions with a work list, so that a long chain of operators
    /// or a deep table does not use the call stack.
    fn drain(&mut self, mut pending: Vec<Node<'a>>) {
        while let Some(node) = pending.pop() {
            self.visit_expression(node, &mut pending);
        }
    }

    fn visit_expression(&mut self, node: Node<'a>, pending: &mut Vec<Node<'a>>) {
        match node.kind() {
            "dot_index_expression" | "bracket_index_expression" => {
                self.access(node, Usage::Read, pending);
            }
            "function_call" => self.call(node, pending),
            "function_definition" => self.function_body(node, false),
            "table_constructor" => self.table(node, pending),
            "comment" | "string" | "number" => {}
            _ if is_name(node) => self.access(node, Usage::Read, pending),
            _ => pending.extend(children(node)),
        }
    }

    fn optional_expression(&mut self, node: Option<Node<'a>>) {
        if let Some(node) = node {
            self.expression(node);
        }
    }

    fn text(&self, node: Node<'_>) -> String {
        self.source
            .get(node.start_byte()..node.end_byte())
            .unwrap_or_default()
            .to_string()
    }

    fn declare(&mut self, name: String, binding: Binding) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.names.insert(name, binding);
        }
    }

    fn declare_all_globals(&mut self) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.all_globals = true;
        }
    }

    /// True when the name is a global variable here: no `local` or `global`
    /// declaration, parameter, or loop variable binds it, and no local `_ENV`
    /// replaces the global table.
    fn is_free(&self, name: &str) -> bool {
        let bound = self
            .scopes
            .iter()
            .any(|scope| scope.names.contains_key(name));
        let declared_by_star = self.scopes.iter().any(|scope| scope.all_globals);
        let env_replaced = self
            .scopes
            .iter()
            .any(|scope| scope.names.get(ENV_NAME) == Some(&Binding::Local));
        !(bound || declared_by_star || env_replaced)
    }

    fn scoped_body(&mut self, body: Option<Node<'a>>) {
        self.scopes.push(Scope::default());
        self.block(body);
        self.scopes.pop();
    }

    fn block(&mut self, body: Option<Node<'a>>) {
        if let Some(body) = body {
            children(body).for_each(|child| self.statement(child));
        }
    }

    /// The condition of `repeat ... until` sees the locals of the body.
    fn repeat(&mut self, node: Node<'a>) {
        self.scopes.push(Scope::default());
        self.block(node.child_by_field_name("body"));
        self.optional_expression(node.child_by_field_name("condition"));
        self.scopes.pop();
    }

    fn conditional(&mut self, node: Node<'a>) {
        self.optional_expression(node.child_by_field_name("condition"));
        self.scoped_body(node.child_by_field_name("consequence"));
        for branch in children(node) {
            match branch.kind() {
                "elseif_statement" => {
                    self.optional_expression(branch.child_by_field_name("condition"));
                    self.scoped_body(branch.child_by_field_name("consequence"));
                }
                "else_statement" => self.scoped_body(branch.child_by_field_name("body")),
                _ => {}
            }
        }
    }

    fn for_loop(&mut self, node: Node<'a>) {
        let mut names = Vec::new();
        if let Some(clause) = node.child_by_field_name("clause") {
            for (field, child) in children_with_fields(clause) {
                match (child.kind(), field) {
                    ("variable_list", _) | ("identifier", Some("name")) => {
                        names.extend(self.declared_names(child));
                    }
                    ("expression_list", _) | (_, Some("start" | "end" | "step")) => {
                        self.expression(child);
                    }
                    _ => {}
                }
            }
        }
        self.scopes.push(Scope::default());
        for name in names {
            self.declare(name, Binding::Local);
        }
        self.block(node.child_by_field_name("body"));
        self.scopes.pop();
    }

    /// The names of a `variable_list` or a `parameters` node, or the name of
    /// a single identifier.
    fn declared_names(&self, node: Node<'_>) -> Vec<String> {
        if node.kind() == "identifier" {
            return vec![self.text(node)];
        }
        children(node)
            .filter(|child| child.kind() == "identifier")
            .map(|child| self.text(child))
            .collect()
    }

    /// `local` and `global` declarations. The values see the scope before the
    /// declaration, so `local x = x` reads an outer `x`.
    fn variable_declaration(&mut self, node: Node<'a>) {
        let binding = match node.child(0).map(|keyword| keyword.kind()) {
            Some("global") => Binding::Global,
            _ => Binding::Local,
        };
        let mut names = Vec::new();
        let mut values = Vec::new();
        for child in children(node) {
            match child.kind() {
                "variable_list" => names.extend(self.declared_names(child)),
                "assignment_statement" => {
                    for part in children(child) {
                        match part.kind() {
                            "variable_list" => names.extend(self.declared_names(part)),
                            "expression_list" => values.push(part),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        self.drain(values);
        for name in names {
            self.declare(name, binding);
        }
    }

    fn function_declaration(&mut self, node: Node<'a>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let keyword = node.child(0).map_or("", |keyword| keyword.kind());
        match keyword {
            "local" => self.declare(self.text(name), Binding::Local),
            "global" => self.declare(self.text(name), Binding::Global),
            _ if name.kind() == "method_index_expression" => {
                self.optional_expression(name.child_by_field_name("table"));
                self.function_body(node, true);
                return;
            }
            _ => {
                let mut pending = Vec::new();
                self.access(name, Usage::Write, &mut pending);
                self.drain(pending);
            }
        }
        self.function_body(node, false);
    }

    /// The scope of a function: its parameters, `self` for a method, and its
    /// body.
    fn function_body(&mut self, node: Node<'a>, method: bool) {
        self.scopes.push(Scope::default());
        if method {
            self.declare("self".to_string(), Binding::Local);
        }
        if let Some(parameters) = node.child_by_field_name("parameters") {
            for name in self.declared_names(parameters) {
                self.declare(name, Binding::Local);
            }
        }
        self.block(node.child_by_field_name("body"));
        self.scopes.pop();
    }

    fn assignment(&mut self, node: Node<'a>) {
        let mut pending = Vec::new();
        for child in children(node) {
            match child.kind() {
                "variable_list" => {
                    for target in children(child).filter(|target| is_target(*target)) {
                        self.access(target, Usage::Write, &mut pending);
                    }
                }
                "expression_list" => pending.push(child),
                _ => {}
            }
        }
        self.drain(pending);
    }

    fn call(&mut self, node: Node<'a>, pending: &mut Vec<Node<'a>>) {
        let arguments = node.child_by_field_name("arguments");
        if let Some(callee) = node.child_by_field_name("name") {
            if callee.kind() == "method_index_expression" {
                pending.extend(callee.child_by_field_name("table"));
            } else if is_target(callee) {
                self.access(callee, Usage::Call(call_shape(arguments)), pending);
            } else {
                pending.push(callee);
            }
        }
        if let Some(arguments) = arguments {
            pending.extend(children(arguments));
        }
    }

    /// The key of a field `name = value` is not a variable. The key of
    /// `[expression] = value` is an expression.
    fn table(&mut self, table: Node<'a>, pending: &mut Vec<Node<'a>>) {
        for field in children(table).filter(|child| child.kind() == "field") {
            let bracket = field.child(0).is_some_and(|first| first.kind() == "[");
            for (name, part) in children_with_fields(field) {
                match name {
                    Some("name") if !bracket => {}
                    Some("name" | "value") => pending.push(part),
                    _ => {}
                }
            }
        }
    }

    /// A name, or a chain of keys on a name, such as `os.time` or `t[k].x`.
    fn access(&mut self, node: Node<'a>, usage: Usage, pending: &mut Vec<Node<'a>>) {
        let (root, keys) = split_chain(node, self.source);
        for key in &keys {
            if let (None, Some(expression)) = (&key.text, key.expression) {
                pending.push(expression);
            }
        }
        if !is_name(root) {
            pending.push(root);
            return;
        }
        let name = self.text(root);
        if !self.is_free(&name) {
            return;
        }
        if GLOBAL_TABLES.contains(&name.as_str()) && !keys.is_empty() {
            self.global_table_access(root, &keys, usage);
        } else {
            self.record(root, name, &keys, 0, usage);
        }
    }

    /// `_G.name` and `_G["name"]` reach the global `name`. `_G[key]` reaches a
    /// global that no rule can name.
    fn global_table_access(&mut self, root: Node<'a>, keys: &[Key<'a>], usage: Usage) {
        if let Some(name) = keys[0].text.clone() {
            self.record(root, name, keys, 1, usage);
            return;
        }
        let kind = if matches!(usage, Usage::Write) && keys.len() == 1 {
            GlobalAccessKind::DynamicWrite
        } else {
            GlobalAccessKind::DynamicRead
        };
        self.facts.push(GlobalFact {
            kind,
            name: String::new(),
            path: String::new(),
            dynamic_key: true,
            via_table: true,
            call: GlobalCall::NotCalled,
            range: self.lines.range(root.start_byte(), keys[0].end.end_byte()),
        });
    }

    /// Records one use. `keys[skip..]` follow the name. The path takes the
    /// string keys up to the first key that is not a string.
    fn record(
        &mut self,
        root: Node<'a>,
        name: String,
        keys: &[Key<'a>],
        skip: usize,
        usage: Usage,
    ) {
        let rest = &keys[skip..];
        let literal = rest.iter().take_while(|key| key.text.is_some()).count();
        let mut path = name.clone();
        for key in &rest[..literal] {
            path.push('.');
            path.push_str(key.text.as_deref().unwrap_or_default());
        }
        let included = skip + literal;
        let end = match included {
            0 => root.end_byte(),
            count => keys[count - 1].end.end_byte(),
        };
        let kind = if rest.is_empty() && matches!(usage, Usage::Write) {
            GlobalAccessKind::Write
        } else {
            GlobalAccessKind::Read
        };
        let call = match usage {
            Usage::Call(call) if literal == rest.len() => call,
            _ => GlobalCall::NotCalled,
        };
        self.facts.push(GlobalFact {
            kind,
            name,
            path,
            dynamic_key: literal < rest.len(),
            via_table: skip > 0,
            call,
            range: self.lines.range(root.start_byte(), end),
        });
    }
}

/// An identifier, or the word `global` used as a name.
fn is_name(node: Node<'_>) -> bool {
    node.kind() == "identifier" || (node.kind() == "global" && !is_declaration_keyword(node))
}

/// A name or an index expression: what an assignment or a call can name.
fn is_target(node: Node<'_>) -> bool {
    is_name(node)
        || matches!(
            node.kind(),
            "dot_index_expression" | "bracket_index_expression"
        )
}

/// The root name or expression of an access chain, and the keys that apply
/// to it, innermost first.
fn split_chain<'tree>(node: Node<'tree>, source: &str) -> (Node<'tree>, Vec<Key<'tree>>) {
    let mut keys = Vec::new();
    let mut current = node;
    loop {
        let (text, expression) = match current.kind() {
            "dot_index_expression" => {
                let field = current.child_by_field_name("field");
                let text = field.and_then(|field| node_text(field, source));
                (text, None)
            }
            "bracket_index_expression" => {
                let field = current.child_by_field_name("field");
                let text = field
                    .filter(|field| field.kind() == "string")
                    .and_then(|field| node_text(field, source))
                    .and_then(|text| decode_string(&text));
                (text, field)
            }
            _ => break,
        };
        keys.push(Key {
            text,
            expression,
            end: current,
        });
        let Some(table) = current.child_by_field_name("table") else {
            break;
        };
        current = table;
    }
    keys.reverse();
    (current, keys)
}

fn node_text(node: Node<'_>, source: &str) -> Option<String> {
    source
        .get(node.start_byte()..node.end_byte())
        .map(str::to_string)
}

/// `require "x"` and `require("x")` pass one string literal.
fn call_shape(arguments: Option<Node<'_>>) -> GlobalCall {
    let Some(arguments) = arguments else {
        return GlobalCall::Other;
    };
    let mut values =
        children(arguments).filter(|child| child.is_named() && child.kind() != "comment");
    match (values.next(), values.next()) {
        (Some(value), None) if value.kind() == "string" => GlobalCall::StringLiteral,
        _ => GlobalCall::Other,
    }
}

fn children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    children_with_fields(node)
        .into_iter()
        .map(|(_, child)| child)
}

fn children_with_fields<'tree>(node: Node<'tree>) -> Vec<(Option<&'tree str>, Node<'tree>)> {
    let mut cursor = node.walk();
    let mut result = Vec::new();
    if cursor.goto_first_child() {
        loop {
            result.push((cursor.field_name(), cursor.node()));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;
