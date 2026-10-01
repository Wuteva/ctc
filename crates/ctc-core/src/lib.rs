#![forbid(unsafe_code)]

pub mod canonical;
pub mod diagnostic;
pub mod discovery;
pub mod engine;
pub mod explain;
pub mod file_name;
pub mod guard;
pub mod language;
pub mod matcher;
pub mod semantic;
pub mod suppression;
pub mod template;

pub use diagnostic::{Diagnostic, DiagnosticCategory, RunReport};
pub use discovery::{DiscoveryResult, RuleApplication as DiscoveredRule, SemanticRuleApplication};
pub use engine::{
    AdHocCheckOptions, CheckOptions, CompileTemplateOptions, CoverageOptions, DiscoverRulesOptions,
    Engine, MatchTemplateOptions, ParseSourceOptions, ParsedSource,
};
pub use explain::{
    ExplainCandidate, ExplainFailure, ExplainNode, ExplainOptions, ExplainReport, ExplainStep,
    ExplainStepKind, ExplainTarget,
};
pub use guard::GuardOptions;
pub use language::{LanguageAdapter, LanguageRegistry};
pub use matcher::{MatchCount, MatchMode, SearchOptions, SearchScope};
pub use semantic::SemanticRule;
