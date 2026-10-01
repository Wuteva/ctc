use ctc_core::{
    canonical::{CallFact, ExceptionFact, ExceptionKind, FunctionFact, SemanticFacts},
    diagnostic::{LineIndex, TextRange},
};
use tree_sitter::Node;

pub fn extract_semantic_facts(
    root: Node<'_>,
    source: &str,
    lines: &LineIndex<'_>,
) -> SemanticFacts {
    let mut facts = SemanticFacts::default();
    collect_facts(root, source, lines, &mut facts);
    facts
}

fn collect_facts(node: Node<'_>, source: &str, lines: &LineIndex<'_>, facts: &mut SemanticFacts) {
    if is_function_like(node.kind()) {
        facts.functions.push(function_fact(node, source, lines));
    }

    match node.kind() {
        "try_statement" => facts.exceptions.push(ExceptionFact {
            kind: ExceptionKind::Try,
            range: node_range(node, lines),
        }),
        "throw_statement" => facts.exceptions.push(ExceptionFact {
            kind: ExceptionKind::Throw,
            range: node_range(node, lines),
        }),
        "call_expression" => {
            if let Some(function) = node.child_by_field_name("function") {
                let callee = semantic_text(function, source);
                facts.calls.push(CallFact {
                    callee: callee.clone(),
                    range: node_range(node, lines),
                });
                if callee == "Promise.reject" {
                    facts.exceptions.push(ExceptionFact {
                        kind: ExceptionKind::PromiseReject,
                        range: node_range(node, lines),
                    });
                }
            }
        }
        "new_expression" => {
            collect_promise_executor_rejections(node, source, lines, facts);
        }
        _ => {}
    }

    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            collect_facts(child, source, lines, facts);
        }
    }
}

fn function_fact(node: Node<'_>, source: &str, lines: &LineIndex<'_>) -> FunctionFact {
    let name = node
        .child_by_field_name("name")
        .and_then(|name| name.utf8_text(source.as_bytes()).ok())
        .unwrap_or("<anonymous>")
        .to_string();
    let return_type_names = node
        .child_by_field_name("return_type")
        .map(|return_type| {
            let mut names = Vec::new();
            collect_type_names(return_type, source, &mut names);
            names
        })
        .unwrap_or_default();
    let body = node.child_by_field_name("body");
    let all_paths_return_value = body.is_some_and(|body| {
        if body.kind() == "statement_block" {
            block_flow(body).all_paths_return_value()
        } else {
            true
        }
    });
    let mut bare_returns = Vec::new();
    if let Some(body) = body {
        collect_bare_returns(body, lines, &mut bare_returns);
    }
    FunctionFact {
        name,
        range: node_range(node, lines),
        return_type_names,
        all_paths_return_value,
        bare_returns,
    }
}

