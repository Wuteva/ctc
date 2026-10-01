//! Finds member function declarations that need an out-of-line definition,
//! and member function definitions outside a class. The core compares these
//! facts across a header and its source file.

use ctc_core::{
    canonical::{MemberFunctionFact, MemberFunctionRole},
    diagnostic::{LineIndex, TextRange},
};
use tree_sitter::Node;

struct Context<'a> {
    source: &'a str,
    lines: &'a LineIndex<'a>,
    facts: Vec<MemberFunctionFact>,
}

pub(crate) fn member_function_facts(
    root: Node<'_>,
    source: &str,
    lines: &LineIndex<'_>,
) -> Vec<MemberFunctionFact> {
    let mut context = Context {
        source,
        lines,
        facts: Vec::new(),
    };
    walk_scope(root, &mut Vec::new(), &mut context);
    context.facts
}

fn named_children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    (0..node.named_child_count() as u32).filter_map(move |index| node.named_child(index))
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or_default()
}

fn walk_scope(node: Node<'_>, scope: &mut Vec<String>, context: &mut Context<'_>) {
    for child in named_children(node) {
        match child.kind() {
            "namespace_definition" => {
                let segments = child
                    .child_by_field_name("name")
                    .map(|name| namespace_segments(name, context.source))
                    .unwrap_or_default();
                let depth = scope.len();
                scope.extend(segments);
                if let Some(body) = child.child_by_field_name("body") {
                    walk_scope(body, scope, context);
                }
                scope.truncate(depth);
            }
            "linkage_specification" => {
                if let Some(body) = child.child_by_field_name("body") {
                    if body.kind() == "declaration_list" {
                        walk_scope(body, scope, context);
                    } else {
                        walk_scope(child, scope, context);
                    }
                }
            }
            "class_specifier" | "struct_specifier" | "union_specifier" => {
                collect_class(child, scope, context);
            }
            "declaration" => {
                if let Some(class) = child.child_by_field_name("type") {
                    collect_class(class, scope, context);
                }
            }
            "function_definition" => collect_definition(child, scope, context),
            kind if kind.starts_with("preproc_") => walk_scope(child, scope, context),
            _ => {}
        }
    }
}

fn namespace_segments(name: Node<'_>, source: &str) -> Vec<String> {
    text(name, source)
        .split("::")
        .map(|segment| segment.trim())
        .filter(|segment| !segment.is_empty() && *segment != "inline")
        .map(str::to_string)
        .collect()
}

fn collect_class(class: Node<'_>, scope: &[String], context: &mut Context<'_>) {
    if !matches!(
        class.kind(),
        "class_specifier" | "struct_specifier" | "union_specifier"
    ) {
        return;
    }
    // Anonymous classes, class template specializations, and qualified class
    // names are skipped.
    let Some(name) = class
        .child_by_field_name("name")
        .filter(|name| name.kind() == "type_identifier")
    else {
        return;
    };
    let Some(body) = class.child_by_field_name("body") else {
        return;
    };
    let mut owner = scope.to_vec();
    owner.push(text(name, context.source).to_string());
    collect_members(body, &owner, context);
}

fn collect_members(body: Node<'_>, owner: &[String], context: &mut Context<'_>) {
    for member in named_children(body) {
        match member.kind() {
            "field_declaration" | "declaration" => {
                if let Some(class) = member.child_by_field_name("type") {
                    collect_class(class, owner, context);
                }
                collect_declaration(member, owner, context);
            }
            kind if kind.starts_with("preproc_") => collect_members(member, owner, context),
            _ => {}
        }
    }
}

fn collect_declaration(member: Node<'_>, owner: &[String], context: &mut Context<'_>) {
    // A pure virtual function has `= 0` as its default value.
    if member.child_by_field_name("default_value").is_some() {
        return;
    }
    let defined_in_header = named_children(member).any(|child| {
        matches!(child.kind(), "storage_class_specifier" | "type_qualifier")
            && matches!(
                text(child, context.source).trim(),
                "inline" | "constexpr" | "consteval"
            )
    });
    if defined_in_header {
        return;
    }
    let mut cursor = member.walk();
    let declarators = member
        .children_by_field_name("declarator", &mut cursor)
        .collect::<Vec<_>>();
    for declarator in declarators {
        let Some(function) = function_declarator(declarator) else {
            continue;
        };
        let Some(name) = function
            .child_by_field_name("declarator")
            .filter(|name| is_function_name(name.kind()))
        else {
            continue;
        };
        context.facts.push(MemberFunctionFact {
            role: MemberFunctionRole::Declaration,
            owner: owner.to_vec(),
            name: normalize_tokens(name, context.source),
            signature: signature(function, context.source),
            range: range(member, context),
        });
    }
}

fn collect_definition(definition: Node<'_>, scope: &[String], context: &mut Context<'_>) {
    let Some(function) = definition
        .child_by_field_name("declarator")
        .and_then(function_declarator)
    else {
        return;
    };
    let Some(qualified) = function
        .child_by_field_name("declarator")
        .filter(|name| name.kind() == "qualified_identifier")
    else {
        return;
    };
    let mut owner = scope.to_vec();
    let mut current = qualified;
    let name = loop {
        let Some(qualifier) = current
            .child_by_field_name("scope")
            .filter(|scope| matches!(scope.kind(), "namespace_identifier" | "type_identifier"))
        else {
            return;
        };
        owner.push(text(qualifier, context.source).to_string());
        let Some(name) = current.child_by_field_name("name") else {
            return;
        };
        if name.kind() == "qualified_identifier" {
            current = name;
        } else if is_function_name(name.kind()) {
            break name;
        } else {
            return;
        }
    };
    context.facts.push(MemberFunctionFact {
        role: MemberFunctionRole::Definition,
        owner,
        name: normalize_tokens(name, context.source),
        signature: signature(function, context.source),
        range: range(definition, context),
    });
}

