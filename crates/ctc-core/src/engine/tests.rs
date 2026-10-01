use std::path::Path;

use super::combine_alternatives;
use crate::{
    diagnostic::{Diagnostic, DiagnosticCategory, TextRange},
    matcher::{MatchMode, MatchResult},
};

fn failure_at(offset: usize) -> Diagnostic {
    Diagnostic::new("CTC3002", DiagnosticCategory::MissingSyntaxNode, "x", 1).with_source(
        TextRange::from_offsets(Path::new("a.ts"), &"x".repeat(100), offset, offset),
    )
}

fn result(offsets: &[usize]) -> MatchResult {
    MatchResult {
        matches: offsets.is_empty(),
        diagnostics: offsets.iter().map(|offset| failure_at(*offset)).collect(),
    }
}

fn offsets(result: &MatchResult) -> Vec<u64> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.source.as_ref().unwrap().start.offset)
        .collect()
}

#[test]
fn every_fails_only_the_candidates_that_all_templates_fail() {
    let combined = combine_alternatives(
        MatchMode::Every,
        vec![
            result(&[10, 20, 30]),
            result(&[20, 30, 40]),
            result(&[30, 20]),
        ],
    );
    assert_eq!(offsets(&combined), vec![20, 30]);
    assert!(!combined.matches);
}

#[test]
fn every_passes_when_one_template_accepts_all_candidates() {
    let combined = combine_alternatives(MatchMode::Every, vec![result(&[10, 20]), result(&[])]);
    assert!(combined.matches);
    assert!(combined.diagnostics.is_empty());
}

#[test]
fn exact_keeps_the_failure_that_reached_furthest_and_the_first_on_a_tie() {
    let combined = combine_alternatives(
        MatchMode::Exact,
        vec![result(&[5]), result(&[40]), result(&[40])],
    );
    assert_eq!(offsets(&combined), vec![40]);
    let combined = combine_alternatives(MatchMode::Contains, vec![result(&[7]), result(&[7])]);
    assert_eq!(offsets(&combined), vec![7]);
}

#[test]
fn forbid_reports_every_match() {
    let combined = combine_alternatives(MatchMode::Forbid, vec![result(&[3]), result(&[9, 1])]);
    assert_eq!(offsets(&combined), vec![3, 9, 1]);
}

#[test]
fn errors_win_over_mismatches() {
    let error = MatchResult {
        matches: false,
        diagnostics: vec![Diagnostic::new(
            "CTC9001",
            DiagnosticCategory::InternalToolError,
            "broken",
            2,
        )],
    };
    let combined = combine_alternatives(MatchMode::Exact, vec![result(&[5]), error]);
    assert_eq!(combined.diagnostics[0].code, "CTC9001");
}
