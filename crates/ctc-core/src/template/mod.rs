use std::{collections::BTreeMap, path::Path, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{
    canonical::{CanonicalNode, CanonicalScalar},
    diagnostic::{Diagnostic, DiagnosticCategory, TextRange},
    file_name::NameCase,
    language::LanguageAdapter,
    matcher::MatchMode,
};

mod directive_scan;
mod pattern;
mod validation;

use directive_scan::scan_directive_comments;
pub use pattern::RegexPattern;
use validation::{build_filter, validate_placeholders};

/// Class member fields, the `callee` field of calls, and the `module` field of
/// imports. Adapters compute them
/// for source nodes only; template literals leave them out because they follow
/// from the written syntax.
pub const MEMBER_FIELDS: [&str; 5] = ["access", "constructor", "static", "callee", "module"];

pub fn remove_member_fields(fields: &mut BTreeMap<Arc<str>, CanonicalScalar>) {
    fields.retain(|name, _| !MEMBER_FIELDS.contains(&name.as_ref()));
}

/// Adds an `access` filter to a member placeholder unless it already filters on
/// access.
pub fn with_access_filter(placeholder: &Placeholder, access: Option<String>) -> Placeholder {
    let mut placeholder = placeholder.clone();
    let Some(access) = access else {
        return placeholder;
    };
    let has_access_filter = placeholder
        .filters
        .iter()
        .any(|filter| matches!(filter, Filter::Field { name, .. } if name == "access"));
    if !has_access_filter && !placeholder.filters.iter().any(Filter::is_name_filter) {
        placeholder.filters.push(Filter::Field {
            name: "access".to_string(),
            operator: FieldOperator::Equal,
            value: Some(CanonicalScalar::String(Arc::from(access))),
            pattern: None,
        });
    }
    placeholder
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureCategory {
    Inferred,
    Identifier,
    Expression,
    Type,
    Keyword,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cardinality {
    Scalar,
    Sequence,
    Optional,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldOperator {
    Equal,
    NotEqual,
    Exists,
    NotExists,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    Matches,
    NotMatches,
}

impl FieldOperator {
    /// True for the operators that compare a field with a number.
    pub fn is_ordering(&self) -> bool {
        matches!(
            self,
            Self::LessThan | Self::LessThanOrEqual | Self::GreaterThan | Self::GreaterThanOrEqual
        )
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Filter {
    Prefix {
        value: String,
    },
    Suffix {
        value: String,
    },
    RemovePrefix {
        value: String,
    },
    RemoveSuffix {
        value: String,
    },
    Kind {
        values: Vec<String>,
    },
    Field {
        name: String,
        operator: FieldOperator,
        value: Option<CanonicalScalar>,
        /// The compiled expression of a `matches` or `notMatches` operator.
        pattern: Option<RegexPattern>,
    },
    FileName {
        case: NameCase,
    },
    /// `matches` requires the identifier to match the pattern. `notMatches`
    /// (`negate`) requires that it does not.
    Regex {
        pattern: RegexPattern,
        negate: bool,
    },
}

impl Filter {
    /// Name filters work on identifier text. `fileName`, `matches`, and
    /// `notMatches` are name filters too, so they follow the same rules, but
    /// they only check the text and do not make the placeholder derived.
    pub fn is_name_filter(&self) -> bool {
        matches!(
            self,
            Filter::Prefix { .. }
                | Filter::Suffix { .. }
                | Filter::RemovePrefix { .. }
                | Filter::RemoveSuffix { .. }
                | Filter::FileName { .. }
                | Filter::Regex { .. }
        )
    }

    /// A check filter tests the captured text and does not derive a new name.
    pub fn is_check(&self) -> bool {
        matches!(self, Filter::FileName { .. } | Filter::Regex { .. })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placeholder {
    pub name: String,
    pub category: CaptureCategory,
    pub cardinality: Cardinality,
    pub filters: Vec<Filter>,
    pub range: TextRange,
    pub byte_range: std::ops::Range<usize>,
}

impl Placeholder {
    /// Returns the case of a `fileName` filter, if the placeholder has one.
    pub fn file_name_case(&self) -> Option<NameCase> {
        self.filters.iter().find_map(|filter| match filter {
            Filter::FileName { case } => Some(*case),
            _ => None,
        })
    }

    /// A derived use computes its expected text from another capture. A
    /// placeholder with a check filter, such as `fileName`, captures its own
    /// text instead.
    pub fn is_derived(&self) -> bool {
        !self.filters.iter().any(Filter::is_check)
            && self.filters.iter().any(Filter::is_name_filter)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TemplateNode {
    Literal {
        kind: Arc<str>,
        value: Option<CanonicalScalar>,
        fields: std::collections::BTreeMap<Arc<str>, CanonicalScalar>,
        children: Vec<TemplateNode>,
        range: TextRange,
    },
    Capture {
        placeholder: Placeholder,
    },
    Sequence {
        placeholder: Placeholder,
    },
    Derived {
        placeholder: Placeholder,
    },
    Optional {
        node: Box<TemplateNode>,
        range: TextRange,
    },
}

impl TemplateNode {
    pub fn range(&self) -> &TextRange {
        match self {
            Self::Literal { range, .. } => range,
            Self::Capture { placeholder }
            | Self::Sequence { placeholder }
            | Self::Derived { placeholder } => &placeholder.range,
            Self::Optional { range, .. } => range,
        }
    }

    pub fn from_literal(node: CanonicalNode, children: Vec<TemplateNode>) -> Self {
        Self::Literal {
            kind: node.kind,
            value: node.value,
            fields: node.fields,
            children,
            range: node.range,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompiledTemplate {
    pub path: String,
    pub language_id: &'static str,
    pub mode: MatchMode,
    pub root: TemplateNode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateDirectives {
    pub mode: MatchMode,
    pub excluded: bool,
}

pub fn parse_directives(source: &str, path: &Path) -> Result<TemplateDirectives, Vec<Diagnostic>> {
    let mut mode = None;
    let mut excluded = false;
    let mut diagnostics = Vec::new();
    let scan = scan_directive_comments(source);

    for comment in scan.line_comments {
        let text = source[comment.clone()].trim();
        let Some(value) = text.strip_prefix("// ctc:") else {
            continue;
        };
        if scan
            .first_token
            .is_some_and(|first_token| comment.start > first_token)
        {
            diagnostics.push(
                Diagnostic::new(
                    "CTC1005",
                    DiagnosticCategory::InvalidTemplateDirective,
                    "A template directive must occur before the first source token.",
                    2,
                )
                .with_template(TextRange::from_offsets(
                    path,
                    source,
                    comment.start,
                    comment.end,
                )),
            );
        } else {
            parse_directive(
                value.trim(),
                path,
                source,
                comment.start,
                comment.end,
                &mut mode,
                &mut excluded,
                &mut diagnostics,
            );
        }
    }

    if excluded && mode.is_some() {
        diagnostics.push(Diagnostic::new(
            "CTC1005",
            DiagnosticCategory::InvalidTemplateDirective,
            "An exclusion cannot also define a match mode.",
            2,
        ));
    }

    if excluded && scan.first_token.is_some() {
        diagnostics.push(Diagnostic::new(
            "CTC1005",
            DiagnosticCategory::InvalidTemplateDirective,
            "An exclusion file cannot contain a template body.",
            2,
        ));
    }

    if diagnostics.is_empty() {
        Ok(TemplateDirectives {
            mode: mode.unwrap_or_default(),
            excluded,
        })
    } else {
        Err(diagnostics)
    }
}

fn parse_directive(
    value: &str,
    path: &Path,
    source: &str,
    start: usize,
    end: usize,
    mode: &mut Option<MatchMode>,
    excluded: &mut bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let range = TextRange::from_offsets(path, source, start, end);
    match value {
        "mode=exact" | "mode=contains" | "mode=forbid" | "mode=every" => {
            if mode.is_some() {
                diagnostics.push(
                    Diagnostic::new(
                        "CTC1005",
                        DiagnosticCategory::InvalidTemplateDirective,
                        "A template can contain only one mode directive.",
                        2,
                    )
                    .with_template(range),
                );
            } else {
                *mode = Some(match value {
                    "mode=contains" => MatchMode::Contains,
                    "mode=forbid" => MatchMode::Forbid,
                    "mode=every" => MatchMode::Every,
                    _ => MatchMode::Exact,
                });
            }
        }
        "exclude" => {
            if *excluded {
                diagnostics.push(
                    Diagnostic::new(
                        "CTC1005",
                        DiagnosticCategory::InvalidTemplateDirective,
                        "An exclusion directive can occur only once.",
                        2,
                    )
                    .with_template(range),
                );
            } else {
                *excluded = true;
            }
        }
        _ => diagnostics.push(
            Diagnostic::new(
                "CTC1005",
                DiagnosticCategory::InvalidTemplateDirective,
                format!("Unknown template directive `{value}`."),
                2,
            )
            .with_template(range),
        ),
    }
}

pub fn parse_placeholders(
    source: &str,
    path: &Path,
    ranges: &[std::ops::Range<usize>],
    adapter: &dyn LanguageAdapter,
) -> Result<Vec<Placeholder>, Vec<Diagnostic>> {
    let mut placeholders = Vec::with_capacity(ranges.len());
    let mut diagnostics = Vec::new();

    for byte_range in ranges {
        let range = TextRange::from_offsets(path, source, byte_range.start, byte_range.end);
        let Some(raw) = source.get(byte_range.clone()) else {
            diagnostics.push(
                Diagnostic::new(
                    "CTC2001",
                    DiagnosticCategory::InvalidTemplateSyntax,
                    "The placeholder range is not valid UTF-8.",
                    2,
                )
                .with_template(range),
            );
            continue;
        };
        match parse_placeholder(raw, byte_range.clone(), range.clone()) {
            Ok(placeholder) => placeholders.push(placeholder),
            Err(error) => diagnostics.push(
                Diagnostic::new(error.code(), error.category(), error.message(), 2)
                    .with_template(range),
            ),
        }
    }

    if diagnostics.is_empty() {
        validate_placeholders(&placeholders, adapter)?;
        Ok(placeholders)
    } else {
        Err(diagnostics)
    }
}

enum PlaceholderParseError {
    Syntax(String),
    Filter(String),
}

impl PlaceholderParseError {
    fn code(&self) -> &'static str {
        match self {
            Self::Syntax(_) => "CTC2001",
            Self::Filter(_) => "CTC2008",
        }
    }

    fn category(&self) -> DiagnosticCategory {
        match self {
            Self::Syntax(_) => DiagnosticCategory::InvalidTemplateSyntax,
            Self::Filter(_) => DiagnosticCategory::InvalidFilter,
        }
    }

    fn message(self) -> String {
        match self {
            Self::Syntax(message) | Self::Filter(message) => message,
        }
    }
}

fn parse_placeholder(
    raw: &str,
    byte_range: std::ops::Range<usize>,
    range: TextRange,
) -> Result<Placeholder, PlaceholderParseError> {
    if !raw.starts_with("{{") || !raw.ends_with("}}") {
        return Err(PlaceholderParseError::Syntax(
            "A placeholder must start with `{{` and end with `}}`.".to_string(),
        ));
    }

    let body = &raw[2..raw.len() - 2];
    let mut parser = PlaceholderParser::new(body);
    parser.skip_whitespace();
    let cardinality = if parser.consume('*') {
        parser.skip_whitespace();
        Cardinality::Sequence
    } else if parser.consume('?') {
        parser.skip_whitespace();
        Cardinality::Optional
    } else {
        Cardinality::Scalar
    };

    let first = parser.identifier().map_err(PlaceholderParseError::Syntax)?;
    parser.skip_whitespace();
    let (category, name) = if parser.consume(':') {
        parser.skip_whitespace();
        let category = match first.as_str() {
            "identifier" => CaptureCategory::Identifier,
            "expression" => CaptureCategory::Expression,
            "type" => CaptureCategory::Type,
            "keyword" => CaptureCategory::Keyword,
            _ => {
                return Err(PlaceholderParseError::Syntax(format!(
                    "Unknown placeholder category `{first}`."
                )));
            }
        };
        (
            category,
            parser.identifier().map_err(PlaceholderParseError::Syntax)?,
        )
    } else {
        (CaptureCategory::Inferred, first)
    };

    let mut filters = Vec::new();
    loop {
        parser.skip_whitespace();
        if parser.is_end() {
            break;
        }
        if !parser.consume('|') {
            return Err(PlaceholderParseError::Syntax(
                "Expected `|` before the placeholder filter.".to_string(),
            ));
        }
        parser.skip_whitespace();
        let filter_name = parser.identifier().map_err(PlaceholderParseError::Filter)?;
        parser.skip_whitespace();
        parser.expect('(').map_err(PlaceholderParseError::Filter)?;
        let arguments = parser.arguments().map_err(PlaceholderParseError::Filter)?;
        parser.expect(')').map_err(PlaceholderParseError::Filter)?;
        filters.push(build_filter(&filter_name, arguments).map_err(PlaceholderParseError::Filter)?);
    }

    Ok(Placeholder {
        name,
        category,
        cardinality,
        filters,
        range,
        byte_range,
    })
}

struct PlaceholderParser<'a> {
    source: &'a str,
    offset: usize,
}

impl<'a> PlaceholderParser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, offset: 0 }
    }

    fn is_end(&self) -> bool {
        self.offset >= self.source.len()
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.source[self.offset..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            self.offset += character.len_utf8();
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.source[self.offset..].starts_with(expected) {
            self.offset += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(format!("Expected `{expected}` in the placeholder."))
        }
    }

    fn identifier(&mut self) -> Result<String, String> {
        let start = self.offset;
        let Some(first) = self.source[self.offset..].chars().next() else {
            return Err("Expected a placeholder identifier.".to_string());
        };
        if first != '_' && !first.is_ascii_alphabetic() {
            return Err("A placeholder identifier must start with a letter or `_`.".to_string());
        }
        self.offset += first.len_utf8();
        while let Some(character) = self.source[self.offset..].chars().next() {
            if character != '_' && !character.is_ascii_alphanumeric() {
                break;
            }
            self.offset += character.len_utf8();
        }
        Ok(self.source[start..self.offset].to_string())
    }

    fn arguments(&mut self) -> Result<Vec<serde_json::Value>, String> {
        let start = self.offset;
        let mut in_string = false;
        let mut escaped = false;
        while let Some(character) = self.source[self.offset..].chars().next() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    in_string = false;
                }
            } else if character == '"' {
                in_string = true;
            } else if character == ')' {
                let raw = self.source[start..self.offset].trim();
                if raw.is_empty() {
                    return Ok(Vec::new());
                }
                return serde_json::from_str::<Vec<serde_json::Value>>(&format!("[{raw}]"))
                    .map_err(|error| format!("Invalid filter arguments: {error}."));
            }
            self.offset += character.len_utf8();
        }
        Err("A filter call is missing `)`.".to_string())
    }
}

#[cfg(test)]
mod tests;
