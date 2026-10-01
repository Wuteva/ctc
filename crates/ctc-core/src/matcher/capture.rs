use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

use crate::{
    canonical::CanonicalNode,
    template::{Filter, Placeholder, TemplateNode},
};

use super::{
    filters::{collect_derived_filters, collect_identifiers, invert_name_filters},
    trace::{TraceLink, TraceStep},
};

#[derive(Clone, Debug)]
pub(super) enum CaptureValue {
    Scalar(CanonicalNode),
    Sequence(Vec<CanonicalNode>),
}

#[derive(Clone, Debug, Default)]
pub(super) struct CaptureEnvironment {
    pub(super) values: BTreeMap<String, CaptureValue>,
    pub(super) pending_derived: BTreeMap<String, Vec<PendingDerived>>,
    // Only set when tracing. It is not part of the fingerprint.
    pub(super) trace: Option<Arc<TraceLink>>,
}

impl CaptureEnvironment {
    pub(super) fn push_step(&mut self, step: TraceStep) {
        let parent = self.trace.take();
        self.trace = Some(Arc::new(TraceLink { step, parent }));
    }

    pub(super) fn fingerprint(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for (name, value) in &self.values {
            name.hash(&mut hasher);
            match value {
                CaptureValue::Scalar(node) => {
                    0_u8.hash(&mut hasher);
                    hash_node(node, &mut hasher);
                }
                CaptureValue::Sequence(nodes) => {
                    1_u8.hash(&mut hasher);
                    nodes.len().hash(&mut hasher);
                    for node in nodes {
                        hash_node(node, &mut hasher);
                    }
                }
            }
        }
        for (name, pending) in &self.pending_derived {
            name.hash(&mut hasher);
            pending.len().hash(&mut hasher);
            for constraint in pending {
                constraint.actual.hash(&mut hasher);
                constraint.placeholder.filters.hash(&mut hasher);
                constraint.placeholder.range.start.offset.hash(&mut hasher);
                hash_node(&constraint.source, &mut hasher);
            }
        }
        hasher.finish()
    }
}

#[derive(Clone, Debug)]
pub(super) struct PendingDerived {
    pub(super) actual: String,
    pub(super) source: CanonicalNode,
    pub(super) placeholder: Placeholder,
}

fn hash_node(node: &CanonicalNode, hasher: &mut impl Hasher) {
    node.kind.hash(hasher);
    node.value.hash(hasher);
    node.fields.hash(hasher);
    node.children.len().hash(hasher);
    for child in &node.children {
        hash_node(child, hasher);
    }
}

#[derive(Clone, Debug)]
pub(super) struct MatchSuccess {
    pub(super) environment: CaptureEnvironment,
    pub(super) matched_nodes: usize,
    pub(super) consumed_nodes: usize,
}

pub(super) struct MatchContext {
    pub(super) states: usize,
    pub(super) candidate_start: usize,
    pub(super) derived_hints: DerivedHints,
    pub(super) tracing: bool,
    pub(super) depth: usize,
}

#[derive(Clone, Debug, Default)]
pub(super) struct DerivedHints {
    candidates: BTreeMap<String, FilterCandidates>,
}

type FilterCandidates = Vec<(Vec<Filter>, BTreeSet<String>)>;

impl DerivedHints {
    pub(super) fn from_trees(
        template: &TemplateNode,
        source: &CanonicalNode,
        validate_identifier: &dyn Fn(&str) -> bool,
    ) -> Self {
        let mut filters = BTreeMap::<String, Vec<Vec<Filter>>>::new();
        collect_derived_filters(template, &mut filters);
        let mut identifiers = Vec::new();
        collect_identifiers(source, &mut identifiers);

        let mut candidates = BTreeMap::new();
        for (name, filter_sets) in filters {
            let values = filter_sets
                .into_iter()
                .map(|filters| {
                    let matches = identifiers
                        .iter()
                        .filter_map(|actual| invert_name_filters(actual, &filters))
                        .filter(|base| validate_identifier(base))
                        .collect::<BTreeSet<_>>();
                    (filters, matches)
                })
                .collect();
            candidates.insert(name, values);
        }
        Self { candidates }
    }

    pub(super) fn for_other_filters(&self, name: &str, current: &[Filter]) -> Option<String> {
        let values = self.candidates.get(name)?;
        let candidates = values
            .iter()
            .filter(|(filters, _)| filters != current)
            .flat_map(|(_, candidates)| candidates.iter().cloned())
            .collect::<BTreeSet<_>>();
        if candidates.len() == 1 {
            candidates.into_iter().next()
        } else {
            None
        }
    }
}
