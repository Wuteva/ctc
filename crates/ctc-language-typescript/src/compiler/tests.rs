use std::path::Path;

use ctc_core::{
    language::LanguageAdapter,
    matcher::{
        MatchMode, SearchScope, match_template, match_template_in_scope,
        match_template_in_scope_with_kinds,
    },
    template::{TemplateNode, parse_directives, parse_placeholders},
};

use crate::TypeScriptAdapter;

#[test]
fn compiles_all_planned_placeholder_slots() {
    let source = r#"
{{* Imports | kind("ImportDeclaration") }}
export interface {{ Name | suffix("Options") }} {
  {{* Members }}
}
export interface {{ Name }} {
  {{* PublicMembers }}
}
class {{ Name | suffix("Implementation") }} implements {{ Name }} {
  {{* ClassMembers }}
}
export function {{ identifier:Factory }}({{* Parameters }}): {{ type:ReturnType }} {
  {{* Statements }}
}
const value = {{ Factory }}({{* Arguments }});
"#;
    let adapter = TypeScriptAdapter::new();
    let ranges = adapter
        .scan_placeholders(source, Path::new("rule.ts.ctmpl"))
        .unwrap();
    let placeholders =
        parse_placeholders(source, Path::new("rule.ts.ctmpl"), &ranges, &adapter).unwrap();
    let directives = parse_directives(source, Path::new("rule.ts.ctmpl")).unwrap();
    let template = adapter
        .compile_template(
            source,
            Path::new("rule.ts.ctmpl"),
            &placeholders,
            directives.mode,
        )
        .unwrap();
    assert!(matches!(template.root, TemplateNode::Literal { .. }));
}

#[test]
fn compiles_forbidden_import_template() {
    let source = r#"// ctc: mode=forbid
{{ Import | kind("ImportDeclaration", "ImportEqualsDeclaration") | field("typeOnly", "notEqual", true) }}
"#;
    let adapter = TypeScriptAdapter::new();
    let ranges = adapter
        .scan_placeholders(source, Path::new("rule.ts.ctmpl"))
        .unwrap();
    let placeholders =
        parse_placeholders(source, Path::new("rule.ts.ctmpl"), &ranges, &adapter).unwrap();
    let template = adapter
        .compile_template(
            source,
            Path::new("rule.ts.ctmpl"),
            &placeholders,
            MatchMode::Forbid,
        )
        .unwrap();
    assert_eq!(template.mode, MatchMode::Forbid);
}

fn compile(
    adapter: &TypeScriptAdapter,
    source: &str,
    mode: MatchMode,
) -> ctc_core::template::CompiledTemplate {
    let path = Path::new("rule.ts.ctmpl");
    let ranges = adapter.scan_placeholders(source, path).unwrap();
    let placeholders = parse_placeholders(source, path, &ranges, adapter).unwrap();
    adapter
        .compile_template(source, path, &placeholders, mode)
        .unwrap()
}

