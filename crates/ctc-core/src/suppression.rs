use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::Path,
};

use crate::diagnostic::{Diagnostic, DiagnosticCategory, TextRange};

const NEXT_LINE: &str = "ctc-ignore-next-line";
const FILE: &str = "ctc-ignore-file";
const PREFIX: &str = "ctc-ignore";

/// Suppression comments found in one source file.
#[derive(Clone, Debug, Default)]
pub struct Suppressions {
    file_rules: BTreeSet<String>,
    line_rules: BTreeMap<u32, BTreeSet<String>>,
    pub diagnostics: Vec<Diagnostic>,
    /// Every rule that a comment names, in source order, including rules that
    /// do not allow suppression.
    pub directives: Vec<SuppressionDirective>,
}

#[derive(Clone, Debug)]
pub struct SuppressionDirective {
    pub rule_id: String,
    pub range: TextRange,
}

impl Suppressions {
    /// Returns true when a suppression comment in this file skips the diagnostic.
    /// The caller must make sure the diagnostic belongs to this file.
    pub fn suppresses(&self, diagnostic: &Diagnostic) -> bool {
        let (Some(rule_id), Some(source)) = (&diagnostic.rule_id, &diagnostic.source) else {
            return false;
        };
        self.file_rules.contains(rule_id)
            || self
                .line_rules
                .get(&source.start.line)
                .is_some_and(|rules| rules.contains(rule_id))
    }
}

/// Reads suppression comments from the comment ranges that a language adapter
/// found in the raw syntax tree. A comment that names a rule outside
/// `ignorable_rule_ids` does not suppress it and reports `CTC5002`.
pub fn collect_suppressions(
    path: &Path,
    source: &str,
    comments: &[Range<usize>],
    known_rule_ids: &BTreeSet<String>,
    ignorable_rule_ids: &BTreeSet<String>,
) -> Suppressions {
    let comments = normalize_comment_ranges(source, comments);
    let mut suppressions = Suppressions::default();
    for range in &comments {
        process_suppression_comment(
            &mut suppressions,
            path,
            source,
            range,
            &comments,
            known_rule_ids,
            ignorable_rule_ids,
        );
    }
    suppressions
}

fn normalize_comment_ranges(source: &str, comments: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut comments = comments
        .iter()
        .filter(|range| range.start <= range.end && range.end <= source.len())
        .filter(|range| source.is_char_boundary(range.start) && source.is_char_boundary(range.end))
        .cloned()
        .collect::<Vec<_>>();
    comments.sort_by_key(|range| (range.start, range.end));
    comments
}

fn process_suppression_comment(
    suppressions: &mut Suppressions,
    path: &Path,
    source: &str,
    range: &Range<usize>,
    comments: &[Range<usize>],
    known_rule_ids: &BTreeSet<String>,
    ignorable_rule_ids: &BTreeSet<String>,
) {
    let Some(body) = comment_body(&source[range.clone()]) else {
        return;
    };
    let directive = body.split_whitespace().next().unwrap_or_default();
    if !directive.starts_with(PREFIX) {
        return;
    }
    if directive != NEXT_LINE && directive != FILE {
        suppressions.diagnostics.push(invalid_suppression(
            path,
            source,
            range,
            format!("Unknown suppression directive `{directive}`. Use `{NEXT_LINE}` or `{FILE}`."),
        ));
        return;
    }
    let rule_list = strip_reason(&body[directive.len()..]).trim();
    if rule_list.is_empty() {
        suppressions.diagnostics.push(invalid_suppression(
            path,
            source,
            range,
            format!("A `{directive}` comment must name at least one rule identifier."),
        ));
        return;
    }

    let rule_ids = collect_rule_ids(
        suppressions,
        path,
        source,
        range,
        rule_list,
        known_rule_ids,
        ignorable_rule_ids,
    );
    if directive == FILE {
        suppressions.file_rules.extend(rule_ids);
    } else if let Some(line) = next_code_line(source, range.end, comments) {
        suppressions
            .line_rules
            .entry(line)
            .or_default()
            .extend(rule_ids);
    }
}

