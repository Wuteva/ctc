use std::{collections::BTreeMap, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::diagnostic::TextRange;

/// A field that every node has without an adapter storing it: the number of
/// lines that the node spans in the source.
pub const LINE_COUNT_FIELD: &str = "lineCount";

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CanonicalScalar {
    Bool(bool),
    Number(serde_json::Number),
    String(Arc<str>),
    Null,
}

impl CanonicalScalar {
    pub fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::Null => Some(Self::Null),
            serde_json::Value::Bool(value) => Some(Self::Bool(value)),
            serde_json::Value::Number(value) => Some(Self::Number(value)),
            serde_json::Value::String(value) => Some(Self::String(Arc::from(value))),
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => value.to_string(),
            Self::String(value) => value.to_string(),
            Self::Null => "null".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalNode {
    pub kind: Arc<str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<CanonicalScalar>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<Arc<str>, CanonicalScalar>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<CanonicalNode>,
    pub range: TextRange,
}

#[derive(Clone, Debug, Default)]
pub struct SemanticFacts {
    pub functions: Vec<FunctionFact>,
    pub exceptions: Vec<ExceptionFact>,
    pub calls: Vec<CallFact>,
    pub member_functions: Vec<MemberFunctionFact>,
    pub globals: Vec<GlobalFact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalAccessKind {
    /// The global is used: its value is read, or code reaches a field below it.
    Read,
    /// The global itself is assigned: `count = 1` or `function update() end`.
    Write,
    /// `_G[key]` or `_ENV[key]` with a key that is not a string literal, used
    /// as a value.
    DynamicRead,
    /// `_G[key] = value` or `_ENV[key] = value` with a key that is not a string
    /// literal.
    DynamicWrite,
}

/// How a global is called, when the access is the callee of a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalCall {
    NotCalled,
    /// Called with one string literal argument, such as `require("app.parts")`.
    StringLiteral,
    /// Called with other arguments.
    Other,
}

/// One use of a global variable in a Lua source file. A name that a `local` or
/// `global` declaration, a parameter, or a loop variable binds is not a global.
#[derive(Clone, Debug)]
pub struct GlobalFact {
    pub kind: GlobalAccessKind,
    /// The global name, such as `os`. Empty for a dynamic access.
    pub name: String,
    /// The name and the string keys that follow it, such as `os.time`.
    pub path: String,
    /// True when the path stops at a key that is not a string literal.
    pub dynamic_key: bool,
    /// True when the code reached the name with `_G` or `_ENV`.
    pub via_table: bool,
    pub call: GlobalCall,
    pub range: TextRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberFunctionRole {
    /// A declaration inside a class that needs a definition outside the class.
    Declaration,
    /// A definition outside a class, such as `int Widget::value() const {}`.
    Definition,
}

#[derive(Clone, Debug)]
pub struct MemberFunctionFact {
    pub role: MemberFunctionRole,
    /// Namespaces and classes that own the function, outermost first.
    pub owner: Vec<String>,
    pub name: String,
    /// Normalized parameter types and trailing qualifiers, such as `(int) const`.
    pub signature: String,
    pub range: TextRange,
}

#[derive(Clone, Debug)]
pub struct FunctionFact {
    pub name: String,
    pub range: TextRange,
    pub return_type_names: Vec<String>,
    pub all_paths_return_value: bool,
    pub bare_returns: Vec<TextRange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionKind {
    Try,
    Throw,
    PromiseReject,
}

#[derive(Clone, Debug)]
pub struct ExceptionFact {
    pub kind: ExceptionKind,
    pub range: TextRange,
}

#[derive(Clone, Debug)]
pub struct CallFact {
    pub callee: String,
    pub range: TextRange,
}

impl CanonicalNode {
    /// The value of a field. `lineCount` is computed from the node range.
    pub fn field_value(&self, name: &str) -> Option<CanonicalScalar> {
        if name == LINE_COUNT_FIELD {
            let lines = self.range.end.line.saturating_sub(self.range.start.line) + 1;
            return Some(CanonicalScalar::Number(u64::from(lines).into()));
        }
        self.fields.get(name).cloned()
    }

    pub fn structurally_eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.value == other.value
            && self.fields == other.fields
            && self.children.len() == other.children.len()
            && self
                .children
                .iter()
                .zip(&other.children)
                .all(|(left, right)| left.structurally_eq(right))
    }
}