#[test]
fn class_factory_template_matches_related_names() {
    let template_source = r#"
{{* Imports | kind("ImportDeclaration") }}
export interface {{ PublicName | suffix("Options") }} {
  {{* OptionMembers }}
}
export interface {{ PublicName }} {
  {{* PublicMembers }}
}
class {{ PublicName | suffix("Implementation") }} implements {{ PublicName }} {
  {{* ImplementationMembers }}
}
export const {{ PublicName | prefix("create") }} = (
  {{* FactoryParameters }}
): {{ PublicName }} =>
  new {{ PublicName | suffix("Implementation") }}(
{{* ConstructorArguments }}
  );
"#;
    let source = r#"
export interface UserOptions {
  name: string;
}
export interface User {
  readonly name: string;
}
class UserImplementation implements User {
  constructor(public readonly name: string) {}
}
export const createUser = (options: UserOptions): User =>
  new UserImplementation(options.name);
"#;
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, template_source, MatchMode::Exact);
    let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
    let result = match_template("class-factory", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn class_factory_template_rejects_wrong_factory_name() {
    let template_source = r#"
export interface {{ Name }} {}
class {{ Name | suffix("Implementation") }} implements {{ Name }} {}
export const {{ Name | prefix("create") }} = (): {{ Name }} =>
  new {{ Name | suffix("Implementation") }}();
"#;
    let source = r#"
export interface User {}
class UserImplementation implements User {}
export const makeUser = (): User => new UserImplementation();
"#;
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, template_source, MatchMode::Exact);
    let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
    let result = match_template("class-factory", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(!result.matches);
    assert_eq!(result.diagnostics[0].code, "CTC3005");
}

#[test]
fn file_name_filter_runs_earlier_filters_on_the_captured_name() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(
        &adapter,
        r#"export interface {{ Name | removePrefix("I") | fileName("PascalCase") }} {}
export class {{ Name | removePrefix("I") | suffix("Impl") }} implements {{ Name }} {}
"#,
        MatchMode::Exact,
    );
    let source = "export interface ILogger {}\nexport class LoggerImpl implements ILogger {}\n";
    let check = |path: &str| {
        let parsed = adapter.parse(source, Path::new(path)).unwrap();
        match_template("file-name", &template, &parsed.root, &|value| {
            adapter.validate_identifier(value)
        })
    };
    let result = check("src/logger.ts");
    assert!(result.matches, "{:?}", result.diagnostics);

    let result = check("src/console-logger.ts");
    assert!(!result.matches);
    let diagnostic = &result.diagnostics[0];
    assert_eq!(diagnostic.code, "CTC3007");
    assert_eq!(diagnostic.expected.as_deref(), Some("IConsoleLogger"));
    assert_eq!(diagnostic.actual.as_deref(), Some("ILogger"));
    assert_eq!(
        diagnostic.message,
        "Capture `Name` must match the file name `console-logger.ts`. The file name in PascalCase is `ConsoleLogger`."
    );
}

const REMOVE_PREFIX_TEMPLATE: &str = r#"
export interface {{ InterfaceName }} {}
class {{ ClassName }} implements {{ InterfaceName }} {}
export const {{ InterfaceName | removePrefix("I") | prefix("create") }} = (): {{ InterfaceName }} => {
  return new {{ ClassName }}();
};
"#;

fn match_remove_prefix_template(source: &str) -> ctc_core::matcher::MatchResult {
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, REMOVE_PREFIX_TEMPLATE, MatchMode::Exact);
    let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
    match_template("class-factory", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    })
}

#[test]
fn remove_prefix_filter_derives_factory_name_from_interface() {
    let result = match_remove_prefix_template(
        r#"
export interface ILogger {}
class WinstonLogger implements ILogger {}
export const createLogger = (): ILogger => {
  return new WinstonLogger();
};
"#,
    );
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn remove_prefix_filter_rejects_wrong_factory_name() {
    let result = match_remove_prefix_template(
        r#"
export interface ILogger {}
class WinstonLogger implements ILogger {}
export const createILogger = (): ILogger => {
  return new WinstonLogger();
};
"#,
    );
    assert!(!result.matches);
    assert_eq!(result.diagnostics[0].code, "CTC3005");
}

#[test]
fn remove_prefix_filter_rejects_missing_prefix() {
    let result = match_remove_prefix_template(
        r#"
export interface Logger {}
class WinstonLogger implements Logger {}
export const createLogger = (): Logger => {
  return new WinstonLogger();
};
"#,
    );
    assert!(!result.matches);
    assert_eq!(result.diagnostics[0].code, "CTC3005");
}

#[test]
fn remove_suffix_filter_derives_base_name() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(
        &adapter,
        r#"
export interface {{ Name }} {}
export const {{ Name | removeSuffix("Service") | prefix("make") }} = 1;
"#,
        MatchMode::Exact,
    );
    let parsed = adapter
        .parse(
            "export interface UserService {}\nexport const makeUser = 1;\n",
            Path::new("source.ts"),
        )
        .unwrap();
    let result = match_template("class-factory", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn forbid_template_reports_only_value_imports() {
    let template_source = r#"// ctc: mode=forbid
{{ Import | kind("ImportDeclaration", "ImportEqualsDeclaration") | field("typeOnly", "notEqual", true) }}
"#;
    let source = r#"
import type { ILogger } from "./types";
import /* gap */ type { Other } from "./types";
import { Value } from "./value";
import "./register";
import type from "./value";
import { type OnlyType } from "./types";
"#;
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, template_source, MatchMode::Forbid);
    let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
    let result = match_template("no-value-imports", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(!result.matches);
    assert_eq!(result.diagnostics.len(), 4);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == "CTC3006")
    );
}