fn collect_rule_ids(
    suppressions: &mut Suppressions,
    path: &Path,
    source: &str,
    range: &Range<usize>,
    rule_list: &str,
    known_rule_ids: &BTreeSet<String>,
    ignorable_rule_ids: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut rule_ids = BTreeSet::new();
    for rule_id in rule_list.split(',').map(str::trim) {
        if rule_id.is_empty() {
            suppressions.diagnostics.push(invalid_suppression(
                path,
                source,
                range,
                "The suppression comment contains an empty rule identifier.".to_string(),
            ));
        } else if !known_rule_ids.contains(rule_id) {
            suppressions.diagnostics.push(invalid_suppression(
                path,
                source,
                range,
                format!("The suppression comment names unknown rule identifier `{rule_id}`."),
            ));
        } else {
            suppressions.directives.push(SuppressionDirective {
                rule_id: rule_id.to_string(),
                range: suppression_range(path, source, range),
            });
            if ignorable_rule_ids.contains(rule_id) {
                rule_ids.insert(rule_id.to_string());
            } else {
                suppressions.diagnostics.push(
                    Diagnostic::new(
                        "CTC5002",
                        DiagnosticCategory::SuppressionNotAllowed,
                        format!(
                            "Rule `{rule_id}` does not allow suppression comments. Set `allowIgnore` to true on the rule to allow them."
                        ),
                        1,
                    )
                    .with_rule(rule_id)
                    .with_source(suppression_range(path, source, range)),
                );
            }
        }
    }
    rule_ids
}

fn invalid_suppression(
    path: &Path,
    source: &str,
    range: &Range<usize>,
    message: String,
) -> Diagnostic {
    Diagnostic::new(
        "CTC5001",
        DiagnosticCategory::InvalidSuppression,
        message,
        1,
    )
    .with_source(suppression_range(path, source, range))
}

fn suppression_range(path: &Path, source: &str, range: &Range<usize>) -> TextRange {
    TextRange::from_offsets(path, source, range.start, range.end)
}

fn comment_body(text: &str) -> Option<&str> {
    if let Some(body) = text.strip_prefix("//") {
        return Some(body.trim_start_matches('/').trim());
    }
    if let Some(body) = text.strip_prefix("--") {
        return Some(
            lua_long_comment_body(body).unwrap_or_else(|| body.trim_start_matches('-').trim()),
        );
    }
    let body = text.strip_prefix("/*")?;
    let body = body.strip_suffix("*/").unwrap_or(body);
    Some(body.trim().trim_start_matches('*').trim())
}

/// The text inside a Lua long comment such as `--[[ ... ]]` or
/// `--[==[ ... ]==]`, given the text after `--`.
fn lua_long_comment_body(body: &str) -> Option<&str> {
    let level = body
        .strip_prefix('[')?
        .bytes()
        .take_while(|byte| *byte == b'=')
        .count();
    let equals = "=".repeat(level);
    let inner = body.strip_prefix(format!("[{equals}[").as_str())?;
    Some(
        inner
            .strip_suffix(format!("]{equals}]").as_str())
            .unwrap_or(inner)
            .trim(),
    )
}

/// Removes the optional reason that follows a ` -- ` separator.
fn strip_reason(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut index = 0;
    while let Some(offset) = text[index..].find("--") {
        let start = index + offset;
        let end = start + 2;
        let before = start == 0 || bytes[start - 1].is_ascii_whitespace();
        let after = end == bytes.len() || bytes[end].is_ascii_whitespace();
        if before && after {
            return &text[..start];
        }
        index = end;
    }
    text
}

/// Finds the line of the first code after `offset`, skipping white space and
/// other comments.
fn next_code_line(source: &str, offset: usize, comments: &[Range<usize>]) -> Option<u32> {
    let mut position = offset;
    loop {
        let rest = &source[position..];
        let skipped = rest.len() - rest.trim_start().len();
        position += skipped;
        if position >= source.len() {
            return None;
        }
        match comments.iter().find(|range| range.start == position) {
            Some(range) => position = range.end.max(position + 1),
            None => break,
        }
    }
    Some(
        source[..position]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count() as u32
            + 1,
    )
}

#[cfg(test)]
mod tests;
