mod generated;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

use ctc_core::{
    diagnostic::{Diagnostic, DiagnosticCategory, LineIndex, TextRange},
    matcher::MatchMode,
    template::{CaptureCategory, Cardinality, CompiledTemplate, Filter, Placeholder, TemplateNode},
};
use tree_sitter::Node;

use crate::{
    canonicalize::{
        SyntaxChild, canonical_kind, canonicalize_node, error_nodes, parse_tree, syntax_children,
        syntax_errors,
    },
    fields::remove_source_only_fields,
};
use generated::{
    GeneratedSource, Sentinel, WrapperKind, error_score, find_exact_node, generate_source,
    line_isolated, parse_generated,
};

pub fn compile_template(
    source: &str,
    path: &Path,
    placeholders: &[Placeholder],
    mode: MatchMode,
) -> Result<CompiledTemplate, Vec<Diagnostic>> {
    let placeholders = resolve_capture_categories(placeholders)?;
    let (generated, tree) = generate_parsable_source(source, &placeholders)?;
    validate_generated_syntax(path, source, &generated, &tree)?;
    let targets = resolve_targets(&generated, &tree)?;
    let context = ConvertContext {
        generated_source: &generated.source,
        path,
        lines: LineIndex::new(path, source),
        offset_map: &generated.offset_map,
        original_length: source.len(),
        targets: &targets,
    };
    let Some(root) = convert_template_node(tree.root_node(), &context) else {
        return Err(vec![Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            "The Lua adapter removed the template root.",
            2,
        )]);
    };
    ctc_core::matcher::validate_adjacent_sequences(&root)?;
    if mode == MatchMode::Every {
        ctc_core::matcher::validate_every_template(&root)?;
    }
    if matches!(mode, MatchMode::Contains | MatchMode::Forbid) && can_match_empty_top_level(&root) {
        return Err(vec![
            Diagnostic::new(
                "CTC2010",
                DiagnosticCategory::EmptySearchTemplate,
                "A contains or forbid template must consume a source item.",
                2,
            )
            .with_template(TextRange::from_offsets(path, source, 0, source.len())),
        ]);
    }
    Ok(CompiledTemplate {
        path: path.to_string_lossy().replace('\\', "/"),
        language_id: "lua",
        mode,
        root,
    })
}

fn generate_parsable_source(
    source: &str,
    placeholders: &[Placeholder],
) -> Result<(GeneratedSource, tree_sitter::Tree), Vec<Diagnostic>> {
    let mut wrappers = vec![WrapperKind::None; placeholders.len()];
    loop {
        let generated = generate_source(source, placeholders, &wrappers);
        let tree = parse_generated(&generated.source)?;
        let current_score = error_score(&error_nodes(tree.root_node()));
        if current_score == (0, 0) {
            return Ok((generated, tree));
        }
        let Some(index) = best_wrapper_choice(source, placeholders, &wrappers, current_score)?
        else {
            return Ok((generated, tree));
        };
        wrappers[index] = WrapperKind::Statement;
    }
}

/// The placeholder whose statement wrapper removes the most syntax errors.
fn best_wrapper_choice(
    source: &str,
    placeholders: &[Placeholder],
    wrappers: &[WrapperKind],
    current_score: (usize, usize),
) -> Result<Option<usize>, Vec<Diagnostic>> {
    let mut best: Option<(usize, (usize, usize))> = None;
    for (index, placeholder) in placeholders.iter().enumerate() {
        if placeholder.category == CaptureCategory::Keyword
            || wrappers[index] == WrapperKind::Statement
            || !(placeholder.cardinality == Cardinality::Sequence
                || line_isolated(source, placeholder))
        {
            continue;
        }
        let mut trial = wrappers.to_vec();
        trial[index] = WrapperKind::Statement;
        let generated = generate_source(source, placeholders, &trial);
        let tree = parse_generated(&generated.source)?;
        let Some(sentinel) = generated.sentinels.get(index) else {
            continue;
        };
        let Some(leaf) = find_exact_node(
            tree.root_node(),
            sentinel.generated_range.start,
            sentinel.generated_range.end,
        ) else {
            continue;
        };
        if select_target(leaf, &generated.source, sentinel).is_err() {
            continue;
        }
        let score = error_score(&error_nodes(tree.root_node()));
        if score < best.map_or(current_score, |(_, best_score)| best_score) {
            best = Some((index, score));
        }
    }
    Ok(best.map(|(index, _)| index))
}