#[test]
fn contains_mode_matches_a_top_level_statement_region() {
    let template_source = r#"// ctc: mode=contains
export function {{ identifier:Name }}({{* Parameters }}): {{ type:ReturnType }} {
  {{* Statements }}
}
"#;
    let source = r#"
import type { Handler } from "./types";
export function handle(value: string): void {
  console.log(value);
}
export const version = 1;
"#;
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, template_source, MatchMode::Contains);
    let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
    let result = match_template("exported-function", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn exact_mode_rejects_an_extra_statement() {
    let template_source = "export const value = 1;";
    let source = "export const value = 1;\nexport const extra = 2;";
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, template_source, MatchMode::Exact);
    let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
    let result = match_template("exact", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(!result.matches);
    assert_eq!(result.diagnostics[0].code, "CTC3003");
}

#[test]
fn formatting_comments_and_trailing_punctuation_do_not_change_matching() {
    let template_source = "export const values = [1, 2];";
    let source = "export /* note */ const values = [1, 2,]\n";
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, template_source, MatchMode::Exact);
    let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
    let result = match_template("formatting", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn byte_order_mark_does_not_change_matching() {
    let template_source = "export const value = 1;";
    let source = "\u{feff}export const value = 1;";
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, template_source, MatchMode::Exact);
    let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
    let result = match_template("bom", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn rejects_search_template_that_can_match_empty_source() {
    let source = "// ctc: mode=contains\n{{* Statements }}";
    let adapter = TypeScriptAdapter::new();
    let path = Path::new("rule.ts.ctmpl");
    let ranges = adapter.scan_placeholders(source, path).unwrap();
    let placeholders = parse_placeholders(source, path, &ranges, &adapter).unwrap();
    let diagnostics = adapter
        .compile_template(source, path, &placeholders, MatchMode::Contains)
        .unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC2010");
}

#[test]
fn one_top_level_placeholder_keeps_the_source_file_root() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, "{{ Statement }}", MatchMode::Exact);
    let parsed = adapter
        .parse("const value = 1;", Path::new("source.ts"))
        .unwrap();
    let result = match_template("one-statement", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn rejects_expression_placeholder_in_declaration_name() {
    let source = "export function {{ expression:Name }}() {}";
    let adapter = TypeScriptAdapter::new();
    let path = Path::new("rule.ts.ctmpl");
    let ranges = adapter.scan_placeholders(source, path).unwrap();
    let placeholders = parse_placeholders(source, path, &ranges, &adapter).unwrap();
    let diagnostics = adapter
        .compile_template(source, path, &placeholders, MatchMode::Exact)
        .unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC2002");
}

#[test]
fn rejects_naming_filter_for_expression_capture() {
    let source = r#"
const first = {{ expression:Name }};
const second = {{ Name | prefix("make") }};
"#;
    let adapter = TypeScriptAdapter::new();
    let path = Path::new("rule.ts.ctmpl");
    let ranges = adapter.scan_placeholders(source, path).unwrap();
    let placeholders = parse_placeholders(source, path, &ranges, &adapter).unwrap();
    let diagnostics = adapter
        .compile_template(source, path, &placeholders, MatchMode::Exact)
        .unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC2008");
}

#[test]
fn rejects_conflicting_explicit_capture_categories_after_inference() {
    let source = r#"
const first = {{ Name }};
const second = {{ expression:Name }};
type Result = {{ type:Name }};
"#;
    let adapter = TypeScriptAdapter::new();
    let path = Path::new("rule.ts.ctmpl");
    let ranges = adapter.scan_placeholders(source, path).unwrap();
    let diagnostics = parse_placeholders(source, path, &ranges, &adapter).unwrap_err();
    assert_eq!(diagnostics[0].code, "CTC2006");
}

#[test]
fn failed_contains_reports_the_candidate_with_more_matched_nodes() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, "expected;", MatchMode::Contains);
    let parsed = adapter
        .parse("actual;\nclass Later {}", Path::new("source.ts"))
        .unwrap();
    let result = match_template("contains", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(!result.matches);
    assert_eq!(
        result.diagnostics[0].source.as_ref().unwrap().start.offset,
        0
    );
}

#[test]
fn forward_derived_mismatch_reports_exact_expected_identifier() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(
        &adapter,
        r#"
export interface {{ Name | suffix("Options") }} {}
export interface {{ Name }} {}
"#,
        MatchMode::Exact,
    );
    let parsed = adapter
        .parse(
            r#"
export interface WrongOptions {}
export interface Widget {}
"#,
            Path::new("source.ts"),
        )
        .unwrap();
    let result = match_template("derived", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(!result.matches);
    assert_eq!(result.diagnostics[0].code, "CTC3005");
    assert_eq!(
        result.diagnostics[0].expected.as_deref(),
        Some("WidgetOptions")
    );
    assert_eq!(
        result.diagnostics[0].actual.as_deref(),
        Some("WrongOptions")
    );
}

#[test]
fn later_derived_occurrence_can_hint_the_expected_identifier() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(
        &adapter,
        r#"
export interface {{ Name | suffix("Options") }} {}
export interface {{ Name }} {}
export const {{ Name | prefix("create") }} = (): {{ Name }} => null;
"#,
        MatchMode::Exact,
    );
    let parsed = adapter
        .parse(
            r#"
export interface IWidget {}
export class Widget {}
export const createWidget = () => null;
"#,
            Path::new("source.ts"),
        )
        .unwrap();
    let result = match_template("derived", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(!result.matches);
    assert_eq!(result.diagnostics[0].code, "CTC3005");
    assert_eq!(
        result.diagnostics[0].expected.as_deref(),
        Some("WidgetOptions")
    );
    assert_eq!(result.diagnostics[0].actual.as_deref(), Some("IWidget"));
}

#[test]
fn search_scopes_select_different_syntax_regions() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(
        &adapter,
        r#"{{ Forbidden | kind("TryStatement") }}"#,
        MatchMode::Forbid,
    );
    let parsed = adapter
        .parse(
            r#"
try {} catch {}

function run() {
  try {} catch {}
}

class Example {
  method() {
try {} catch {}
  }
}
"#,
            Path::new("source.ts"),
        )
        .unwrap();

    let match_scope = |scope| {
        match_template_in_scope("no-try", &template, &parsed.root, scope, &|value| {
            adapter.validate_identifier(value)
        })
    };

    assert_eq!(match_scope(SearchScope::TopLevel).diagnostics.len(), 1);
    assert_eq!(match_scope(SearchScope::Descendants).diagnostics.len(), 3);
    assert_eq!(match_scope(SearchScope::FunctionBody).diagnostics.len(), 2);
    assert_eq!(match_scope(SearchScope::ClassBody).diagnostics.len(), 1);
}

#[test]
fn optional_async_keyword_matches_present_and_absent_forms() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(
        &adapter,
        "export {{? keyword:async }} function load(): Promise<void> {}",
        MatchMode::Exact,
    );
    for source in [
        "export async function load(): Promise<void> {}",
        "export function load(): Promise<void> {}",
    ] {
        let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
        let result = match_template("optional-async", &template, &parsed.root, &|value| {
            adapter.validate_identifier(value)
        });
        assert!(result.matches, "{source}: {:?}", result.diagnostics);
    }
}

