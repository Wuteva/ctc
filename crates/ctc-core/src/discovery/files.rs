use std::{
    fs,
    path::{Path, PathBuf},
};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::{
    diagnostic::{Diagnostic, DiagnosticCategory},
    language::LanguageRegistry,
};

#[derive(Clone, Debug)]
pub(super) struct ExplicitPath {
    pub(super) path: PathBuf,
    pub(super) is_file: bool,
}

pub(super) fn normalize_explicit_paths(
    root: &Path,
    paths: &[PathBuf],
) -> Result<Vec<ExplicitPath>, Vec<Diagnostic>> {
    let mut result = Vec::new();
    let mut diagnostics = Vec::new();
    for path in paths {
        let resolved = if path.is_absolute() {
            path.clone()
        } else {
            root.join(path)
        };
        match normalize_existing(&resolved) {
            Ok(normalized) if normalized.starts_with(root) => result.push(ExplicitPath {
                is_file: normalized.is_file(),
                path: normalized,
            }),
            Ok(_) => diagnostics.push(Diagnostic::new(
                "CTC0001",
                DiagnosticCategory::InvalidCommandLine,
                format!(
                    "Explicit path `{}` is outside the scan root.",
                    path.display()
                ),
                2,
            )),
            Err(message) => diagnostics.push(Diagnostic::new(
                "CTC0001",
                DiagnosticCategory::InvalidCommandLine,
                message,
                2,
            )),
        }
    }
    if diagnostics.is_empty() {
        Ok(result)
    } else {
        Err(diagnostics)
    }
}

pub(super) fn matches_explicit_filter(source_path: &Path, explicit: &[ExplicitPath]) -> bool {
    explicit.is_empty()
        || explicit.iter().any(|path| {
            if path.is_file {
                source_path == path.path
            } else {
                source_path.starts_with(&path.path)
            }
        })
}

pub(crate) fn collect_source_files(
    root: &Path,
    languages: &LanguageRegistry,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for entry in sorted_entries(root)? {
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if matches!(
                entry.file_name().to_str(),
                Some(".git" | ".ctmpl" | "node_modules" | "target")
            ) {
                continue;
            }
            collect_source_files(&entry.path(), languages, files)?;
        } else if file_type.is_file() && languages.for_path(&entry.path()).is_some() {
            files.push(normalize_existing(&entry.path())?);
        }
    }
    Ok(())
}

fn sorted_entries(root: &Path) -> Result<Vec<fs::DirEntry>, String> {
    let mut entries = fs::read_dir(root)
        .map_err(|error| format!("Cannot read directory `{}`: {error}", root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

/// Builds a glob set the same way rule patterns are built, and skips patterns
/// that do not compile. Used to compare rules of another revision.
pub(crate) fn lenient_globs(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(false)
            .case_insensitive(cfg!(windows))
            .build()
        {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

pub(super) fn normalize_existing(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path)
        .map_err(|error| format!("Cannot resolve path `{}`: {error}", path.display()))
}

pub(super) fn display_path(root: &Path, path: &Path) -> String {
    let value = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    if let Some(value) = value.strip_prefix("//?/UNC/") {
        format!("//{value}")
    } else {
        value.strip_prefix("//?/").unwrap_or(&value).to_string()
    }
}
