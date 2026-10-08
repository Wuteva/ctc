use std::{cmp::Ordering, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPosition {
    pub offset: u64,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub path: String,
    pub start: TextPosition,
    pub end: TextPosition,
}

impl TextRange {
    pub fn from_offsets(path: &Path, source: &str, start: usize, end: usize) -> Self {
        Self {
            path: normalized_path(path),
            start: position_at(source, start),
            end: position_at(source, end),
        }
    }
}

/// Line starts of one source text, so that many positions can be computed
/// without scanning the text again. `TextRange::from_offsets` scans from the
/// start of the text on every call, which is too slow for every node of a
/// large file.
#[derive(Clone, Debug)]
pub struct LineIndex<'a> {
    path: String,
    source: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub fn new(path: &Path, source: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index + 1),
        );
        Self {
            path: normalized_path(path),
            source,
            line_starts,
        }
    }

    pub fn range(&self, start: usize, end: usize) -> TextRange {
        TextRange {
            path: self.path.clone(),
            start: self.position(start),
            end: self.position(end),
        }
    }

    fn position(&self, offset: usize) -> TextPosition {
        let offset = offset.min(self.source.len());
        let line = self.line_starts.partition_point(|start| *start <= offset);
        let line_start = self.line_starts[line - 1];
        TextPosition {
            offset: offset as u64,
            line: line as u32,
            column: self.source[line_start..offset].chars().count() as u32 + 1,
        }
    }
}

fn normalized_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if let Some(value) = value.strip_prefix("//?/UNC/") {
        format!("//{value}")
    } else {
        value.strip_prefix("//?/").unwrap_or(&value).to_string()
    }
}

fn position_at(source: &str, offset: usize) -> TextPosition {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = source[line_start..offset].chars().count() as u32 + 1;
    TextPosition {
        offset: offset as u64,
        line,
        column,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCategory {
    InvalidCommandLine,
    InvalidConfiguration,
    InvalidTemplateFilename,
    ConfiguredRuleSelectsNoFiles,
    InvalidTemplateDirective,
    NoSelectedSourceFiles,
    NoApplicableRule,
    UnsupportedSourceType,
    InvalidUtf8,
    InvalidTemplateSyntax,
    UnsupportedPlaceholderContext,
    InvalidNodeKind,
    InvalidCanonicalField,
    DerivedSourceMissing,
    PlaceholderCategoryMismatch,
    AmbiguousSequence,
    InvalidFilter,
    TemplateComplexity,
    EmptySearchTemplate,
    InvalidEveryTemplate,
    SourceParseError,
    MissingSyntaxNode,
    UnexpectedSyntaxNode,
    CapturedValueMismatch,
    DerivedIdentifierMismatch,
    FileNameMismatch,
    IdentifierPatternMismatch,
    TooFewMatches,
    TooManyMatches,
    ForbiddenStructure,
    ReturnPathFallthrough,
    ReturnPathBareReturn,
    ForbiddenTry,
    ForbiddenThrow,
    PromiseRejection,
    ExceptionSourceCall,
    UndeclaredGlobalRead,
    GlobalAssignment,
    RestrictedGlobal,
    DynamicRequire,
    DynamicGlobalAccess,
    InvalidSuppression,
    SuppressionNotAllowed,
    UncoveredSourceFile,
    SuppressionAdded,
    RuleSelectionReduced,
    RuleRemovedOrWeakened,
    RuleDefinitionChanged,
    MissingPartnerFile,
    FileTooLong,
    MissingMemberDefinition,
    MemberDefinitionOrder,
    InternalToolError,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub category: DiagnosticCategory,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<TextRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<TextRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    #[serde(skip)]
    pub exit_class: u8,
}

impl Diagnostic {
    pub fn new(
        code: impl Into<String>,
        category: DiagnosticCategory,
        message: impl Into<String>,
        exit_class: u8,
    ) -> Self {
        Self {
            code: code.into(),
            category,
            message: message.into(),
            rule_id: None,
            rule_message: None,
            source: None,
            template: None,
            expected: None,
            actual: None,
            exit_class,
        }
    }

    pub fn with_rule(mut self, rule_id: impl Into<String>) -> Self {
        self.rule_id = Some(rule_id.into());
        self
    }

    pub fn with_rule_message(mut self, message: impl Into<String>) -> Self {
        self.rule_message = Some(message.into());
        self
    }

    /// A rule failure is a source-file mismatch reported by a rule. Command-line,
    /// configuration, template, and internal diagnostics are not rule failures.
    pub fn is_rule_failure(&self) -> bool {
        self.exit_class == 1
            && self.rule_id.is_some()
            && (self.code.starts_with("CTC3") || self.code.starts_with("CTC4"))
    }

    pub fn with_source(mut self, range: TextRange) -> Self {
        self.source = Some(range);
        self
    }

    pub fn with_template(mut self, range: TextRange) -> Self {
        self.template = Some(range);
        self
    }

    pub fn with_values(mut self, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self.actual = Some(actual.into());
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub schema_version: u32,
    pub matches: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl RunReport {
    pub fn new(mut diagnostics: Vec<Diagnostic>) -> Self {
        diagnostics.sort_by(compare_diagnostics);
        Self {
            schema_version: 1,
            matches: diagnostics.is_empty(),
            diagnostics,
        }
    }

    pub fn exit_code(&self) -> u8 {
        self.diagnostics
            .iter()
            .map(|diagnostic| diagnostic.exit_class)
            .max()
            .unwrap_or(0)
    }
}

fn compare_diagnostics(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    let left_source = left.source.as_ref();
    let right_source = right.source.as_ref();
    left_source
        .map(|range| &range.path)
        .cmp(&right_source.map(|range| &range.path))
        .then_with(|| left.rule_id.cmp(&right.rule_id))
        .then_with(|| {
            left_source
                .map(|range| range.start.offset)
                .cmp(&right_source.map(|range| range.start.offset))
        })
        .then_with(|| left.code.cmp(&right.code))
        .then_with(|| {
            left.template
                .as_ref()
                .map(|range| &range.path)
                .cmp(&right.template.as_ref().map(|range| &range.path))
        })
        .then_with(|| {
            left.template
                .as_ref()
                .map(|range| range.start.offset)
                .cmp(&right.template.as_ref().map(|range| range.start.offset))
        })
}

#[cfg(test)]
mod tests;