#[test]
fn optional_async_keyword_rejects_a_different_modifier() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(
        &adapter,
        "export {{? keyword:async }} function load(): void {}",
        MatchMode::Exact,
    );
    let parsed = adapter
        .parse(
            "export declare function load(): void;",
            Path::new("source.ts"),
        )
        .unwrap();
    let result = match_template("optional-async", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(!result.matches);
}

#[test]
fn generic_optional_statement_matches_zero_or_one_node() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(
        &adapter,
        "{{? MaybeStatement }}\nexport const value = 1;",
        MatchMode::Exact,
    );
    for source in [
        "export const value = 1;",
        "debugger;\nexport const value = 1;",
    ] {
        let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
        let result = match_template("optional-statement", &template, &parsed.root, &|value| {
            adapter.validate_identifier(value)
        });
        assert!(result.matches, "{source}: {:?}", result.diagnostics);
    }
}

const MEMBER_ORDER: &str = r#"class {{ Name }} {
  {{* Constructors | field("constructor", "equal", true) }}
  {{* PublicMembers | field("access", "equal", "public") }}
  {{* PrivateMembers | field("access", "equal", "private") }}
}
"#;

#[test]
fn every_mode_checks_member_order_in_each_class() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(&adapter, MEMBER_ORDER, MatchMode::Every);
    let source = r#"
