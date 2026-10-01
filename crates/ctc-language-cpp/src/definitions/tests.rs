use std::path::Path;

use ctc_core::canonical::{MemberFunctionFact, MemberFunctionRole};

use crate::canonicalize::parse_source;

fn facts(source: &str) -> Vec<MemberFunctionFact> {
    parse_source(source, Path::new("source.hpp"))
        .unwrap()
        .semantic_facts
        .member_functions
}

fn describe(facts: &[MemberFunctionFact], role: MemberFunctionRole) -> Vec<String> {
    facts
        .iter()
        .filter(|fact| fact.role == role)
        .map(|fact| {
            let mut owner = fact.owner.join("::");
            if !owner.is_empty() {
                owner.push_str("::");
            }
            format!("{owner}{}{}", fact.name, fact.signature)
        })
        .collect()
}

fn declarations(source: &str) -> Vec<String> {
    describe(&facts(source), MemberFunctionRole::Declaration)
}

fn definitions(source: &str) -> Vec<String> {
    describe(&facts(source), MemberFunctionRole::Definition)
}

#[test]
fn finds_declarations_that_need_definitions() {
    let found = declarations(
        r#"
namespace a::b {
class Widget final : public Base {
public:
  Widget() = default;
  Widget(const Widget&) = delete;
  explicit Widget(int value);
  ~Widget();
  virtual void run() = 0;
  int value() const;
  int& ref() &&;
  static Widget make(const std::string& name = "x", int n = 1);
  bool operator==(const Widget& other) const;
  operator bool() const;
  inline int fast();
  constexpr int size() const;
  template <typename T> void visit(T t);
  friend void swap(Widget&, Widget&);
  void f(void);
  void g(const int x, int* const p, const char* text, int (&values)[3], ...);
  int (*callback)(int);
  int inlineValue() const { return 1; }
  virtual int over() const override;
  int* pointer() noexcept;
  int data_;
#ifdef FEATURE
  void feature();
#endif
  class Inner {
    void inner();
  };
};
}
"#,
    );
    assert_eq!(
        found,
        [
            "a::b::Widget::Widget(int)",
            "a::b::Widget::~Widget()",
            "a::b::Widget::value() const",
            "a::b::Widget::ref() &&",
            "a::b::Widget::make(const std::string&, int)",
            "a::b::Widget::operator==(const Widget&) const",
            "a::b::Widget::f()",
            "a::b::Widget::g(int, int*, const char*, int(&)[3], ...)",
            "a::b::Widget::over() const",
            "a::b::Widget::pointer()",
            "a::b::Widget::feature()",
            "a::b::Widget::Inner::inner()",
        ]
    );
}

#[test]
fn skips_class_templates_and_free_functions() {
    let found = declarations(
        r#"
template <typename T>
class Box {
public:
  void put(T value);
};
void free_function();
struct Point {
  void move(int dx, int dy);
};
namespace {
class Hidden {
  void hide();
};
}
extern "C++" {
class Linked {
  void link();
};
}
"#,
    );
    assert_eq!(
        found,
        ["Point::move(int, int)", "Hidden::hide()", "Linked::link()"]
    );
}

#[test]
fn finds_qualified_definitions_outside_classes() {
    let found = definitions(
        r#"
namespace a::b {
Widget::Widget(int value) : value_(value) {}
Widget::~Widget() {}
int Widget::value() const { return 1; }
int& Widget::ref() && { return value_; }
Widget Widget::make(const std::string& label, int count) { return {}; }
bool Widget::operator==(const Widget& other) const { return true; }
Widget::operator bool() const { return true; }
template <typename T> void Widget::visit(T t) {}
void Widget::Inner::inner() {}
auto Widget::f() -> void {}
int* Widget::pointer() noexcept { return nullptr; }
Widget::Widget() = default;
void Widget::g(const int x, int* const p, const char* text, int (&values)[3], ...) {}
}
void free_function() {}
void ::global() {}
void a::Widget::h() {}
class Local {
  void inline_member() {}
};
"#,
    );
    assert_eq!(
        found,
        [
            "a::b::Widget::Widget(int)",
            "a::b::Widget::~Widget()",
            "a::b::Widget::value() const",
            "a::b::Widget::ref() &&",
            "a::b::Widget::make(const std::string&, int)",
            "a::b::Widget::operator==(const Widget&) const",
            "a::b::Widget::Inner::inner()",
            "a::b::Widget::f()",
            "a::b::Widget::pointer()",
            "a::b::Widget::Widget()",
            "a::b::Widget::g(int, int*, const char*, int(&)[3], ...)",
            "a::Widget::h()",
        ]
    );
}

#[test]
fn declaration_ranges_point_at_the_member() {
    let found = facts("class Widget {\npublic:\n  void run();\n};\n");
    assert_eq!(found[0].range.start.line, 3);
    assert_eq!(found[0].range.path, "source.hpp");
}
