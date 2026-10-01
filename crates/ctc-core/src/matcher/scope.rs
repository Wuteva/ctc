use serde::{Deserialize, Serialize};

use crate::{canonical::CanonicalNode, diagnostic::TextRange};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchMode {
    #[default]
    Exact,
    Contains,
    Forbid,
    Every,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchScope {
    #[default]
    TopLevel,
    Descendants,
    FunctionBody,
    ClassBody,
    /// The nodes of the kinds listed in `SearchOptions::inside` and everything
    /// below them. Configuration selects it with an object, never a string.
    #[serde(skip_deserializing)]
    Inside,
}

/// The allowed number of matches for a contains rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchCount {
    pub min: u32,
    pub max: Option<u32>,
}

impl MatchCount {
    /// True when a run must count every match instead of stopping at the first.
    pub(super) fn needs_full_count(self) -> bool {
        self.min > 1 || self.max.is_some()
    }
}

/// Rule settings that go beyond the template, the mode, and the scope name.
#[derive(Clone, Copy, Debug, Default)]
pub struct SearchOptions<'a> {
    /// The node kinds that the `Inside` scope searches.
    pub inside: &'a [String],
    /// Skip function bodies nested below an `Inside` node.
    pub stop_at_functions: bool,
    pub count: Option<MatchCount>,
}

pub(super) struct CandidateList<'a> {
    pub(super) nodes: &'a [CanonicalNode],
    pub(super) fallback: &'a TextRange,
}

pub(super) fn collect_candidate_lists<'a>(
    source: &'a CanonicalNode,
    scope: SearchScope,
    options: &SearchOptions<'_>,
) -> Vec<CandidateList<'a>> {
    let mut lists = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    match scope {
        SearchScope::TopLevel => push_candidate_list(source, &mut lists, &mut seen),
        SearchScope::Descendants => collect_all_lists(source, &mut lists, &mut seen),
        SearchScope::FunctionBody => {
            collect_scoped_roots(source, is_function_like, &mut |function| {
                if let Some(body) = function
                    .children
                    .iter()
                    .find(|child| child.kind.as_ref() == "StatementBlock")
                {
                    collect_all_lists(body, &mut lists, &mut seen);
                }
            });
        }
        SearchScope::ClassBody => {
            collect_scoped_roots(
                source,
                |node| node.kind.as_ref() == "ClassBody",
                &mut |body| collect_all_lists(body, &mut lists, &mut seen),
            );
        }
        SearchScope::Inside => {
            collect_scoped_roots(
                source,
                |node| options.inside.iter().any(|kind| kind == node.kind.as_ref()),
                &mut |region| {
                    if options.stop_at_functions {
                        collect_lists_outside_functions(region, &mut lists, &mut seen);
                    } else {
                        collect_all_lists(region, &mut lists, &mut seen);
                    }
                },
            );
        }
    }
    lists
}

/// Like `collect_all_lists`, but does not enter function nodes below `node`.
/// The function nodes themselves stay in the lists of their parents.
fn collect_lists_outside_functions<'a>(
    node: &'a CanonicalNode,
    lists: &mut Vec<CandidateList<'a>>,
    seen: &mut std::collections::BTreeSet<usize>,
) {
    push_candidate_list(node, lists, seen);
    for child in &node.children {
        if !is_function_like(child) {
            collect_lists_outside_functions(child, lists, seen);
        }
    }
}

fn collect_all_lists<'a>(
    node: &'a CanonicalNode,
    lists: &mut Vec<CandidateList<'a>>,
    seen: &mut std::collections::BTreeSet<usize>,
) {
    push_candidate_list(node, lists, seen);
    for child in &node.children {
        collect_all_lists(child, lists, seen);
    }
}

fn push_candidate_list<'a>(
    node: &'a CanonicalNode,
    lists: &mut Vec<CandidateList<'a>>,
    seen: &mut std::collections::BTreeSet<usize>,
) {
    if node.children.is_empty() {
        return;
    }
    let key = std::ptr::from_ref(node) as usize;
    if seen.insert(key) {
        lists.push(CandidateList {
            nodes: &node.children,
            fallback: &node.range,
        });
    }
}

fn collect_scoped_roots<'a>(
    node: &'a CanonicalNode,
    predicate: impl Fn(&CanonicalNode) -> bool + Copy,
    visitor: &mut impl FnMut(&'a CanonicalNode),
) {
    if predicate(node) {
        visitor(node);
    }
    for child in &node.children {
        collect_scoped_roots(child, predicate, visitor);
    }
}

fn is_function_like(node: &CanonicalNode) -> bool {
    matches!(
        node.kind.as_ref(),
        "FunctionDeclaration"
            | "FunctionExpression"
            | "ArrowFunction"
            | "MethodDefinition"
            | "GeneratorFunction"
            | "GeneratorFunctionDeclaration"
    )
}
