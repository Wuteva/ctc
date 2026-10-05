# Rust guide

This page covers the Rust adapter: file suffixes, template names, useful kinds
and fields, known limits, and verified recipes based on the rules this
repository uses.

Related pages:

- [README](../../README.md)
- [Configuration guide](../configuration.md)
- [Template guide](../templates.md)
- [Command guide](../commands.md)
- [TypeScript guide](typescript.md)
- [C++ guide](cpp.md)
- [Lua guide](lua.md)

## File suffixes and template names

| Source files | Template files |
|---|---|
| `.rs` | `.rs.ctmpl` |

Example:

```text
no-unwrap.rs.ctmpl
```

## Canonical kinds worth knowing

The Rust adapter uses a fixed set of shared names and exposes other named kinds
in PascalCase.

| Kind | Use |
|---|---|
| `SourceFile` | Whole-file templates |
| `FunctionDeclaration` | Functions and methods |
| `ArrowFunction` | Closures |
| `StatementBlock` | Function bodies |
| `ClassBody` | Impl bodies, trait bodies, module bodies, struct fields, and enum variants |
| `StructDeclaration` | `struct` |
| `EnumDeclaration` | `enum` |
| `TraitDeclaration` | `trait` |
| `ImplBlock` | `impl` blocks |
| `ModuleDeclaration` | `mod` items |
| `UseDeclaration` | `use` rules |
| `CallExpression` | Call matching with `callee` |
| `MacroInvocation` | Macro rules such as `panic!` |
| `AttributeItem` | Outer attributes such as `#[allow(...)]` |

## Fields

| Field | Where it appears | Meaning |
|---|---|---|
| `callee` | calls and macros | compact callee text, such as `unwrap` or `panic!` |
| `attribute` | `AttributeItem`, `InnerAttributeItem` | attribute path, such as `allow` or `clippy::foo` |
| `public` | items and fields | `true` only for plain `pub` |
| `visibility` | items and fields | full visibility text, such as `pub(crate)` |
| `async` | `FunctionDeclaration` | `true` when `async` is written |
| `unsafe` | `FunctionDeclaration` | `true` when `unsafe` is written |
| `const` | `FunctionDeclaration` | `true` when `const` is written |
| `mutable` | `let` declarations and parameters | `true` when `mut` is written |
| `module` | `UseDeclaration` | The path of the `use`, without white space |

Every node also supports `lineCount`.

## Supported sequence slots

Rust templates support sequence placeholders in:

- source-file items
- block statements
- function parameters
- call arguments
- struct fields
- enum variants
- impl, trait, and module items
- use-list items
- generic parameters and generic arguments
- match arms
- item attributes

Optional keyword placeholders support `async`, `const`, `default`, `extern`,
`mut`, `pub`, and `unsafe`.

## Semantic rules

Rust has no Rust-only semantic rules today. It supports the generic
`companionFile` and `fileLength` rules.

## Known limits

- The adapter does not do type analysis or name resolution.
- It does not expand macros.
- Tokens inside a macro invocation are opaque, so a template cannot inspect
  `println!(...)` arguments.
- Tuple-struct field lists are not normalized to `ClassBody`.

## Rule recipes

Every recipe below was checked against `ctc 0.1.0` in a scratch project.

### Tests live in `tests.rs` files

Goal: ban inline `mod tests` blocks in production files.

Template file:

```rust
mod {{ Name | matches("^tests$") }} {
    {{* Items }}
}
```

`.ctc.json` rule:

```json
{
  "id": "tests-in-own-file",
  "template": ".ctmpl/no-inline-tests.rs.ctmpl",
  "include": ["src/**/*.rs"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```rust
