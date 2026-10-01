use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

use ctc_core::{
    diagnostic::{Diagnostic, DiagnosticCategory, TextRange},
    matcher::MatchMode,
    template::{
        CaptureCategory, Cardinality, CompiledTemplate, Filter, Placeholder, TemplateNode,
        with_access_filter,
    },
};
use tree_sitter::Node;

use crate::{
    canonicalize::{canonical_kind, canonicalize_node, error_nodes, parse_tree},
    members::{member_access, remove_member_fields},
};

mod generated;
#[cfg(test)]
mod tests;

use generated::{GeneratedSource, Sentinel, generate_source, line_isolated};

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
    let Some(root) = convert_template_node(
        tree.root_node(),
        &generated.source,
        source,
        path,
        &generated.offset_map,
        &targets,
    ) else {
        return Err(vec![Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            "The C++ adapter removed the template root.",
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
                "A contains or forbid template must consume a source declaration.",
                2,
            )
            .with_template(TextRange::from_offsets(path, source, 0, source.len())),
        ]);
    }

    Ok(CompiledTemplate {
        path: path.to_string_lossy().replace('\\', "/"),
        language_id: "cpp",
        mode,
        root,
    })
}

fn generate_parsable_source(
    source: &str,
    placeholders: &[Placeholder],
) -> Result<(GeneratedSource, tree_sitter::Tree), Vec<Diagnostic>> {
    let mut wrapped = BTreeSet::new();
    loop {
        let generated = generate_source(source, placeholders, &wrapped);
        let tree = parse_generated(&generated.source)?;
        let errors = error_nodes(tree.root_node());
        if errors.is_empty() {
            return Ok((generated, tree));
        }
        let current_score = error_score(&errors);
        let candidates = wrapping_candidates(source, &generated, &wrapped);
        if let Some(candidate) =
            best_wrapped_candidate(source, placeholders, &wrapped, current_score, &candidates)?
        {
            wrapped.insert(candidate);
            continue;
        }
        if let Some(all_wrapped) =
            improved_all_wrapped(source, placeholders, &wrapped, current_score, &candidates)?
        {
            wrapped = all_wrapped;
            continue;
        }
        return Ok((generated, tree));
    }
}

fn wrapping_candidates(
    source: &str,
    generated: &GeneratedSource,
    wrapped: &BTreeSet<usize>,
) -> Vec<usize> {
    generated
        .sentinels
        .iter()
        .filter(|sentinel| {
            sentinel.placeholder.category != CaptureCategory::Keyword
                && line_isolated(source, &sentinel.placeholder)
                && !wrapped.contains(&sentinel.index)
        })
        .map(|sentinel| sentinel.index)
        .collect()
}

fn best_wrapped_candidate(
    source: &str,
    placeholders: &[Placeholder],
    wrapped: &BTreeSet<usize>,
    current_score: (usize, usize),
    candidates: &[usize],
) -> Result<Option<usize>, Vec<Diagnostic>> {
    let mut best = None;
    for &candidate in candidates {
        let mut trial_wrapped = wrapped.clone();
        trial_wrapped.insert(candidate);
        let trial_generated = generate_source(source, placeholders, &trial_wrapped);
        let trial_tree = parse_generated(&trial_generated.source)?;
        let score = error_score(&error_nodes(trial_tree.root_node()));
        if score < best.map_or(current_score, |(_, best_score)| best_score) {
            best = Some((candidate, score));
        }
    }
    Ok(best.map(|(candidate, _)| candidate))
}