fn validate_generated_syntax(
    path: &Path,
    source: &str,
    generated: &GeneratedSource,
    tree: &tree_sitter::Tree,
) -> Result<(), Vec<Diagnostic>> {
    let errors = syntax_errors(tree.root_node(), &generated.source);
    if errors.is_empty() {
        return Ok(());
    }
    Err(errors
        .into_iter()
        .map(|(range, message)| {
            let message = message.replace("The source contains", "The generated template contains");
            Diagnostic::new(
                "CTC2001",
                DiagnosticCategory::InvalidTemplateSyntax,
                message,
                2,
            )
            .with_template(generated.map_range(path, source, range.start, range.end))
        })
        .collect())
}

fn resolve_targets(
    generated: &GeneratedSource,
    tree: &tree_sitter::Tree,
) -> Result<BTreeMap<usize, Placeholder>, Vec<Diagnostic>> {
    let mut targets = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for sentinel in &generated.sentinels {
        let unsupported = |message: String| {
            Diagnostic::new(
                "CTC2002",
                DiagnosticCategory::UnsupportedPlaceholderContext,
                message,
                2,
            )
            .with_template(sentinel.placeholder.range.clone())
        };
        let Some(leaf) = find_exact_node(
            tree.root_node(),
            sentinel.generated_range.start,
            sentinel.generated_range.end,
        ) else {
            diagnostics.push(unsupported(
                "The parser could not locate the generated placeholder sentinel.".to_string(),
            ));
            continue;
        };
        match select_target(leaf, &generated.source, sentinel) {
            Ok(target) => {
                if targets
                    .insert(target.id(), sentinel.placeholder.clone())
                    .is_some()
                {
                    diagnostics.push(unsupported(
                        "Two placeholders resolve to the same syntax node.".to_string(),
                    ));
                }
            }
            Err(message) => diagnostics.push(unsupported(message)),
        }
    }
    if diagnostics.is_empty() {
        Ok(targets)
    } else {
        Err(diagnostics)
    }
}

fn select_target<'a>(
    leaf: Node<'a>,
    generated_source: &str,
    sentinel: &Sentinel,
) -> Result<Node<'a>, String> {
    if sentinel.placeholder.category == CaptureCategory::Keyword {
        return keyword_target(leaf, generated_source, sentinel);
    }
    if leaf.kind() != "identifier" {
        return Err("A placeholder sentinel did not parse as an identifier.".to_string());
    }
    validate_explicit_category(leaf, sentinel.placeholder.category)?;
    if sentinel.wrapper == WrapperKind::Statement {
        return statement_target(leaf);
    }
    if sentinel.placeholder.cardinality == Cardinality::Scalar
        && sentinel.placeholder.category != CaptureCategory::Inferred
    {
        return Ok(leaf);
    }
    let Some(item) = list_item_target(leaf) else {
        return if sentinel.placeholder.cardinality == Cardinality::Sequence {
            Err("A sequence placeholder must occur in a supported Lua syntax list.".to_string())
        } else {
            Ok(leaf)
        };
    };
    let occupies_item = compact_text(item, generated_source) == sentinel.text;
    match (sentinel.placeholder.cardinality, occupies_item) {
        (Cardinality::Sequence, false) => {
            Err("A sequence placeholder must occupy one complete syntax-list item.".to_string())
        }
        (_, true) => Ok(item),
        (_, false) => Ok(leaf),
    }
}

fn keyword_target<'a>(
    leaf: Node<'a>,
    generated_source: &str,
    sentinel: &Sentinel,
) -> Result<Node<'a>, String> {
    if compact_text(leaf, generated_source) == sentinel.placeholder.name {
        return Ok(leaf);
    }
    Err(format!(
        "Keyword placeholder `{}` did not parse as that keyword.",
        sentinel.placeholder.name
    ))
}

