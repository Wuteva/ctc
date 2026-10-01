# Template guide

This page explains the template language. It covers placeholders, sequence and
optional captures, name filters, node constraints, and the source-only fields
that you can test with `field(...)`.

Related pages:

- [README](../README.md)
- [Configuration guide](configuration.md)
- [Command guide](commands.md)
- [TypeScript guide](languages/typescript.md)
- [C++ guide](languages/cpp.md)
- [Rust guide](languages/rust.md)

## Placeholder basics

These are the main forms:

```text
{{ Name }}
{{ identifier:Name }}
{{ expression:Value }}
{{ type:Result }}
{{* Members }}
{{? MaybeMember }}
{{? keyword:async }}
```

- `{{ Name }}` captures one syntax node.
- A later `{{ Name }}` must match the same captured node.
- `{{* Members }}` captures zero or more items in one syntax list.
- `{{? MaybeMember }}` captures zero or one item in one syntax list.
- `{{? keyword:async }}` matches the literal keyword `async` or nothing.

Sequence placeholders must fill a whole list item. Two adjacent sequence
placeholders are allowed, but every one except the last needs a `kind(...)` or
`field(...)` filter so `ctc` knows where each group starts.

## Categories

Use a category when you want to say what kind of thing a placeholder captures.

| Category | Use |
|---|---|
| `identifier` | Names such as `Widget` or `run` |
| `expression` | Expressions such as `value + 1` |
| `type` | Type syntax such as `Result<string>` |
| `keyword` | Optional keywords only, with `{{? ... }}` |

If you omit the category, `ctc` infers it from the template position.

## Name filters

Name filters work on identifier captures. They cannot be mixed with node
constraints like `kind(...)` or `field(...)`.

| Filter | Meaning |
|---|---|
| `prefix("create")` | Add text at the front |
| `suffix("Impl")` | Add text at the end |
| `removePrefix("I")` | Remove exact text at the front |
| `removeSuffix("Service")` | Remove exact text at the end |
| `fileName("PascalCase")` | Require the captured name to match the file name |

Examples:

```text
{{ PublicName | suffix("Options") }}
{{ PublicName | suffix("Implementation") }}
{{ InterfaceName | removePrefix("I") | prefix("create") }}
{{ Name | fileName("PascalCase") }}
```

The plain capture can appear before or after the derived use.

`fileName(...)` must be the last filter. The supported case names are
`PascalCase`, `camelCase`, `snake_case`, and `asIs`.

### Pattern filters on names

Use `matches(...)` and `notMatches(...)` to test an identifier with a regular
expression.

```text
{{ Name | matches("^I[A-Z]") }}
{{ Name | notMatches("Impl$") }}
{{ Name | removePrefix("I") | matches("^[A-Z]") }}
```

The expression is not anchored by default. Add `^` and `$` when you need a full
match.

## Node constraints

Use node constraints when you want to match by syntax kind or by canonical
field value.

### `kind(...)`

```text
{{ Import | kind("ImportDeclaration", "ImportEqualsDeclaration") }}
{{ Long | kind("FunctionDeclaration") }}
```

`kind(...)` takes one or more canonical kind names from the selected language.

### `field(...)`

```text
field("name", "equal", value)
field("name", "notEqual", value)
field("name", "exists")
field("name", "notExists")
field("name", "lessThan", number)
field("name", "lessThanOrEqual", number)
field("name", "greaterThan", number)
field("name", "greaterThanOrEqual", number)
field("name", "matches", "regex")
field("name", "notMatches", "regex")
```

Supported operators:

| Operator | Meaning |
|---|---|
| `equal`, `notEqual` | Compare with a scalar value |
| `exists`, `notExists` | Check whether the field exists |
| `lessThan`, `lessThanOrEqual` | Numeric comparison |
| `greaterThan`, `greaterThanOrEqual` | Numeric comparison |
| `matches`, `notMatches` | Regular-expression test on the field text |

## `lineCount`, `callee`, and `module`

Every node has `lineCount`. It is the number of source lines that the node
spans.

```text
{{ Long | kind("FunctionDeclaration") | field("lineCount", "greaterThan", 60) }}
```

Call nodes can expose `callee`. Use it on a placeholder, not on literal call
syntax.

```text
{{ Call | kind("CallExpression") | field("callee", "equal", "eval") }}
{{ Call | kind("CallExpression") | field("callee", "matches", "^(malloc|free)$") }}
```

The expression is not anchored by default here either.

Imports and includes have `module`. It holds the module name or the included
file, without quotes or angle brackets. In TypeScript it is also set on
`require(...)` and `import(...)` calls with a plain string argument. In Rust it is
the path of a `use` declaration, such as `std::fs`. See the language pages for
recipes.

```text
{{ Import | field("module", "matches", "^(node:)?fs(/promises)?$") }}
```n
## String literals in templates

Text that looks like a placeholder inside a string literal is treated as plain
literal text, not as a placeholder. Match the literal text exactly instead.

TypeScript:

```ts
import {{ Bound }} from "jquery";
import "jquery";
```

C++:

```cpp
#include "widget.hpp"
```

Rust:

```rust
#![forbid(unsafe_code)]
```

When several literal forms are allowed, use several template files in one rule.

## Tiny examples by language

### TypeScript

Only `import type` is allowed:

```ts
{{ Import | kind("ImportDeclaration", "ImportEqualsDeclaration") | field("typeOnly", "notEqual", true) }}
```

### C++

Ban raw pointers in one scope:

```cpp
{{ Ptr | kind("PointerDeclarator") }}
```

### Rust

Ban `unwrap()` calls:

```rust
{{ Call | kind("CallExpression") | field("callee", "equal", "unwrap") }}
```