fn improved_all_wrapped(
    source: &str,
    placeholders: &[Placeholder],
    wrapped: &BTreeSet<usize>,
    current_score: (usize, usize),
    candidates: &[usize],
) -> Result<Option<BTreeSet<usize>>, Vec<Diagnostic>> {
    let mut all_wrapped = wrapped.clone();
    all_wrapped.extend(candidates);
    if all_wrapped.len() == wrapped.len() {
        return Ok(None);
    }
    let trial_generated = generate_source(source, placeholders, &all_wrapped);
    let trial_tree = parse_generated(&trial_generated.source)?;
    if error_score(&error_nodes(trial_tree.root_node())) < current_score {
        Ok(Some(all_wrapped))
    } else {
        Ok(None)
    }
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
                "The generated template contains invalid C++ syntax.",
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

fn parse_generated(source: &str) -> Result<tree_sitter::Tree, Vec<Diagnostic>> {
    parse_tree(source).map_err(|message| {
        vec![Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            message,
            2,
        )]
    })
}

fn find_exact_node(node: Node<'_>, start: usize, end: usize) -> Option<Node<'_>> {
    for index in 0..node.child_count() {
        if let Some(found) = node
            .child(index)
            .and_then(|child| find_exact_node(child, start, end))
        {
            return Some(found);
        }
    }
    (node.start_byte() == start && node.end_byte() == end).then_some(node)
}

fn error_score(errors: &[Node<'_>]) -> (usize, usize) {
    (
        errors
            .iter()
            .map(|node| node.end_byte().saturating_sub(node.start_byte()).max(1))
            .sum(),
        errors.len(),
    )
}

fn select_target<'a>(
    leaf: Node<'a>,
    generated_source: &str,
    sentinel: &Sentinel,
) -> Result<Node<'a>, String> {
    if sentinel.placeholder.category == CaptureCategory::Keyword {
        if !leaf.is_named() && leaf.kind() == sentinel.placeholder.name {
            if let Some(parent) = leaf.parent()
                && keyword_wrapper(parent, generated_source, &sentinel.placeholder.name)
            {
                return Ok(parent);
            }
            return Ok(leaf);
        }
        return Err(format!(
            "Keyword placeholder `{}` did not parse as that keyword.",
            sentinel.placeholder.name
        ));
    }
    if !is_identifier_node(leaf) {
        return Err("A placeholder sentinel did not parse as an identifier.".to_string());
    }
    validate_explicit_category(leaf, sentinel.placeholder.category)?;

    if sentinel.wrapped_item {
        let mut current = leaf;
        while let Some(parent) = current.parent() {
            if is_list_container(parent.kind()) {
                return Ok(current);
            }
            current = parent;
        }
        return Err("A standalone placeholder did not parse as a syntax-list item.".to_string());
    }

    fn keyword_wrapper(node: Node<'_>, source: &str, keyword: &str) -> bool {
        matches!(
            node.kind(),
            "access_specifier"
                | "explicit_function_specifier"
                | "function_specifier"
                | "noexcept"
                | "storage_class_specifier"
                | "type_qualifier"
                | "virtual_specifier"
        ) && node
            .utf8_text(source.as_bytes())
            .is_ok_and(|text| text.trim() == keyword)
    }

    if sentinel.placeholder.cardinality == Cardinality::Scalar
        && sentinel.placeholder.category != CaptureCategory::Inferred
    {
        return Ok(leaf);
    }

    let mut current = leaf;
    let mut list_item = None;
    while let Some(parent) = current.parent() {
        if is_list_container(parent.kind()) {
            list_item = Some(current);
            break;
        }
        current = parent;
    }

    let Some(item) = list_item else {
        if sentinel.placeholder.cardinality == Cardinality::Sequence {
            return Err(
                "A sequence placeholder must occur in a supported C++ syntax list.".to_string(),
            );
        }
        return Ok(leaf);
    };
    let item_text = item
        .utf8_text(generated_source.as_bytes())
        .unwrap_or_default()
        .chars()
        .filter(|character| !character.is_whitespace() && *character != ';')
        .collect::<String>();
    let occupies_item = item_text == sentinel.text;

    if sentinel.placeholder.cardinality == Cardinality::Sequence {
        if !occupies_item {
            return Err(
                "A sequence placeholder must occupy one complete C++ syntax-list item.".to_string(),
            );
        }
        Ok(item)
    } else if occupies_item {
        Ok(item)
    } else {
        Ok(leaf)
    }
}

