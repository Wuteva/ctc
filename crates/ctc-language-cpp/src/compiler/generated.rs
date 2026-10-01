use std::{collections::BTreeSet, path::Path};

use ctc_core::{
    diagnostic::TextRange,
    template::{CaptureCategory, Placeholder},
};

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

pub(super) struct Sentinel {
    pub(super) index: usize,
    pub(super) text: String,
    pub(super) generated_range: std::ops::Range<usize>,
    pub(super) placeholder: Placeholder,
    pub(super) wrapped_item: bool,
}

pub(super) fn generate_source(
    source: &str,
    placeholders: &[Placeholder],
    wrapped: &BTreeSet<usize>,
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
        let wrapped_item = wrapped.contains(&index);
        if wrapped_item {
            append_inserted(
                &mut generated,
                &mut offset_map,
                "void ",
                placeholder.byte_range.start,
            );
        }
        let start = generated.len();
        generated.push_str(&text);
        offset_map.extend(std::iter::repeat_n(
            placeholder.byte_range.start,
            text.len(),
        ));
        let generated_range = start..generated.len();
        if wrapped_item {
            append_inserted(
                &mut generated,
                &mut offset_map,
                "();",
                placeholder.byte_range.end,
            );
        }
        sentinels.push(Sentinel {
            index,
            text,
            generated_range,
            placeholder: placeholder.clone(),
            wrapped_item,
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

fn append_inserted(
    generated: &mut String,
    offset_map: &mut Vec<usize>,
    value: &str,
    source_offset: usize,
) {
    generated.push_str(value);
    offset_map.extend(std::iter::repeat_n(source_offset, value.len()));
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
