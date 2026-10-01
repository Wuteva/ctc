use std::path::Path;

use ctc_core::{
    language::LanguageAdapter,
    matcher::{MatchMode, SearchScope, match_template, match_template_in_scope},
    template::{CompiledTemplate, parse_placeholders},
};

use crate::RustAdapter;

fn compile(adapter: &RustAdapter, source: &str, mode: MatchMode) -> CompiledTemplate {
    let path = Path::new("rule.rs.ctmpl");
    let ranges = adapter.scan_placeholders(source, path).unwrap();
    let placeholders = parse_placeholders(source, path, &ranges, adapter).unwrap();
    adapter
        .compile_template(source, path, &placeholders, mode)
        .unwrap()
}

#[test]
fn compiles_all_supported_sequence_slots() {
    let source = r#"
{{* Attributes }}
pub mod {{ ModuleName }} {
{{* Items }}
}

struct {{ ItemName }} {
{{* Fields }}
}

enum {{ ItemName | suffix("Kind") }} {
{{* Variants }}
}

fn {{ identifier:Factory }}<{{* GenericParameters }}>({{* Parameters }}) -> Result<{{ type:ItemType }}, {{ type:ErrorType }}> {
{{* Statements }}
}

let _ = {{ Factory }}::<{{* GenericArguments }}>({{* Arguments }});

use std::{ {{* Imports }} };

match value {
{{* Arms }}
}
"#;
    let adapter = RustAdapter::new();
    let template = compile(&adapter, source, MatchMode::Exact);
    assert!(matches!(
        template.root,
        ctc_core::template::TemplateNode::Literal { .. }
    ));
}

#[test]
fn class_like_rust_template_matches_source() {
    let template = compile(
        &RustAdapter::new(),
        r#"
pub struct {{ Name }} {
{{* Fields }}
}

impl {{ Name }} {
{{* Items }}
}
"#,
        MatchMode::Exact,
    );
    let adapter = RustAdapter::new();
    let parsed = adapter
        .parse(
            "pub struct Widget {\n    value: usize,\n}\n\nimpl Widget {\n    pub fn new(value: usize) -> Self {\n        Self { value }\n    }\n}\n",
            Path::new("source.rs"),
        )
        .unwrap();
    let result = match_template("shape", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn optional_keywords_match_present_and_absent_forms() {
    let adapter = RustAdapter::new();
    for (template_source, sources) in [
        (
            "{{? keyword:pub }} fn build() {}\n",
            ["pub fn build() {}\n", "fn build() {}\n"],
        ),
        (
            "{{? keyword:async }} fn build() {}\n",
            ["async fn build() {}\n", "fn build() {}\n"],
        ),
        (
            "{{? keyword:const }} fn build() {}\n",
            ["const fn build() {}\n", "fn build() {}\n"],
        ),
        (
            "{{? keyword:unsafe }} fn build() {}\n",
            ["unsafe fn build() {}\n", "fn build() {}\n"],
        ),
    ] {
        let template = compile(&adapter, template_source, MatchMode::Exact);
        for source in sources {
            let parsed = adapter.parse(source, Path::new("source.rs")).unwrap();
            let result = match_template("keywords", &template, &parsed.root, &|value| {
                adapter.validate_identifier(value)
            });
            assert!(result.matches, "{source}: {:?}", result.diagnostics);
        }
    }
}

#[test]
fn standalone_placeholder_matches_any_item() {
    let adapter = RustAdapter::new();
    let template = compile(&adapter, "{{ Item }}\n", MatchMode::Exact);
    let parsed = adapter
        .parse("const VALUE: usize = 1;\n", Path::new("source.rs"))
        .unwrap();
    let result = match_template("one-item", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn sequence_placeholder_reports_unsupported_context() {
    let adapter = RustAdapter::new();
    let path = Path::new("rule.rs.ctmpl");
    let source = "let value = [{{* Values }}];\n";
    let ranges = adapter.scan_placeholders(source, path).unwrap();
    let placeholders = parse_placeholders(source, path, &ranges, &adapter).unwrap();
    let diagnostics = adapter
        .compile_template(source, path, &placeholders, MatchMode::Exact)
        .unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "CTC2002")
    );
}

#[test]
fn function_body_scope_finds_nested_calls() {
    let adapter = RustAdapter::new();
    let template = compile(
        &adapter,
        r#"{{ Call | kind("CallExpression") | field("callee", "equal", "unwrap") }}"#,
        MatchMode::Forbid,
    );
    let parsed = adapter
        .parse(
            "const VALUE: usize = 1;\nfn run(value: Option<usize>) { value.unwrap(); }\n",
            Path::new("source.rs"),
        )
        .unwrap();
    let result = match_template_in_scope(
        "no-unwrap",
        &template,
        &parsed.root,
        SearchScope::FunctionBody,
        &|value| adapter.validate_identifier(value),
    );
    assert!(!result.matches);
    assert_eq!(result.diagnostics.len(), 1);
}