mod tests {
    fn it_works() {}
}
```

Expected diagnostic: `src/lib.rs:1:1 CTC3006`

### No `unwrap()`

Goal: ban `unwrap()` in production code.

Template file:

```rust
{{ Call | kind("CallExpression") | field("callee", "equal", "unwrap") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-unwrap",
  "template": ".ctmpl/no-unwrap.rs.ctmpl",
  "include": ["src/**/*.rs"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```rust
fn run(value: Option<usize>) {
    value.unwrap();
}
```

Expected diagnostic: `src/lib.rs:2:5 CTC3006`

### No panic macros

Goal: ban `panic!`, `todo!`, `unimplemented!`, and `dbg!`.

Template file:

```rust
{{ Call | kind("MacroInvocation") | field("callee", "matches", "^(panic|todo|unimplemented|dbg)!$") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-panic",
  "template": ".ctmpl/no-panic.rs.ctmpl",
  "include": ["src/**/*.rs"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```rust
fn run() {
    panic!("bad");
}
```

Expected diagnostic: `src/lib.rs:2:5 CTC3006`

### No lint-escape attributes

Goal: ban `#[allow(...)]` and `#[expect(...)]`.

Template file:

```rust
{{ Attr | field("attribute", "matches", "^(allow|expect)$") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-lint-escapes",
  "template": ".ctmpl/no-lint.rs.ctmpl",
  "include": ["src/**/*.rs"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```rust
#[allow(dead_code)]
fn run() {}
```

Expected diagnostic: `src/lib.rs:1:1 CTC3006`

### Ban a crate or module path

Goal: block `use` of a module, for example direct `std::fs` in code that must go
through a wrapper. A `use` declaration has a `module` field. It holds the path
text without white space, such as `std::fs` or `std::fs::{self,File}`.

Template file `.ctmpl/no-direct-fs.rs.ctmpl`:

```rust
{{ Use | field("module", "matches", "^(std::fs|tokio::fs)") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-direct-fs",
  "template": ".ctmpl/no-direct-fs.rs.ctmpl",
  "include": ["src/**/*.rs"],
  "exclude": ["src/fs_wrapper.rs"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```rust
use std::fs;
use std::fs::{self, File};
use tokio::fs::read;
use std::collections::HashMap;
fn main() {}
```

Expected diagnostics: `src/a.rs:1:1`, `src/a.rs:2:1`, and `src/a.rs:3:1`, all
`CTC3006`.

`module` only covers `use` declarations. A call written with the full path, such
as `std::fs::read_to_string(p)`, has no `use`. Catch it with `callee`:

```rust
{{ Call | kind("CallExpression") | field("callee", "matches", "^(std|tokio)::fs::") }}
```

### Function length

Goal: fail long functions with `lineCount`.

Template file:

```rust
{{ Long | kind("FunctionDeclaration") | field("lineCount", "greaterThan", 4) }}
```

The limit is 4 lines here only to keep the sample short. Use a real limit, such as 60 or 100.

`.ctc.json` rule:

```json
{
  "id": "short-functions",
  "template": ".ctmpl/long-function.rs.ctmpl",
  "include": ["src/**/*.rs"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```rust
fn run() {
    let a = 1;
    let b = 2;
    let _c = a + b;
}
```

Expected diagnostic: `src/lib.rs:1:1 CTC3006`

### File length

Goal: limit total file size without parsing syntax.

`.ctc.json` rule:

```json
{
  "kind": "fileLength",
  "id": "short-files",
  "include": ["src/**/*.rs"],
  "maxLines": 3
}
```

Violating source:

```rust
const A: usize = 1;
const B: usize = 2;
const C: usize = 3;
const D: usize = 4;
```

Expected diagnostic: `src/lib.rs:4:1 CTC4301`

### Crate roots start with `#![forbid(unsafe_code)]`

Goal: require the crate root attribute that this repository uses.

Template file:

```rust
#![forbid(unsafe_code)]
```

`.ctc.json` rule:

```json
{
  "id": "forbid-unsafe",
  "template": ".ctmpl/forbid-unsafe.rs.ctmpl",
  "include": ["src/lib.rs"],
  "mode": "contains"
}
```

Violating source. The crate root has no such attribute:

```rust
fn run() {}
```

Expected diagnostic: `src/lib.rs:1:1 CTC3002`. The message says the rule expected
an `InnerAttributeItem` and found a `FunctionDeclaration`.

A different attribute at the top also fails. With `#![deny(missing_docs)]` first,
the report points at `deny` (`src/lib.rs:1:4`), because the rule expected `forbid`.

Passing source:

```rust
#![forbid(unsafe_code)]
fn run() {}
```

### `every` on `impl` blocks

Goal: keep public methods before private helper methods inside each `impl`.

Template file:

```rust
impl {{ Name }} {
    {{* PublicMembers | field("public", "equal", true) }}
    {{* PrivateMembers }}
}
```

`.ctc.json` rule:

```json
{
  "id": "impl-order",
  "template": ".ctmpl/impl-order.rs.ctmpl",
  "include": ["src/**/*.rs"],
  "mode": "every",
  "scope": "descendants",
  "kinds": ["ImplBlock"]
}
```

Violating source:

```rust
struct Widget;

impl Widget {
    fn helper(&self) {}

    pub fn run(&self) {}
}
```

Expected diagnostic: `src/lib.rs:6:5 CTC3003`

### No plain `pub` API

Goal: ban plain `pub` where you want an internal crate surface.

Template file:

```rust
{{ Item | field("public", "equal", true) }}
```

`.ctc.json` rule:

```json
{
  "id": "no-pub-api",
  "template": ".ctmpl/no-pub.rs.ctmpl",
  "include": ["src/**/*.rs"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```rust
pub fn leak() {}
```

Expected diagnostic: `src/lib.rs:1:1 CTC3006`