fn collect_type_names(node: Node<'_>, source: &str, names: &mut Vec<String>) {
    if matches!(node.kind(), "type_identifier" | "identifier")
        && let Ok(value) = node.utf8_text(source.as_bytes())
    {
        names.push(value.to_string());
    }
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            collect_type_names(child, source, names);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct FlowOutcome {
    falls_through: bool,
    exits_without_value: bool,
}

impl FlowOutcome {
    const RETURN_VALUE: Self = Self {
        falls_through: false,
        exits_without_value: false,
    };
    const FALLTHROUGH: Self = Self {
        falls_through: true,
        exits_without_value: false,
    };
    const INVALID_EXIT: Self = Self {
        falls_through: false,
        exits_without_value: true,
    };

    fn all_paths_return_value(self) -> bool {
        !self.falls_through && !self.exits_without_value
    }
}

fn block_flow(block: Node<'_>) -> FlowOutcome {
    let mut outcome = FlowOutcome::FALLTHROUGH;
    let mut cursor = block.walk();
    for child in block.named_children(&mut cursor) {
        if !outcome.falls_through {
            break;
        }
        let statement = statement_flow(child);
        outcome.falls_through = statement.falls_through;
        outcome.exits_without_value |= statement.exits_without_value;
    }
    outcome
}

fn statement_flow(node: Node<'_>) -> FlowOutcome {
    match node.kind() {
        "return_statement" if node.named_child_count() > 0 => FlowOutcome::RETURN_VALUE,
        "return_statement" | "throw_statement" => FlowOutcome::INVALID_EXIT,
        "statement_block" => block_flow(node),
        "if_statement" => {
            let consequence = node
                .child_by_field_name("consequence")
                .map(statement_flow)
                .unwrap_or(FlowOutcome::FALLTHROUGH);
            let alternative = node
                .child_by_field_name("alternative")
                .map(unwrap_else_clause)
                .map(statement_flow)
                .unwrap_or(FlowOutcome::FALLTHROUGH);
            merge_branches(consequence, alternative)
        }
        "labeled_statement" => node
            .child_by_field_name("body")
            .map(statement_flow)
            .unwrap_or(FlowOutcome::FALLTHROUGH),
        "switch_statement" => switch_flow(node),
        "try_statement" => try_flow(node),
        "while_statement" | "do_statement" | "for_statement" | "for_in_statement" => node
            .child_by_field_name("body")
            .map(statement_flow)
            .map_or(FlowOutcome::FALLTHROUGH, |body| FlowOutcome {
                falls_through: true,
                exits_without_value: body.exits_without_value,
            }),
        _ => FlowOutcome::FALLTHROUGH,
    }
}

fn unwrap_else_clause(node: Node<'_>) -> Node<'_> {
    if node.kind() != "else_clause" {
        return node;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next().unwrap_or(node)
}

fn merge_branches(left: FlowOutcome, right: FlowOutcome) -> FlowOutcome {
    FlowOutcome {
        falls_through: left.falls_through || right.falls_through,
        exits_without_value: left.exits_without_value || right.exits_without_value,
    }
}

fn try_flow(node: Node<'_>) -> FlowOutcome {
    let mut cursor = node.walk();
    let children = node.named_children(&mut cursor).collect::<Vec<_>>();
    let Some(try_body) = children
        .iter()
        .find(|child| child.kind() == "statement_block")
    else {
        return FlowOutcome::FALLTHROUGH;
    };
    let finally_flow = children
        .iter()
        .find(|child| child.kind() == "finally_clause")
        .and_then(|finally| {
            let mut cursor = finally.walk();
            finally
                .named_children(&mut cursor)
                .find(|child| child.kind() == "statement_block")
        })
        .map(block_flow);
    if let Some(finally) = finally_flow
        && !finally.falls_through
    {
        return finally;
    }
    let catch_flow = children
        .iter()
        .find(|child| child.kind() == "catch_clause")
        .and_then(|catch| {
            let mut cursor = catch.walk();
            catch
                .named_children(&mut cursor)
                .find(|child| child.kind() == "statement_block")
        })
        .map(block_flow);
    let mut outcome = if let Some(catch) = catch_flow {
        merge_branches(block_flow(*try_body), catch)
    } else {
        block_flow(*try_body)
    };
    if let Some(finally) = finally_flow {
        outcome.exits_without_value |= finally.exits_without_value;
    }
    outcome
}

fn switch_flow(node: Node<'_>) -> FlowOutcome {
    let mut node_cursor = node.walk();
    let Some(body) = node
        .named_children(&mut node_cursor)
        .find(|child| child.kind() == "switch_body")
    else {
        return FlowOutcome::FALLTHROUGH;
    };
    let mut has_default = false;
    let mut has_case = false;
    let mut outcome = FlowOutcome::RETURN_VALUE;
    let mut body_cursor = body.walk();
    for case in body.named_children(&mut body_cursor) {
        if !matches!(case.kind(), "switch_case" | "switch_default") {
            continue;
        }
        has_case = true;
        has_default |= case.kind() == "switch_default";
        let case_flow = case_flow(case);
        outcome = merge_branches(outcome, case_flow);
    }
    if !has_case || !has_default {
        outcome.falls_through = true;
    }
    outcome
}

fn case_flow(case: Node<'_>) -> FlowOutcome {
    let mut outcome = FlowOutcome::FALLTHROUGH;
    let mut cursor = case.walk();
    for (index, child) in case.named_children(&mut cursor).enumerate() {
        if case.kind() == "switch_case" && index == 0 {
            continue;
        }
        if !outcome.falls_through {
            break;
        }
        let statement = statement_flow(child);
        outcome.falls_through = statement.falls_through;
        outcome.exits_without_value |= statement.exits_without_value;
    }
    outcome
}

fn collect_bare_returns(node: Node<'_>, lines: &LineIndex<'_>, ranges: &mut Vec<TextRange>) {
    if node.kind() == "return_statement" && node.named_child_count() == 0 {
        ranges.push(node_range(node, lines));
        return;
    }
    if is_function_like(node.kind()) {
        return;
    }
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            collect_bare_returns(child, lines, ranges);
        }
    }
}

fn collect_promise_executor_rejections(
    node: Node<'_>,
    source: &str,
    lines: &LineIndex<'_>,
    facts: &mut SemanticFacts,
) {
    let Some(constructor) = node.child_by_field_name("constructor") else {
        return;
    };
    if semantic_text(constructor, source) != "Promise" {
        return;
    }
    let Some(arguments) = node.child_by_field_name("arguments") else {
        return;
    };
    let mut arguments_cursor = arguments.walk();
    let Some(executor) = arguments
        .named_children(&mut arguments_cursor)
        .find(|child| matches!(child.kind(), "arrow_function" | "function_expression"))
    else {
        return;
    };
    let Some(parameters) = executor.child_by_field_name("parameters") else {
        return;
    };
    let mut parameters_cursor = parameters.walk();
    let parameter_names = parameters
        .named_children(&mut parameters_cursor)
        .filter(|child| child.kind() == "required_parameter" || child.kind() == "identifier")
        .filter_map(|parameter| {
            if parameter.kind() == "identifier" {
                parameter.utf8_text(source.as_bytes()).ok()
            } else {
                parameter
                    .child_by_field_name("pattern")
                    .and_then(|pattern| pattern.utf8_text(source.as_bytes()).ok())
            }
        })
        .collect::<Vec<_>>();
    let Some(reject_name) = parameter_names.get(1) else {
        return;
    };
    let Some(body) = executor.child_by_field_name("body") else {
        return;
    };
    collect_named_calls(body, source, lines, reject_name, &mut facts.exceptions);
}

fn collect_named_calls(
    node: Node<'_>,
    source: &str,
    lines: &LineIndex<'_>,
    name: &str,
    exceptions: &mut Vec<ExceptionFact>,
) {
    if node.kind() == "call_expression"
        && node
            .child_by_field_name("function")
            .is_some_and(|function| semantic_text(function, source) == name)
    {
        exceptions.push(ExceptionFact {
            kind: ExceptionKind::PromiseReject,
            range: node_range(node, lines),
        });
    }
    if is_function_like(node.kind()) && function_binds_name(node, source, name) {
        return;
    }
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            collect_named_calls(child, source, lines, name, exceptions);
        }
    }
}

