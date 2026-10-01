use std::path::Path;

use ctc_core::{
    language::LanguageAdapter,
    matcher::{
        MatchMode, SearchScope, match_template, match_template_in_scope,
        match_template_in_scope_with_kinds,
    },
    template::{CompiledTemplate, TemplateNode, parse_placeholders},
};

use crate::CppAdapter;

fn compile(adapter: &CppAdapter, source: &str, mode: MatchMode) -> CompiledTemplate {
    let path = Path::new("rule.cpp.ctmpl");
    let ranges = adapter.scan_placeholders(source, path).unwrap();
    let placeholders = parse_placeholders(source, path, &ranges, adapter).unwrap();
    adapter
        .compile_template(source, path, &placeholders, mode)
        .unwrap()
}

#[test]
fn class_factory_template_matches_cpp_source() {
    let template_source = r#"
{{* Prefix }}
class {{ Name }} {
{{* Members }}
};
std::unique_ptr<{{ Name }}> {{ Name | prefix("create") }}({{* Parameters }}) {
{{* Statements }}
}
"#;
    let source = r#"
#include <memory>
class Widget {
public:
  explicit Widget(int value) : value_(value) {}
  int value() const { return value_; }
private:
  int value_;
};
std::unique_ptr<Widget> createWidget(int value) {
  return std::make_unique<Widget>(value);
}
"#;
    let adapter = CppAdapter::new();
    let template = compile(&adapter, template_source, MatchMode::Exact);
    let parsed = adapter.parse(source, Path::new("source.cpp")).unwrap();
    let result = match_template("class-factory", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn file_name_filter_checks_the_class_name_and_keeps_backreferences() {
    let template_source = r#"
class {{ Name | fileName("PascalCase") }} {
{{* Members }}
};
std::unique_ptr<{{ Name }}> {{ Name | prefix("create") }}();
"#;
    let source = r#"
class UserService {
public:
  void run();
};
std::unique_ptr<UserService> createUserService();
"#;
    let adapter = CppAdapter::new();
    let template = compile(&adapter, template_source, MatchMode::Exact);
    let check = |path: &str| {
        let parsed = adapter.parse(source, Path::new(path)).unwrap();
        match_template("file-name", &template, &parsed.root, &|value| {
            adapter.validate_identifier(value)
        })
    };
    let result = check("include/user_service.hpp");
    assert!(result.matches, "{:?}", result.diagnostics);

    let result = check("include/widget.hpp");
    assert!(!result.matches);
    let diagnostic = &result.diagnostics[0];
    assert_eq!(diagnostic.code, "CTC3007");
    assert_eq!(diagnostic.expected.as_deref(), Some("Widget"));
    assert_eq!(diagnostic.actual.as_deref(), Some("UserService"));
}

#[test]
fn forbid_template_finds_nested_throw_statement() {
    let adapter = CppAdapter::new();
    let template = compile(
        &adapter,
        r#"{{ Forbidden | kind("ThrowStatement") }}"#,
        MatchMode::Forbid,
    );
    let parsed = adapter
        .parse(
            "int parse(bool invalid) { if (invalid) { throw 1; } return 0; }",
            Path::new("source.cpp"),
        )
        .unwrap();
    let result = match_template_in_scope(
        "no-throw",
        &template,
        &parsed.root,
        SearchScope::Descendants,
        &|value| adapter.validate_identifier(value),
    );
    assert!(!result.matches);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code, "CTC3006");
}

#[test]
fn shared_search_scopes_work_for_cpp_functions_and_classes() {
    let adapter = CppAdapter::new();
    let template = compile(
        &adapter,
        r#"{{ Forbidden | kind("ThrowStatement") }}"#,
        MatchMode::Forbid,
    );
    let parsed = adapter
        .parse(
            r#"
void free_function() {
  throw 1;
}

class Example {
public:
  void method() {
throw 2;
  }
};
"#,
            Path::new("source.cpp"),
        )
        .unwrap();

    let match_scope = |scope| {
        match_template_in_scope("no-throw", &template, &parsed.root, scope, &|value| {
            adapter.validate_identifier(value)
        })
    };

    assert_eq!(match_scope(SearchScope::TopLevel).diagnostics.len(), 0);
    assert_eq!(match_scope(SearchScope::Descendants).diagnostics.len(), 2);
    assert_eq!(match_scope(SearchScope::FunctionBody).diagnostics.len(), 2);
    assert_eq!(match_scope(SearchScope::ClassBody).diagnostics.len(), 1);
}

#[test]
fn optional_keyword_matches_present_and_absent_forms() {
    let adapter = CppAdapter::new();
    for (rule, template_source, sources) in [
        (
            "optional-inline",
            "{{? keyword:inline }} int value();",
            ["inline int value();", "int value();"],
        ),
        (
            "optional-noexcept",
            "int value() {{? keyword:noexcept }};",
            ["int value() noexcept;", "int value();"],
        ),
    ] {
        let template = compile(&adapter, template_source, MatchMode::Exact);
        for source in sources {
            let parsed = adapter.parse(source, Path::new("source.hpp")).unwrap();
            let result = match_template(rule, &template, &parsed.root, &|value| {
                adapter.validate_identifier(value)
            });
            assert!(result.matches, "{source}: {:?}", result.diagnostics);
        }
    }
}

#[test]
fn explicit_type_and_expression_placeholders_match() {
    let adapter = CppAdapter::new();
    let template = compile(
        &adapter,
        "{{ type:ReturnType }} convert({{ type:InputType }} value) { return {{ expression:Value }}; }",
        MatchMode::Exact,
    );
    let parsed = adapter
        .parse(
            "Result convert(Input value) { return Result{value}; }",
            Path::new("source.cpp"),
        )
        .unwrap();
    let result = match_template("categories", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn one_standalone_placeholder_keeps_the_source_file_root() {
    let adapter = CppAdapter::new();
    let template = compile(&adapter, "{{ Declaration }}", MatchMode::Exact);
    let parsed = adapter
        .parse("constexpr int value = 1;", Path::new("source.cpp"))
        .unwrap();
    let result = match_template("one-declaration", &template, &parsed.root, &|value| {
        adapter.validate_identifier(value)
    });
    assert!(result.matches, "{:?}", result.diagnostics);
}

#[test]
fn standalone_fallback_does_not_wrap_an_initializer_capture() {
    let adapter = CppAdapter::new();
    let template = compile(
        &adapter,
        r#"
std::array<int, 2> values = {
{{* Values }}
};
{{* Declarations }}
"#,
        MatchMode::Exact,
    );
    let parsed = adapter
        .parse(
            r#"
std::array<int, 2> values = {
  1,
  2
};
int other;
"#,
            Path::new("source.cpp"),
        )
        .unwrap();
    let result = match_template(
        "initializer-and-declarations",
        &template,
        &parsed.root,
        &|value| adapter.validate_identifier(value),
    );
    assert!(result.matches, "{:?}", result.diagnostics);
}

const MEMBER_ORDER: &str = r#"class {{ Name }} {
  {{* Usings | kind("AliasDeclaration") }}
public:
  {{* Constructors | field("constructor", "equal", true) }}
  {{* PublicMembers }}
private:
  {{* PrivateMembers }}
};
"#;

#[test]
fn access_labels_filter_the_placeholders_after_them() {
    let adapter = CppAdapter::new();
    let template = compile(&adapter, MEMBER_ORDER, MatchMode::Every);
    let source = r#"
class Good {
  using Id = int;
public:
  Good();
  explicit Good(int value) {}
  int value() const;
private:
  int value_;
public:
};
class Empty {};
namespace ns {
class BadConstructor {
public:
  int value() const;
  BadConstructor();
};
}
class BadUsing {
public:
  BadUsing();
  using Id = int;
};
class BadPrivate {
  int hidden_;
public:
  int value() const;
};
"#;
    let parsed = adapter.parse(source, Path::new("source.hpp")).unwrap();
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
    assert_eq!(lines, [17, 23, 28], "{:?}", result.diagnostics);
}

#[test]
fn literal_member_after_label_requires_that_access() {
    let adapter = CppAdapter::new();
    let template = compile(
        &adapter,
        "class {{ Name }} {\npublic:\n  int value() const;\n};",
        MatchMode::Exact,
    );
    let check = |source: &str| {
        let parsed = adapter.parse(source, Path::new("source.hpp")).unwrap();
        match_template("literal", &template, &parsed.root, &|value| {
            adapter.validate_identifier(value)
        })
        .matches
    };
    assert!(check("class A {\npublic:\n  int value() const;\n};"));
    assert!(check(
        "class A {\npublic:\npublic:\n  int value() const;\n};"
    ));
    assert!(!check("class A {\n  int value() const;\n};"));
}

#[test]
fn literal_members_do_not_carry_cpp_member_flags() {
    fn check_literals(node: &TemplateNode) {
        if let TemplateNode::Literal {
            fields, children, ..
        } = node
        {
            assert!(
                fields.keys().all(|name| name.as_ref() == "access"),
                "{fields:?}"
            );
            children.iter().for_each(check_literals);
        }
    }
    let adapter = CppAdapter::new();
    let template = compile(
        &adapter,
        r#"class {{ Name }} {
  friend class Builder;
public:
  virtual ~Widget() = default;
  bool operator==(const Widget&) const = delete;
  virtual void draw() const override final = 0;
};"#,
        MatchMode::Exact,
    );
    check_literals(&template.root);
}

#[test]
fn destructor_group_must_follow_constructor_group() {
    let adapter = CppAdapter::new();
    let template = compile(
        &adapter,
        r#"class {{ Name }} {
  {{* Friends | field("friend", "equal", true) }}
public:
  {{* Constructors | field("constructor", "equal", true) }}
  {{* Destructors | field("destructor", "equal", true) }}
  {{* Operators | field("operator", "equal", true) }}
  {{* PublicMembers }}
private:
  {{* PrivateMembers }}
};
"#,
        MatchMode::Every,
    );
    let source = r#"
class Good {
  friend class Builder;
public:
  Good();
  ~Good();
  Good& operator=(const Good&) = default;
  int value() const;
private:
  int value_;
};
class DestructorFirst {
public:
  ~DestructorFirst();
  DestructorFirst();
};
class OperatorLast {
public:
  int value() const;
  bool operator==(const OperatorLast&) const;
};
"#;
    let parsed = adapter.parse(source, Path::new("source.hpp")).unwrap();
    let result = match_template_in_scope(
        "member-order",
        &template,
        &parsed.root,
        SearchScope::Descendants,
        &|value| adapter.validate_identifier(value),
    );
    let lines = result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.source.as_ref().unwrap().start.line)
        .collect::<Vec<_>>();
    assert_eq!(lines, [15, 20], "{:?}", result.diagnostics);
}

#[test]
fn every_mode_kinds_cover_structs_with_their_default_access() {
    let adapter = CppAdapter::new();
    let template = compile(
        &adapter,
        &MEMBER_ORDER.replace("{{ Name }}", "{{? Name }}"),
        MatchMode::Every,
    );
    let source = r#"
class Declared;
struct Forward* pointer;
struct Point {
  Point();
  int x() const;
private:
  int x_;
};
template <typename T>
struct Holder final : Base<T> {
  using Value = T;
  Holder();
  T get() const;
};
struct BadPrivate {
private:
  int hidden_;
public:
  int shown() const;
};
class Outer {
public:
  struct Inner {
int value() const;
Inner();
  };
};
typedef struct {
private:
  int hidden_;
public:
  int shown_;
} Anonymous;
"#;
    let parsed = adapter.parse(source, Path::new("source.hpp")).unwrap();
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
    assert!(lines(&[]).is_empty());
    assert_eq!(
        lines(&["ClassDeclaration", "StructDeclaration"]),
        [20, 26, 33]
    );
}