/// The statement that a `Statement` wrapper made of the sentinel.
fn statement_target(leaf: Node<'_>) -> Result<Node<'_>, String> {
    let mut current = leaf;
    loop {
        if current
            .parent()
            .is_some_and(|parent| matches!(parent.kind(), "chunk" | "block"))
        {
            return Ok(current);
        }
        current = current
            .parent()
            .ok_or_else(|| "A placeholder did not parse as a Lua statement.".to_string())?;
    }
}

fn list_item_target(leaf: Node<'_>) -> Option<Node<'_>> {
    let mut current = leaf;
    while let Some(parent) = current.parent() {
        if is_list_container(parent.kind()) {
            return Some(current);
        }
        current = parent;
    }
    None
}

fn is_list_container(kind: &str) -> bool {
    matches!(
        kind,
        "arguments"
            | "block"
            | "chunk"
            | "expression_list"
            | "parameters"
            | "table_constructor"
            | "variable_list"
    )
}

fn validate_explicit_category(leaf: Node<'_>, category: CaptureCategory) -> Result<(), String> {
    match category {
        CaptureCategory::Inferred | CaptureCategory::Identifier => Ok(()),
        CaptureCategory::Expression if !is_name_slot(leaf) => Ok(()),
        CaptureCategory::Expression => {
            Err("An expression placeholder occurs in a name slot.".to_string())
        }
        CaptureCategory::Type => Err(
            "A type placeholder does not occur in a type slot. Lua has no type slots.".to_string(),
        ),
        CaptureCategory::Keyword => {
            Err("A keyword placeholder did not parse as a keyword.".to_string())
        }
    }
}

/// True when the identifier names something instead of being an expression:
/// a declared variable, a parameter, a field or method name, a label, or an
/// attribute.
fn is_name_slot(leaf: Node<'_>) -> bool {
    let Some(parent) = leaf.parent() else {
        return false;
    };
    match parent.kind() {
        "attribute"
        | "function_declaration"
        | "goto_statement"
        | "label_statement"
        | "parameters" => true,
        "dot_index_expression" | "method_index_expression" => parent
            .child_by_field_name("table")
            .is_none_or(|table| table.id() != leaf.id()),
        "for_numeric_clause" => parent
            .child_by_field_name("name")
            .is_some_and(|name| name.id() == leaf.id()),
        "field" => parent.child(0).is_some_and(|first| first.id() == leaf.id()),
        "variable_list" => declares_names(parent),
        _ => false,
    }
}

fn declares_names(list: Node<'_>) -> bool {
    match list.parent() {
        Some(parent) if matches!(parent.kind(), "variable_declaration" | "for_generic_clause") => {
            true
        }
        Some(parent) if parent.kind() == "assignment_statement" => parent
            .parent()
            .is_some_and(|grandparent| grandparent.kind() == "variable_declaration"),
        _ => false,
    }
}

fn resolve_capture_categories(
    placeholders: &[Placeholder],
) -> Result<Vec<Placeholder>, Vec<Diagnostic>> {
    let mut placeholders = placeholders.to_vec();
    let mut explicit_categories = BTreeMap::new();
    let mut derived_names = BTreeSet::new();
    for placeholder in &placeholders {
        if placeholder.category == CaptureCategory::Keyword {
            continue;
        }
        if has_name_filter(placeholder) {
            derived_names.insert(placeholder.name.clone());
        } else if placeholder.category != CaptureCategory::Inferred {
            explicit_categories.insert(placeholder.name.clone(), placeholder.category);
        }
    }
    for placeholder in &mut placeholders {
        if placeholder.category == CaptureCategory::Keyword {
            continue;
        }
        if has_name_filter(placeholder) {
            placeholder.category = CaptureCategory::Identifier;
        } else if placeholder.category == CaptureCategory::Inferred {
            if let Some(category) = explicit_categories.get(&placeholder.name) {
                placeholder.category = *category;
            } else if derived_names.contains(&placeholder.name) {
                placeholder.category = CaptureCategory::Identifier;
            }
        }
    }
    let diagnostics = derived_names
        .iter()
        .flat_map(|derived_name| {
            placeholders.iter().filter(move |placeholder| {
                &placeholder.name == derived_name
                    && !has_name_filter(placeholder)
                    && placeholder.category != CaptureCategory::Identifier
            })
        })
        .map(|placeholder| {
            Diagnostic::new(
                "CTC2008",
                DiagnosticCategory::InvalidFilter,
                format!(
                    "Naming filters require identifier capture `{}`.",
                    placeholder.name
                ),
                2,
            )
            .with_template(placeholder.range.clone())
        })
        .collect::<Vec<_>>();
    if diagnostics.is_empty() {
        Ok(placeholders)
    } else {
        Err(diagnostics)
    }
}

fn has_name_filter(placeholder: &Placeholder) -> bool {
    placeholder.filters.iter().any(Filter::is_name_filter)
}

struct ConvertContext<'a> {
    generated_source: &'a str,
    path: &'a Path,
    lines: LineIndex<'a>,
    offset_map: &'a [usize],
    original_length: usize,
    targets: &'a BTreeMap<usize, Placeholder>,
}

impl ConvertContext<'_> {
    fn original(&self, generated_offset: usize) -> usize {
        self.offset_map
            .get(generated_offset)
            .copied()
            .unwrap_or(self.original_length)
    }

    fn range(&self, start: usize, end: usize) -> TextRange {
        self.lines.range(self.original(start), self.original(end))
    }
}

