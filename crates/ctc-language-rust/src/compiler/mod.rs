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
    canonicalize::{canonical_kind, canonicalize_node, error_nodes, parse_tree},
    members::remove_source_only_fields,
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
    let lines = LineIndex::new(path, source);
    let Some(root) = convert_template_node(
        tree.root_node(),
        &generated.source,
        source,
        path,
        &lines,
        &generated.offset_map,
        &targets,
    ) else {
        return Err(vec![Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            "The Rust adapter removed the template root.",
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
        language_id: "rust",
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
        let Some((index, wrapper)) =
            best_wrapper_choice(source, placeholders, &wrappers, current_score)?
        else {
            return Ok((generated, tree));
        };
        wrappers[index] = wrapper;
    }
}

fn best_wrapper_choice(
    source: &str,
    placeholders: &[Placeholder],
    wrappers: &[WrapperKind],
    current_score: (usize, usize),
) -> Result<Option<(usize, WrapperKind)>, Vec<Diagnostic>> {
    let mut best = None;
    for (index, placeholder) in placeholders.iter().enumerate() {
        if placeholder.category == CaptureCategory::Keyword {
            continue;
        }
        for wrapper in wrapper_candidates(source, placeholder) {
            if wrappers[index] == wrapper {
                continue;
            }
            let mut trial = wrappers.to_vec();
            trial[index] = wrapper;
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
            if score < best.map_or(current_score, |(_, _, best_score)| best_score) {
                best = Some((index, wrapper, score));
            }
        }
    }
    Ok(best.map(|(index, wrapper, _)| (index, wrapper)))
}

fn wrapper_candidates(source: &str, placeholder: &Placeholder) -> Vec<WrapperKind> {
    let mut wrappers = Vec::new();
    if placeholder.cardinality == Cardinality::Sequence || line_isolated(source, placeholder) {
        wrappers.extend([
            WrapperKind::Item,
            WrapperKind::Parameter,
            WrapperKind::Field,
            WrapperKind::Variant,
            WrapperKind::MatchArm,
            WrapperKind::Attribute,
        ]);
    }
    wrappers
}

fn validate_generated_syntax(
    path: &Path,
    source: &str,
    generated: &GeneratedSource,
    tree: &tree_sitter::Tree,
) -> Result<(), Vec<Diagnostic>> {
    let errors = error_nodes(tree.root_node());
    if errors.is_empty() {
        return Ok(());
    }
    Err(errors
        .into_iter()
        .map(|node| {
            Diagnostic::new(
                "CTC2001",
                DiagnosticCategory::InvalidTemplateSyntax,
                "The generated template contains invalid Rust syntax.",
                2,
            )
            .with_template(generated.map_range(
                path,
                source,
                node.start_byte(),
                node.end_byte(),
            ))
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
        let Some(leaf) = find_exact_node(
            tree.root_node(),
            sentinel.generated_range.start,
            sentinel.generated_range.end,
        ) else {
            diagnostics.push(
                Diagnostic::new(
                    "CTC2002",
                    DiagnosticCategory::UnsupportedPlaceholderContext,
                    "The parser could not locate the generated placeholder sentinel.",
                    2,
                )
                .with_template(sentinel.placeholder.range.clone()),
            );
            continue;
        };
        match select_target(leaf, &generated.source, sentinel) {
            Ok(target) => {
                if targets
                    .insert(target.id(), sentinel.placeholder.clone())
                    .is_some()
                {
                    diagnostics.push(
                        Diagnostic::new(
                            "CTC2002",
                            DiagnosticCategory::UnsupportedPlaceholderContext,
                            "Two placeholders resolve to the same syntax node.",
                            2,
                        )
                        .with_template(sentinel.placeholder.range.clone()),
                    );
                }
            }
            Err(message) => diagnostics.push(
                Diagnostic::new(
                    "CTC2002",
                    DiagnosticCategory::UnsupportedPlaceholderContext,
                    message,
                    2,
                )
                .with_template(sentinel.placeholder.range.clone()),
            ),
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
    if !is_identifier_node(leaf) {
        return Err("A placeholder sentinel did not parse as an identifier.".to_string());
    }
    validate_explicit_category(leaf, sentinel.placeholder.category)?;
    if sentinel.wrapper != WrapperKind::None {
        return wrapped_target(leaf, sentinel.wrapper);
    }
    if sentinel.placeholder.cardinality == Cardinality::Scalar
        && sentinel.placeholder.category != CaptureCategory::Inferred
    {
        return Ok(leaf);
    }
    let Some(item) = list_item_target(leaf) else {
        return if sentinel.placeholder.cardinality == Cardinality::Sequence {
            Err("A sequence placeholder must occur in a supported Rust syntax list.".to_string())
        } else {
            Ok(leaf)
        };
    };
    let item_text = compact_text(item, generated_source);
    let occupies_item = item_text == sentinel.text;
    if sentinel.placeholder.cardinality == Cardinality::Sequence {
        if occupies_item {
            Ok(item)
        } else {
            Err("A sequence placeholder must occupy one complete syntax-list item.".to_string())
        }
    } else if occupies_item {
        Ok(item)
    } else {
        Ok(leaf)
    }
}

fn keyword_target<'a>(
    leaf: Node<'a>,
    generated_source: &str,
    sentinel: &Sentinel,
) -> Result<Node<'a>, String> {
    let name = sentinel.placeholder.name.as_str();
    if keyword_wrapper(leaf, generated_source, name) {
        return Ok(leaf);
    }
    if compact_text(leaf, generated_source) == name {
        if let Some(parent) = leaf.parent()
            && keyword_wrapper(parent, generated_source, name)
        {
            return Ok(parent);
        }
        return Ok(leaf);
    }
    Err(format!(
        "Keyword placeholder `{}` did not parse as that keyword.",
        sentinel.placeholder.name
    ))
}

fn keyword_wrapper(node: Node<'_>, source: &str, keyword: &str) -> bool {
    matches!(
        node.kind(),
        "extern_modifier" | "function_modifiers" | "mutable_specifier" | "visibility_modifier"
    ) && compact_text(node, source) == keyword
}

fn wrapped_target(leaf: Node<'_>, wrapper: WrapperKind) -> Result<Node<'_>, String> {
    let Some(target) = first_ancestor_with_list_parent(leaf, wrapper_matches_parent(wrapper))
    else {
        return Err(
            "A placeholder did not parse as a supported Rust syntax-list item.".to_string(),
        );
    };
    Ok(target)
}

fn wrapper_matches_parent(wrapper: WrapperKind) -> impl Fn(Node<'_>) -> bool {
    move |node| match wrapper {
        WrapperKind::None => false,
        WrapperKind::Item => matches!(
            node.parent().map(|parent| parent.kind()),
            Some("source_file" | "block" | "declaration_list")
        ),
        WrapperKind::Parameter => matches!(
            node.parent().map(|parent| parent.kind()),
            Some("parameters" | "closure_parameters")
        ),
        WrapperKind::Field => node
            .parent()
            .is_some_and(|parent| parent.kind() == "field_declaration_list"),
        WrapperKind::Variant => node
            .parent()
            .is_some_and(|parent| parent.kind() == "enum_variant_list"),
        WrapperKind::MatchArm => node
            .parent()
            .is_some_and(|parent| parent.kind() == "match_block"),
        WrapperKind::Attribute => node.kind() == "attribute_item",
    }
}

fn first_ancestor_with_list_parent(
    leaf: Node<'_>,
    predicate: impl Fn(Node<'_>) -> bool,
) -> Option<Node<'_>> {
    let mut current = leaf;
    loop {
        if predicate(current) {
            return Some(current);
        }
        current = current.parent()?;
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

fn is_identifier_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "field_identifier" | "identifier" | "shorthand_field_identifier" | "type_identifier"
    )
}

fn validate_explicit_category(leaf: Node<'_>, category: CaptureCategory) -> Result<(), String> {
    match category {
        CaptureCategory::Inferred => Ok(()),
        CaptureCategory::Identifier => {
            if is_identifier_node(leaf) {
                Ok(())
            } else {
                Err("An identifier placeholder does not occur on an identifier.".to_string())
            }
        }
        CaptureCategory::Expression => {
            if slot_category(leaf) == SlotCategory::Expression {
                Ok(())
            } else {
                Err("An expression placeholder occurs in a type or name slot.".to_string())
            }
        }
        CaptureCategory::Type => {
            if slot_category(leaf) == SlotCategory::Type {
                Ok(())
            } else {
                Err("A type placeholder does not occur in a type slot.".to_string())
            }
        }
        CaptureCategory::Keyword => {
            Err("A keyword placeholder did not parse as a keyword.".to_string())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotCategory {
    Identifier,
    Expression,
    Type,
}

fn slot_category(leaf: Node<'_>) -> SlotCategory {
    if field_name_in_parent(leaf)
        .is_some_and(|field| matches!(field, "alias" | "field" | "macro" | "name"))
    {
        return SlotCategory::Identifier;
    }
    if leaf.kind() == "type_identifier" {
        return SlotCategory::Type;
    }
    let mut current = leaf;
    while let Some(parent) = current.parent() {
        if field_name_in_parent(current).is_some_and(|field| {
            matches!(
                field,
                "default_type" | "return_type" | "trait" | "type" | "type_parameters"
            )
        }) || matches!(
            parent.kind(),
            "abstract_type"
                | "bounded_type"
                | "dyn_trait_type"
                | "function_type"
                | "generic_type"
                | "generic_type_with_turbofish"
                | "reference_type"
                | "tuple_type"
                | "type_arguments"
                | "type_parameters"
                | "where_predicate"
        ) {
            return SlotCategory::Type;
        }
        if is_list_container(parent.kind()) || parent.kind() == "source_file" {
            break;
        }
        current = parent;
    }
    SlotCategory::Expression
}

fn field_name_in_parent<'tree>(node: Node<'tree>) -> Option<&'tree str> {
    let parent = node.parent()?;
    (0..parent.child_count()).find_map(|index| {
        parent
            .child(index)
            .is_some_and(|child| child.id() == node.id())
            .then(|| parent.field_name_for_child(index))
            .flatten()
    })
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
    let mut diagnostics = Vec::new();
    for derived_name in derived_names {
        for placeholder in placeholders
            .iter()
            .filter(|placeholder| placeholder.name == derived_name && !has_name_filter(placeholder))
        {
            if placeholder.category != CaptureCategory::Identifier {
                diagnostics.push(
                    Diagnostic::new(
                        "CTC2008",
                        DiagnosticCategory::InvalidFilter,
                        format!("Naming filters require identifier capture `{derived_name}`."),
                        2,
                    )
                    .with_template(placeholder.range.clone()),
                );
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(placeholders)
    } else {
        Err(diagnostics)
    }
}

fn has_name_filter(placeholder: &Placeholder) -> bool {
    placeholder.filters.iter().any(Filter::is_name_filter)
}

fn is_list_container(kind: &str) -> bool {
    matches!(
        kind,
        "arguments"
            | "block"
            | "closure_parameters"
            | "declaration_list"
            | "enum_variant_list"
            | "field_declaration_list"
            | "match_block"
            | "parameters"
            | "source_file"
            | "type_arguments"
            | "type_parameters"
            | "use_list"
    )
}

fn convert_template_node(
    node: Node<'_>,
    generated_source: &str,
    original_source: &str,
    path: &Path,
    lines: &LineIndex<'_>,
    offset_map: &[usize],
    targets: &BTreeMap<usize, Placeholder>,
) -> Option<TemplateNode> {
    if let Some(placeholder) = targets.get(&node.id()) {
        return placeholder_node(node, generated_source, path, placeholder);
    }
    let canonical = canonicalize_node(node, generated_source, path)?;
    let is_value_node = canonical.value.is_some() && canonical.children.is_empty();
    let mut children = Vec::new();
    for index in 0..if is_value_node { 0 } else { node.child_count() } {
        let Some(raw_child) = node.child(index) else {
            continue;
        };
        if let Some(child) = Some(raw_child).and_then(|child| {
            convert_template_node(
                child,
                generated_source,
                original_source,
                path,
                lines,
                offset_map,
                targets,
            )
        }) {
            if flatten_wrapper(raw_child.kind(), &child) {
                if let TemplateNode::Literal {
                    children: wrapper_children,
                    ..
                } = child
                {
                    children.extend(wrapper_children);
                }
            } else {
                children.push(child);
            }
        }
    }
    let start = offset_map
        .get(node.start_byte())
        .copied()
        .unwrap_or(original_source.len());
    let end = offset_map
        .get(node.end_byte())
        .copied()
        .unwrap_or(original_source.len());
    let mut fields = canonical.fields;
    remove_source_only_fields(&mut fields);
    Some(TemplateNode::Literal {
        kind: Arc::from(canonical_kind(node)),
        value: canonical.value,
        fields,
        children,
        range: lines.range(start, end),
    })
}

fn placeholder_node(
    node: Node<'_>,
    generated_source: &str,
    path: &Path,
    placeholder: &Placeholder,
) -> Option<TemplateNode> {
    let is_derived = placeholder.is_derived();
    Some(if placeholder.cardinality == Cardinality::Optional {
        let inner = if placeholder.category == CaptureCategory::Keyword {
            let canonical = canonicalize_node(node, generated_source, path)?;
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
    } else if placeholder.cardinality == Cardinality::Sequence {
        TemplateNode::Sequence {
            placeholder: placeholder.clone(),
        }
    } else if is_derived {
        TemplateNode::Derived {
            placeholder: placeholder.clone(),
        }
    } else {
        TemplateNode::Capture {
            placeholder: placeholder.clone(),
        }
    })
}

fn flatten_wrapper(kind: &str, node: &TemplateNode) -> bool {
    matches!(
        kind,
        "extern_modifier" | "function_modifiers" | "visibility_modifier"
    ) && matches!(
        node,
        TemplateNode::Literal {
            value: None,
            fields,
            children,
            ..
        } if fields.is_empty()
            && children
                .iter()
                .any(|child| matches!(child, TemplateNode::Optional { .. }))
    )
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

fn compact_text(node: Node<'_>, source: &str) -> String {
    fn append(node: Node<'_>, source: &str, output: &mut String) {
        if matches!(node.kind(), "line_comment" | "block_comment") {
            return;
        }
        if node.child_count() == 0 {
            if let Ok(value) = node.utf8_text(source.as_bytes()) {
                output.extend(value.chars().filter(|character| {
                    !character.is_whitespace() && *character != ',' && *character != ';'
                }));
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