fn is_identifier_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "identifier"
            | "type_identifier"
            | "field_identifier"
            | "namespace_identifier"
            | "statement_identifier"
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
            if !leaf.is_named() {
                Ok(())
            } else {
                Err("A keyword placeholder did not parse as a keyword.".to_string())
            }
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
    if field_name_in_parent(leaf).is_some_and(|field| {
        matches!(
            field,
            "name" | "declarator" | "field" | "namespace" | "label"
        )
    }) {
        return SlotCategory::Identifier;
    }
    if leaf.kind() == "type_identifier" {
        return SlotCategory::Type;
    }

    let mut current = leaf;
    while let Some(parent) = current.parent() {
        if field_name_in_parent(current) == Some("type")
            || matches!(
                parent.kind(),
                "type_descriptor"
                    | "base_class_clause"
                    | "template_type"
                    | "trailing_return_type"
                    | "type_definition"
                    | "alias_declaration"
                    | "dependent_type"
                    | "placeholder_type_specifier"
                    | "sized_type_specifier"
            )
        {
            return SlotCategory::Type;
        }
        if is_list_container(parent.kind()) || parent.kind() == "translation_unit" {
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
        "translation_unit"
            | "field_declaration_list"
            | "parameter_list"
            | "argument_list"
            | "compound_statement"
            | "declaration_list"
            | "initializer_list"
            | "enumerator_list"
            | "field_initializer_list"
            | "template_argument_list"
            | "template_parameter_list"
            | "lambda_capture_specifier"
    )
}

fn convert_template_node(
    node: Node<'_>,
    generated_source: &str,
    original_source: &str,
    path: &Path,
    offset_map: &[usize],
    targets: &BTreeMap<usize, Placeholder>,
) -> Option<TemplateNode> {
    if let Some(placeholder) = targets.get(&node.id()) {
        let is_derived = placeholder.is_derived();
        let placeholder =
            &with_access_filter(placeholder, member_access(node, generated_source, true));
        return Some(if placeholder.cardinality == Cardinality::Optional {
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
        });
    }

    let canonical = canonicalize_node(node, generated_source, path)?;
    // A string literal is one node with a value in the canonical form, so its
    // raw content must not become template children.
    let is_value_node = canonical.value.is_some() && canonical.children.is_empty();
    let mut children = Vec::new();
    for index in 0..if is_value_node { 0 } else { node.child_count() } {
        if let Some(child) = node.child(index).and_then(|child| {
            convert_template_node(
                child,
                generated_source,
                original_source,
                path,
                offset_map,
                targets,
            )
        }) {
            children.push(child);
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
    remove_member_fields(&mut fields);
    if let Some(access) = member_access(node, generated_source, true) {
        fields.insert(
            Arc::from("access"),
            ctc_core::canonical::CanonicalScalar::String(Arc::from(access)),
        );
    }
    Some(TemplateNode::Literal {
        kind: Arc::from(canonical_kind(node)),
        value: canonical.value,
        fields,
        children,
        range: TextRange::from_offsets(path, original_source, start, end),
    })
}

fn literal_from_canonical(
    node: ctc_core::canonical::CanonicalNode,
    range: &TextRange,
) -> TemplateNode {
    TemplateNode::Literal {
        kind: node.kind,
        value: node.value,
        fields: node.fields,
        children: node
            .children
            .into_iter()
            .map(|child| literal_from_canonical(child, range))
            .collect(),
        range: range.clone(),
    }
}

fn can_match_empty_top_level(root: &TemplateNode) -> bool {
    let TemplateNode::Literal { children, .. } = root else {
        return false;
    };
    children.is_empty()
        || children.iter().all(|child| {
            matches!(
                child,
                TemplateNode::Sequence { .. } | TemplateNode::Optional { .. }
            )
        })
}
