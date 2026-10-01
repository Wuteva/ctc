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
        remove_member_fields,
    },
};
use tree_sitter::Node;

use crate::canonicalize::{canonical_kind, canonicalize_node, error_nodes, parse_tree};

pub fn compile_template(
    source: &str,
    path: &Path,
    placeholders: &[Placeholder],
    mode: MatchMode,
) -> Result<CompiledTemplate, Vec<Diagnostic>> {
    let generated = generate_source(source, placeholders);
    let tree = parse_generated(&generated.source)?;
    validate_generated_syntax(path, source, &generated, &tree)?;
    let mut targets = resolve_targets(&generated, &tree)?;
    resolve_capture_categories(&mut targets)?;

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
            "The TypeScript adapter removed the template root.",
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
                "A contains or forbid template must consume a source statement.",
                2,
            )
            .with_template(TextRange::from_offsets(path, source, 0, source.len())),
        ]);
    }

    Ok(CompiledTemplate {
        path: path.to_string_lossy().replace('\\', "/"),
        language_id: "typescript",
        mode,
        root,
    })
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

fn validate_generated_syntax(
    path: &Path,
    source: &str,
    generated: &GeneratedSource,
    tree: &tree_sitter::Tree,
) -> Result<(), Vec<Diagnostic>> {
    let error_nodes = error_nodes(tree.root_node());
    if error_nodes.is_empty() {
        return Ok(());
    }
    Err(error_nodes
        .into_iter()
        .map(|node| {
            Diagnostic::new(
                "CTC2001",
                DiagnosticCategory::InvalidTemplateSyntax,
                "The generated template contains invalid TypeScript syntax.",
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
                let key = target.id();
                if targets.insert(key, sentinel.placeholder.clone()).is_some() {
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

struct GeneratedSource {
    source: String,
    offset_map: Vec<usize>,
    sentinels: Vec<Sentinel>,
}

impl GeneratedSource {
    fn map_range(&self, path: &Path, original: &str, start: usize, end: usize) -> TextRange {
        let mapped_start = self
            .offset_map
            .get(start)
            .copied()
            .unwrap_or(original.len());
        let mapped_end = self.offset_map.get(end).copied().unwrap_or(original.len());
        TextRange::from_offsets(path, original, mapped_start, mapped_end)
    }
}

struct Sentinel {
    text: String,
    generated_range: std::ops::Range<usize>,
    placeholder: Placeholder,
}

fn generate_source(source: &str, placeholders: &[Placeholder]) -> GeneratedSource {
    let mut prefix = "__CTC_PLACEHOLDER_".to_string();
    while source.contains(&prefix) {
        prefix.push('X');
    }

    let mut generated = String::new();
    let mut offset_map = Vec::new();
    let mut sentinels = Vec::new();
    let mut source_offset = 0;

    for (index, placeholder) in placeholders.iter().enumerate() {
        append_original(
            &mut generated,
            &mut offset_map,
            source,
            source_offset,
            placeholder.byte_range.start,
        );
        let text = if placeholder.category == CaptureCategory::Keyword {
            placeholder.name.clone()
        } else {
            format!("{prefix}{index}__")
        };
        let start = generated.len();
        generated.push_str(&text);
        offset_map.extend(std::iter::repeat_n(
            placeholder.byte_range.start,
            text.len(),
        ));
        sentinels.push(Sentinel {
            text,
            generated_range: start..generated.len(),
            placeholder: placeholder.clone(),
        });
        source_offset = placeholder.byte_range.end;
    }
    append_original(
        &mut generated,
        &mut offset_map,
        source,
        source_offset,
        source.len(),
    );
    offset_map.push(source.len());
    GeneratedSource {
        source: generated,
        offset_map,
        sentinels,
    }
}

fn append_original(
    generated: &mut String,
    offset_map: &mut Vec<usize>,
    source: &str,
    start: usize,
    end: usize,
) {
    generated.push_str(&source[start..end]);
    offset_map.extend(start..end);
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

fn select_target<'a>(
    leaf: Node<'a>,
    generated_source: &str,
    sentinel: &Sentinel,
) -> Result<Node<'a>, String> {
    if sentinel.placeholder.category == CaptureCategory::Keyword {
        if !leaf.is_named() && leaf.kind() == sentinel.placeholder.name {
            return Ok(leaf);
        }
        return Err(format!(
            "Keyword placeholder `{}` did not parse as that keyword.",
            sentinel.placeholder.name
        ));
    }
    if !matches!(
        leaf.kind(),
        "identifier" | "type_identifier" | "property_identifier"
    ) {
        return Err("A placeholder sentinel did not parse as an identifier.".to_string());
    }
    validate_explicit_category(leaf, sentinel.placeholder.category)?;

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
                "A sequence placeholder must occur in a supported syntax list.".to_string(),
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
                "A sequence placeholder must occupy one complete syntax-list item.".to_string(),
            );
        }
        Ok(item)
    } else if occupies_item {
        Ok(item)
    } else {
        Ok(leaf)
    }
}

fn validate_explicit_category(leaf: Node<'_>, category: CaptureCategory) -> Result<(), String> {
    match category {
        CaptureCategory::Inferred => Ok(()),
        CaptureCategory::Identifier => {
            if matches!(
                leaf.kind(),
                "identifier" | "type_identifier" | "property_identifier"
            ) {
                Ok(())
            } else {
                Err("An identifier placeholder does not occur on an identifier.".to_string())
            }
        }
        CaptureCategory::Expression => {
            if slot_category(leaf) != SlotCategory::Expression {
                Err("An expression placeholder occurs in a type or name slot.".to_string())
            } else {
                Ok(())
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
    if let Some(parent) = leaf.parent() {
        for index in 0..parent.child_count() {
            if parent
                .child(index)
                .is_some_and(|child| child.id() == leaf.id())
                && parent
                    .field_name_for_child(index)
                    .is_some_and(|field| matches!(field, "name" | "property" | "alias"))
            {
                return SlotCategory::Identifier;
            }
        }
    }

    let mut current = leaf;
    while let Some(parent) = current.parent() {
        if matches!(
            parent.kind(),
            "type_annotation"
                | "type_arguments"
                | "implements_clause"
                | "extends_type_clause"
                | "type_alias_declaration"
                | "type_query"
                | "generic_type"
        ) {
            return SlotCategory::Type;
        }
        if is_list_container(parent.kind()) || parent.kind() == "program" {
            break;
        }
        current = parent;
    }
    SlotCategory::Expression
}

fn resolve_capture_categories(
    targets: &mut BTreeMap<usize, Placeholder>,
) -> Result<(), Vec<Diagnostic>> {
    let mut explicit_categories = BTreeMap::new();
    let mut derived_names = BTreeSet::new();
    for placeholder in targets.values() {
        if placeholder.category == CaptureCategory::Keyword {
            continue;
        }
        let derived = has_name_filter(placeholder);
        if derived {
            derived_names.insert(placeholder.name.clone());
        } else if placeholder.category != CaptureCategory::Inferred {
            explicit_categories.insert(placeholder.name.clone(), placeholder.category);
        }
    }

    for placeholder in targets.values_mut() {
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
        for placeholder in targets
            .values()
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
        Ok(())
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
        "program"
            | "interface_body"
            | "class_body"
            | "formal_parameters"
            | "arguments"
            | "statement_block"
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
        return Some(convert_placeholder_node(placeholder));
    }

    let canonical = canonicalize_node(node, generated_source, path)?;
    let children = convert_literal_children(
        node,
        generated_source,
        original_source,
        path,
        offset_map,
        targets,
        canonical.value.is_some() && canonical.children.is_empty(),
    );
    let mut fields = canonical.fields;
    remove_member_fields(&mut fields);
    Some(TemplateNode::Literal {
        kind: Arc::from(canonical_kind(node, generated_source)),
        value: canonical.value,
        fields,
        children,
        range: node_range(node, original_source, path, offset_map),
    })
}

fn convert_placeholder_node(placeholder: &Placeholder) -> TemplateNode {
    if placeholder.cardinality == Cardinality::Optional {
        return TemplateNode::Optional {
            node: Box::new(optional_placeholder_inner(placeholder)),
            range: placeholder.range.clone(),
        };
    }
    if placeholder.cardinality == Cardinality::Sequence {
        return TemplateNode::Sequence {
            placeholder: placeholder.clone(),
        };
    }
    if placeholder.is_derived() {
        TemplateNode::Derived {
            placeholder: placeholder.clone(),
        }
    } else {
        TemplateNode::Capture {
            placeholder: placeholder.clone(),
        }
    }
}

fn optional_placeholder_inner(placeholder: &Placeholder) -> TemplateNode {
    if placeholder.category == CaptureCategory::Keyword {
        TemplateNode::Literal {
            kind: Arc::from("Token"),
            value: Some(ctc_core::canonical::CanonicalScalar::String(Arc::from(
                placeholder.name.as_str(),
            ))),
            fields: BTreeMap::new(),
            children: Vec::new(),
            range: placeholder.range.clone(),
        }
    } else {
        TemplateNode::Capture {
            placeholder: placeholder.clone(),
        }
    }
}

fn convert_literal_children(
    node: Node<'_>,
    generated_source: &str,
    original_source: &str,
    path: &Path,
    offset_map: &[usize],
    targets: &BTreeMap<usize, Placeholder>,
    is_value_node: bool,
) -> Vec<TemplateNode> {
    if is_value_node {
        return Vec::new();
    }

    let mut children = Vec::new();
    let mut decorators = Vec::new();
    for index in 0..node.child_count() {
        let Some(raw_child) = node.child(index) else {
            continue;
        };
        let Some(child) = convert_template_node(
            raw_child,
            generated_source,
            original_source,
            path,
            offset_map,
            targets,
        ) else {
            continue;
        };
        if node.kind() == "class_body" {
            collect_class_body_child(raw_child, child, &mut children, &mut decorators, targets);
        } else {
            children.push(child);
        }
    }
    children.append(&mut decorators);
    children
}

fn collect_class_body_child(
    raw_child: Node<'_>,
    mut child: TemplateNode,
    children: &mut Vec<TemplateNode>,
    decorators: &mut Vec<TemplateNode>,
    targets: &BTreeMap<usize, Placeholder>,
) {
    if raw_child.kind() == "decorator" && !targets.contains_key(&raw_child.id()) {
        decorators.push(child);
        return;
    }
    if let TemplateNode::Literal {
        children: member_children,
        ..
    } = &mut child
    {
        member_children.splice(0..0, decorators.drain(..));
    } else {
        children.append(decorators);
    }
    children.push(child);
}

fn node_range(
    node: Node<'_>,
    original_source: &str,
    path: &Path,
    offset_map: &[usize],
) -> TextRange {
    let start = mapped_offset(offset_map, node.start_byte(), original_source.len());
    let end = mapped_offset(offset_map, node.end_byte(), original_source.len());
    TextRange::from_offsets(path, original_source, start, end)
}

fn mapped_offset(offset_map: &[usize], offset: usize, fallback: usize) -> usize {
    offset_map.get(offset).copied().unwrap_or(fallback)
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

#[cfg(test)]
mod tests;
