use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::diagnostic::Diagnostic;

use super::{PartnerFile, config_error, display_path, normalize_existing};

const PARTNER_TOKENS: &[&str] = &["dir", "subdir", "stem", "ext"];

pub(super) fn validate_partner_patterns(
    patterns: &[String],
    rule_id: &str,
    field: &str,
    config_display_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    if patterns.is_empty() {
        diagnostics.push(config_error(
            config_display_path,
            format!("Semantic rule `{rule_id}` must contain at least one {field} pattern."),
        ));
    }
    for pattern in patterns {
        if let Err(reason) = check_partner_pattern(pattern) {
            diagnostics.push(config_error(
                config_display_path,
                format!(
                    "Semantic rule `{rule_id}` has invalid {field} pattern `{pattern}`: {reason}."
                ),
            ));
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

pub(super) fn check_partner_pattern(pattern: &str) -> Result<(), String> {
    if pattern.is_empty() {
        return Err("the pattern is empty".to_string());
    }
    if pattern.contains('\\') {
        return Err("use `/` separators".to_string());
    }
    if pattern.starts_with('/') {
        return Err("use a path relative to the scan root".to_string());
    }
    if pattern.split('/').any(|segment| segment == "..") {
        return Err("a pattern cannot contain `..`".to_string());
    }
    let mut rest = pattern;
    while let Some(start) = rest.find(['{', '}']) {
        if rest[start..].starts_with('}') {
            return Err("`}` has no matching `{`".to_string());
        }
        let Some(length) = rest[start..].find('}') else {
            return Err("`{` has no matching `}`".to_string());
        };
        let token = &rest[start + 1..start + length];
        if !PARTNER_TOKENS.contains(&token) {
            return Err(format!(
                "unknown token `{{{token}}}`. Use `{{dir}}`, `{{subdir}}`, `{{stem}}`, or `{{ext}}`"
            ));
        }
        rest = &rest[start + length + 1..];
    }
    Ok(())
}

/// Replaces the tokens in a validated partner pattern with parts of the
/// selected file path. Empty and `.` segments are removed.
pub(super) fn expand_partner_pattern(pattern: &str, anchor: &str) -> Option<String> {
    let (dir, name) = anchor.rsplit_once('/').unwrap_or(("", anchor));
    let subdir = dir.split_once('/').map_or("", |(_, rest)| rest);
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, ext),
        _ => (name, ""),
    };
    let mut expanded = String::new();
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        expanded.push_str(&rest[..start]);
        let end = start + rest[start..].find('}')?;
        expanded.push_str(match &rest[start + 1..end] {
            "dir" => dir,
            "subdir" => subdir,
            "stem" => stem,
            "ext" => ext,
            _ => return None,
        });
        rest = &rest[end + 1..];
    }
    expanded.push_str(rest);
    let segments = expanded
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>();
    if segments.is_empty() || segments.contains(&"..") {
        return None;
    }
    Some(segments.join("/"))
}

pub(super) fn resolve_partner(
    root: &Path,
    anchor: &str,
    patterns: &[String],
) -> (Option<PartnerFile>, Vec<String>) {
    let mut candidates = Vec::new();
    for pattern in patterns {
        if let Some(candidate) = expand_partner_pattern(pattern, anchor)
            && candidate != anchor
            && !candidates.contains(&candidate)
        {
            candidates.push(candidate);
        }
    }
    let partner = candidates
        .iter()
        .find_map(|candidate| existing_partner(root, candidate));
    (partner, candidates)
}

/// Returns the partner file when it is a regular file inside the root and no
/// path component below the root is a symbolic link.
fn existing_partner(root: &Path, relative: &str) -> Option<PartnerFile> {
    let segments = relative.split('/').collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    for (index, segment) in segments.iter().enumerate() {
        current.push(segment);
        let metadata = fs::symlink_metadata(&current).ok()?;
        let last = index + 1 == segments.len();
        if metadata.file_type().is_symlink()
            || (last && !metadata.is_file())
            || (!last && !metadata.is_dir())
        {
            return None;
        }
    }
    let path = normalize_existing(&current).ok()?;
    if !path.starts_with(root) {
        return None;
    }
    Some(PartnerFile {
        display_path: PathBuf::from(display_path(root, &path)),
        path,
    })
}