fn is_function_name(kind: &str) -> bool {
    matches!(
        kind,
        "identifier" | "field_identifier" | "destructor_name" | "operator_name"
    )
}

/// Looks through pointer and reference declarators, such as `int* f()` and
/// `int& f()`, to the function declarator.
fn function_declarator(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = node;
    loop {
        match current.kind() {
            "function_declarator" => return Some(current),
            "pointer_declarator" => current = current.child_by_field_name("declarator")?,
            "reference_declarator" => current = named_children(current).last()?,
            _ => return None,
        }
    }
}

fn signature(function: Node<'_>, source: &str) -> String {
    let mut parameters = Vec::new();
    if let Some(list) = function.child_by_field_name("parameters") {
        let mut cursor = list.walk();
        for child in list.children(&mut cursor) {
            match child.kind() {
                "parameter_declaration"
                | "optional_parameter_declaration"
                | "variadic_parameter_declaration" => {
                    parameters.push(parameter_type(child, source))
                }
                "..." => parameters.push("...".to_string()),
                _ => {}
            }
        }
    }
    if parameters.len() == 1 && parameters[0] == "void" {
        parameters.clear();
    }
    let mut signature = format!("({})", parameters.join(", "));
    for child in named_children(function) {
        if matches!(child.kind(), "type_qualifier" | "ref_qualifier") {
            signature.push(' ');
            signature.push_str(text(child, source).trim());
        }
    }
    signature
}

/// Returns the parameter type as written, without the parameter name, the
/// default value, or a top-level `const` or `volatile`. C++ ignores that
/// top-level qualifier in a function type.
fn parameter_type(parameter: Node<'_>, source: &str) -> String {
    let declarator = parameter.child_by_field_name("declarator");
    let mut skip = declarator
        .and_then(parameter_name)
        .into_iter()
        .collect::<Vec<_>>();
    // In `int* const p`, the `const` belongs to the pointer itself, so it is
    // a top-level qualifier too.
    if let Some(pointer) = declarator.filter(|declarator| declarator.kind() == "pointer_declarator")
    {
        skip.extend(named_children(pointer).filter(|child| is_cv_qualifier(*child, source)));
    }
    let by_value = declarator.is_none_or(|declarator| declarator.kind() == "identifier");
    let default_value = parameter.child_by_field_name("default_value");
    let mut tokens = Vec::new();
    let mut cursor = parameter.walk();
    for child in parameter.children(&mut cursor) {
        if Some(child) == default_value || (child.kind() == "=" && !child.is_named()) {
            continue;
        }
        if by_value && is_cv_qualifier(child, source) {
            continue;
        }
        collect_tokens(child, &skip, source, &mut tokens);
    }
    join_tokens(&tokens)
}

fn parameter_name(declarator: Node<'_>) -> Option<Node<'_>> {
    let mut current = declarator;
    loop {
        match current.kind() {
            "identifier" => return Some(current),
            "pointer_declarator" | "array_declarator" | "function_declarator" => {
                current = current.child_by_field_name("declarator")?;
            }
            "reference_declarator" | "parenthesized_declarator" => {
                current = named_children(current).last()?;
            }
            _ => return None,
        }
    }
}

fn is_cv_qualifier(node: Node<'_>, source: &str) -> bool {
    node.kind() == "type_qualifier" && matches!(text(node, source).trim(), "const" | "volatile")
}

fn collect_tokens<'a>(
    node: Node<'_>,
    skip: &[Node<'_>],
    source: &'a str,
    tokens: &mut Vec<&'a str>,
) {
    if skip.contains(&node) || node.kind() == "comment" {
        return;
    }
    if node.child_count() == 0 {
        let value = text(node, source).trim();
        if !value.is_empty() {
            tokens.push(value);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tokens(child, skip, source, tokens);
    }
}

fn normalize_tokens(node: Node<'_>, source: &str) -> String {
    let mut tokens = Vec::new();
    collect_tokens(node, &[], source, &mut tokens);
    join_tokens(&tokens)
}

/// Joins tokens with one space, then removes each space that sits next to a
/// punctuation character, so `std :: string &` becomes `std::string&`.
fn join_tokens(tokens: &[&str]) -> String {
    let joined = tokens.join(" ");
    let characters = joined.chars().collect::<Vec<_>>();
    let is_word = |character: char| character.is_alphanumeric() || character == '_';
    let mut result = String::with_capacity(joined.len());
    for (index, character) in characters.iter().enumerate() {
        if *character == ' ' {
            let before = index.checked_sub(1).map(|index| characters[index]);
            let after = characters.get(index + 1).copied();
            if !before.is_some_and(is_word) || !after.is_some_and(is_word) {
                continue;
            }
        }
        result.push(*character);
    }
    result
}

fn range(node: Node<'_>, context: &Context<'_>) -> TextRange {
    context.lines.range(node.start_byte(), node.end_byte())
}

#[cfg(test)]
mod tests;
