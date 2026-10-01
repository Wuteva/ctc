use std::path::Path;

use ctc_core::canonical::{CanonicalNode, CanonicalScalar};

use super::EXTRA_MEMBER_FIELDS;
use crate::canonicalize::parse_source;

fn members(source: &str) -> Vec<CanonicalNode> {
    fn collect(node: &CanonicalNode, members: &mut Vec<CanonicalNode>) {
        for child in &node.children {
            if child.fields.contains_key("access") {
                members.push(child.clone());
            }
            collect(child, members);
        }
    }
    let parsed = parse_source(source, Path::new("source.hpp")).unwrap();
    let mut members = Vec::new();
    collect(&parsed.root, &mut members);
    members
}

/// Returns the names of the fields that are `true`, in field order.
fn flags(node: &CanonicalNode) -> Vec<&str> {
    node.fields
        .iter()
        .filter(|(_, value)| **value == CanonicalScalar::Bool(true))
        .map(|(name, _)| name.as_ref())
        .collect()
}

#[test]
fn exposes_cpp_member_flags() {
    let found = members(
        r#"
class Widget : public Base {
  friend class Builder;
  template <typename T> friend class Holder;
  friend bool operator==(const Widget&, const Widget&);
public:
  Widget();
  Widget(const Widget&) = default;
  Widget(Widget&&) = delete;
  ~Widget();
  Widget& operator=(const Widget&) = delete;
  bool operator<(const Widget& other) const;
  explicit operator bool() const;
  virtual void draw() = 0;
  virtual int* find() = 0;
  virtual int size() const override;
  void close() final {}
  const int value();
  template <typename T> T get() const;
  static Widget make();
#ifdef DEBUG
  virtual void dump() const override final;
#endif
private:
  void (*callback_)() = nullptr;
  int count_ = 0;
};
class Base {
public:
  virtual ~Base() = default;
};
class Abstract {
public:
  virtual ~Abstract() = 0;
  virtual operator int() const = 0;
};
"#,
    );
    let expected: &[&[&str]] = &[
        &["friend"],
        &["friend"],
        &["friend"],
        &["constructor"],
        &["constructor", "defaulted"],
        &["constructor", "deleted"],
        &["destructor"],
        &["deleted", "operator"],
        &["const", "operator"],
        &["const", "operator"],
        &["pure", "virtual"],
        &["pure", "virtual"],
        &["const", "override", "virtual"],
        &["final"],
        &[],
        &["const"],
        &["static"],
        &["const", "final", "override", "virtual"],
        &[],
        &[],
        &["defaulted", "destructor", "virtual"],
        &["destructor", "pure", "virtual"],
        &["const", "operator", "pure", "virtual"],
    ];
    let found_flags = found.iter().map(flags).collect::<Vec<_>>();
    assert_eq!(found_flags, expected, "{found:#?}");
}

#[test]
fn stores_false_for_every_member_flag_that_does_not_apply() {
    let found = members("class Widget { int value_; };");
    assert_eq!(found.len(), 1);
    for name in ["constructor", "static"]
        .into_iter()
        .chain(EXTRA_MEMBER_FIELDS)
    {
        assert_eq!(
            found[0].fields.get(name),
            Some(&CanonicalScalar::Bool(false)),
            "{name}"
        );
    }
}

#[test]
fn out_of_class_destructor_definition_has_no_member_flags() {
    let parsed = parse_source("Widget::~Widget() {}", Path::new("source.cpp")).unwrap();
    assert!(
        parsed
            .root
            .children
            .iter()
            .all(|child| child.fields.is_empty())
    );
}
