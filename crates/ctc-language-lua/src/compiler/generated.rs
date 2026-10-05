use std::path::Path;

use ctc_core::{
    diagnostic::{Diagnostic, DiagnosticCategory, TextRange},
    template::{CaptureCategory, Placeholder},
};
use tree_sitter::Node;

use super::parse_tree;

pub(super) fn parse_generated(source: &str) -> Result<tree_sitter::Tree, Vec<Diagnostic>> {
    parse_tree(source).map_err(|message| {
        vec![Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            message,
            2,
        )]
    })
}

pub(super) struct GeneratedSource {
    pub(super) source: String,
    pub(super) offset_map: Vec<usize>,
    pub(super) sentinels: Vec<Sentinel>,
}

impl GeneratedSource {
    pub(super) fn map_range(
        &self,
        path: &Path,
        original: &str,
        start: usize,
        end: usize,
    ) -> TextRange {
        let mapped_start = self
            .offset_map
            .get(start)
            .copied()
            .unwrap_or(original.len());
        let mapped_end = self.offset_map.get(end).copied().unwrap_or(original.len());
        TextRange::from_offsets(path, original, mapped_start, mapped_end)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WrapperKind {
    None,
    /// `name()`, so that a placeholder alone on a line is a statement. Lua
    /// does not allow an expression as a statement.
    Statement,
}

pub(super) struct Sentinel {
    pub(super) text: String,
    pub(super) generated_range: std::ops::Range<usize>,
    pub(super) placeholder: Placeholder,
    pub(super) wrapper: WrapperKind,
}

pub(super) fn generate_source(
    source: &str,
    placeholders: &[Placeholder],
    wrappers: &[WrapperKind],
) -> GeneratedSource {
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
        let wrapper = wrappers[index];
        let start = append_wrapper(
            &mut generated,
            &mut offset_map,
            &text,
            placeholder.byte_range.start,
            placeholder.byte_range.end,
            wrapper,
        );
        sentinels.push(Sentinel {
            text,
            generated_range: start..generated.len() - wrapper_suffix(wrapper).len(),
            placeholder: placeholder.clone(),
            wrapper,
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

pub(super) fn line_isolated(source: &str, placeholder: &Placeholder) -> bool {
    let line_start = source[..placeholder.byte_range.start]
        .rfind(['\r', '\n'])
        .map_or(0, |offset| offset + 1);
    let line_end = source[placeholder.byte_range.end..]
        .find(['\r', '\n'])
        .map_or(source.len(), |offset| placeholder.byte_range.end + offset);
    source[line_start..placeholder.byte_range.start]
        .trim()
        .is_empty()
        && source[placeholder.byte_range.end..line_end]
            .trim()
            .is_empty()
}

pub(super) fn find_exact_node(node: Node<'_>, start: usize, end: usize) -> Option<Node<'_>> {
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

pub(super) fn error_score(errors: &[Node<'_>]) -> (usize, usize) {
    (
        errors
            .iter()
            .map(|node| node.end_byte().saturating_sub(node.start_byte()).max(1))
            .sum(),
        errors.len(),
    )
}

fn append_wrapper(
    generated: &mut String,
    offset_map: &mut Vec<usize>,
    text: &str,
    start: usize,
    end: usize,
    wrapper: WrapperKind,
) -> usize {
    let sentinel_start = generated.len();
    generated.push_str(text);
    offset_map.extend(std::iter::repeat_n(start, text.len()));
    let suffix = wrapper_suffix(wrapper);
    generated.push_str(suffix);
    offset_map.extend(std::iter::repeat_n(end, suffix.len()));
    sentinel_start
}

fn wrapper_suffix(wrapper: WrapperKind) -> &'static str {
    match wrapper {
        WrapperKind::None => "",
        WrapperKind::Statement => "()",
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