export class Good {
  constructor(private readonly value: number) {}
  @Get()
  run(): void {}
  @Inject() private dep = 1;
  #secret = 2;
}
class Empty {}
class BadPrivate {
  private a = 1;
  run(): void {}
}
class BadConstructor {
  run(): void {}
  constructor() {}
}
"#;
    let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
    let result = match_template_in_scope(
        "member-order",
        &template,
        &parsed.root,
        SearchScope::Descendants,
        &|value| adapter.validate_identifier(value),
    );
    assert!(!result.matches);
    let lines = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.source.as_ref().unwrap().start.line)
        .collect::<Vec<_>>();
    assert_eq!(lines, [12, 16], "{:?}", result.diagnostics);
}

#[test]
fn literal_members_still_match_without_member_fields() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(
        &adapter,
        "class {{ Name }} {\n  constructor() {}\n  {{* Rest }}\n}",
        MatchMode::Exact,
    );
    let parsed = adapter
        .parse(
            "class A {\n  constructor() {}\n  private x = 1;\n}",
            Path::new("source.ts"),
        )
        .unwrap();
    let result = match_template("literal", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

fn compile_error(adapter: &TypeScriptAdapter, source: &str, mode: MatchMode) -> String {
    let path = Path::new("rule.ts.ctmpl");
    let ranges = adapter.scan_placeholders(source, path).unwrap();
    let placeholders = parse_placeholders(source, path, &ranges, adapter).unwrap();
    adapter
        .compile_template(source, path, &placeholders, mode)
        .unwrap_err()[0]
        .code
        .clone()
}

#[test]
fn rejects_unfiltered_sequence_before_another_sequence() {
    let adapter = TypeScriptAdapter::new();
    let code = compile_error(
        &adapter,
        "class {{ Name }} {\n  {{* First }}\n  {{* Second | field(\"static\", \"equal\", true) }}\n}",
        MatchMode::Exact,
    );
    assert_eq!(code, "CTC2007");
}

#[test]
fn every_mode_requires_one_top_level_node() {
    let adapter = TypeScriptAdapter::new();
    assert_eq!(
        compile_error(&adapter, "class A {}\nclass B {}", MatchMode::Every),
        "CTC2011"
    );
    assert_eq!(
        compile_error(&adapter, "{{* Before }}\nclass A {}", MatchMode::Every),
        "CTC2011"
    );
}

#[test]
fn every_mode_kinds_cover_abstract_classes_and_class_expressions() {
    let adapter = TypeScriptAdapter::new();
    let template = compile(
        &adapter,
        &MEMBER_ORDER.replace("{{ Name }}", "{{? Name }}"),
        MatchMode::Every,
    );
    let source = r#"
export abstract class GoodAbstract {
  constructor() {}
  public abstract run(): void;
}
export default abstract class BadAbstract {
  private a = 1;
  run(): void {}
}
@Injectable()
class Generic<T> extends Base<T> implements Runner {
  run(): void {}
  private item?: T;
}
const Anonymous = class {
  private a = 1;
  run(): void {}
};
namespace Outer {
  export class Nested {
private a = 1;
run(): void {}
  }
}
function factory() {
  return class Local {
run(): void {}
  };
}
"#;
    let parsed = adapter.parse(source, Path::new("source.ts")).unwrap();
    let lines = |kinds: &[&str]| {
        let kinds = kinds
            .iter()
            .map(|kind| kind.to_string())
            .collect::<Vec<_>>();
        let result = match_template_in_scope_with_kinds(
            "member-order",
            &template,
            &parsed.root,
            SearchScope::Descendants,
            &kinds,
            &|value| adapter.validate_identifier(value),
        );
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.source.as_ref().unwrap().start.line)
            .collect::<Vec<_>>()
    };
    assert_eq!(lines(&[]), [22]);
    assert_eq!(
        lines(&["ClassDeclaration", "AbstractClassDeclaration", "Class"]),
        [8, 17, 22]
    );
}