fn function_binds_name(node: Node<'_>, source: &str, name: &str) -> bool {
    node.child_by_field_name("parameters")
        .is_some_and(|parameters| {
            let mut cursor = parameters.walk();
            parameters.named_children(&mut cursor).any(|parameter| {
                let pattern = if parameter.kind() == "required_parameter" {
                    parameter.child_by_field_name("pattern")
                } else {
                    Some(parameter)
                };
                pattern
                    .and_then(|pattern| pattern.utf8_text(source.as_bytes()).ok())
                    .is_some_and(|value| value == name)
            })
        })
}

pub(crate) fn semantic_text(node: Node<'_>, source: &str) -> String {
    fn append(node: Node<'_>, source: &str, output: &mut String) {
        if node.kind() == "comment" {
            return;
        }
        if node.child_count() == 0 {
            if let Ok(value) = node.utf8_text(source.as_bytes()) {
                output.extend(value.chars().filter(|character| !character.is_whitespace()));
            }
            return;
        }
        for index in 0..node.child_count() {
            if let Some(child) = node.child(index) {
                append(child, source, output);
            }
        }
    }

    let mut output = String::new();
    append(node, source, &mut output);
    output
}

fn node_range(node: Node<'_>, lines: &LineIndex<'_>) -> TextRange {
    lines.range(node.start_byte(), node.end_byte())
}

fn is_function_like(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "function_expression"
            | "arrow_function"
            | "method_definition"
            | "generator_function"
            | "generator_function_declaration"
    )
}