fn convert_template_node(node: Node<'_>, context: &ConvertContext<'_>) -> Option<TemplateNode> {
    if let Some(placeholder) = context.targets.get(&node.id()) {
        return placeholder_node(node, context, placeholder);
    }
    let canonical = canonicalize_node(node, context.generated_source, context.path)?;
    let is_value_node = canonical.value.is_some() && canonical.children.is_empty();
    let mut children = Vec::new();
    for child in if is_value_node {
        Vec::new()
    } else {
        syntax_children(node)
    } {
        match child {
            SyntaxChild::Node(raw_child) => {
                children.extend(convert_template_node(raw_child, context));
            }
            SyntaxChild::EmptyBlock(offset) => children.push(TemplateNode::Literal {
                kind: Arc::from("StatementBlock"),
                value: None,
                fields: BTreeMap::new(),
                children: Vec::new(),
                range: context.range(offset, offset),
            }),
        }
    }
    let mut fields = canonical.fields;
    remove_source_only_fields(&mut fields);
    Some(TemplateNode::Literal {
        kind: Arc::from(canonical_kind(node)),
        value: canonical.value,
        fields,
        children,
        range: context.range(node.start_byte(), node.end_byte()),
    })
}

fn placeholder_node(
    node: Node<'_>,
    context: &ConvertContext<'_>,
    placeholder: &Placeholder,
) -> Option<TemplateNode> {
    Some(match placeholder.cardinality {
        Cardinality::Optional => {
            let inner = if placeholder.category == CaptureCategory::Keyword {
                let canonical = canonicalize_node(node, context.generated_source, context.path)?;
                literal_from_canonical(canonical, &placeholder.range)
            } else {
                TemplateNode::Capture {
                    placeholder: placeholder.clone(),
                }
            };
            TemplateNode::Optional {
                node: Box::new(inner),
                range: placeholder.range.clone(),
            }
        }
        Cardinality::Sequence => TemplateNode::Sequence {
            placeholder: placeholder.clone(),
        },
        Cardinality::Scalar if placeholder.is_derived() => TemplateNode::Derived {
            placeholder: placeholder.clone(),
        },
        Cardinality::Scalar => TemplateNode::Capture {
            placeholder: placeholder.clone(),
        },
    })
}

fn literal_from_canonical(
    node: ctc_core::canonical::CanonicalNode,
    range: &TextRange,
) -> TemplateNode {
    let mut fields = node.fields;
    remove_source_only_fields(&mut fields);
    TemplateNode::Literal {
        kind: node.kind,
        value: node.value,
        fields,
        children: node
            .children
            .into_iter()
            .map(|child| literal_from_canonical(child, range))
            .collect(),
        range: range.clone(),
    }
}

/// The text of `node` without white space, comments, or list separators.
fn compact_text(node: Node<'_>, source: &str) -> String {
    crate::fields::compact_text(node, source).replace([',', ';'], "")
}

fn can_match_empty_top_level(root: &TemplateNode) -> bool {
    let TemplateNode::Literal { children, .. } = root else {
        return false;
    };
    children.is_empty()
        || children.iter().all(|child| {
            matches!(
                child,
                TemplateNode::Optional { .. } | TemplateNode::Sequence { .. }
            )
        })
}

#[cfg(test)]
mod tests;
