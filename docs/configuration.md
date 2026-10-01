# Configuration guide

This page explains how `.ctc.json` selects files and connects them to
templates. It also covers match modes, scopes, ignore comments, rule messages,
and semantic rule setup.

Related pages:

- [README](../README.md)
- [Template guide](templates.md)
- [Command guide](commands.md)
- [TypeScript guide](languages/typescript.md)
- [C++ guide](languages/cpp.md)
- [Rust guide](languages/rust.md)

## Project layout

`ctc` looks for a root `.ctc.json` file. Keep reusable templates in `.ctmpl\`.

```text
project\
  .ctc.json
  .ctmpl\
    no-value-imports.ts.ctmpl
    member-order.hpp.ctmpl
    no-unwrap.rs.ctmpl
  src\
  include\
  crates\
```

This split lets `.ctmpl\` be shared, even as a Git submodule, while each
project keeps its own file selection in `.ctc.json`.

Template filenames must include a supported source suffix before `.ctmpl`, such
as `rule.ts.ctmpl`, `rule.hpp.ctmpl`, or `rule.rs.ctmpl`.

## `.ctc.json` shape

The root object always contains `schemaVersion` and at least one rule in
`rules` or `semanticRules`.

```json
{
  "schemaVersion": 1,
  "rules": [
    {
      "id": "no-value-imports",
      "template": ".ctmpl/no-value-imports.ts.ctmpl",
      "include": ["src/**/*.ts"],
      "exclude": ["src/index.ts"],
      "mode": "forbid",
      "scope": "topLevel",
      "message": "Use import type outside the composition root."
    }
  ],
  "semanticRules": [
    {
      "kind": "fileLength",
      "id": "short-files",
      "include": ["src/**/*.ts"],
      "maxLines": 800
    }
  ]
}
```

## Template rule fields

| Field | Required | Meaning |
|---|---|---|
| `id` | Yes | Rule name. Use lowercase letters, digits, and `-`. |
| `template` | Yes | One template path or an array of paths. |
| `include` | Yes | One or more glob patterns. |
| `exclude` | No | Glob patterns to skip after `include` matches. |
| `mode` | No | `exact`, `contains`, `forbid`, or `every`. Default is `exact`. |
| `scope` | No | Search region. Default is `topLevel`. |
| `kinds` | No | Extra candidate kinds for `every` mode. |
| `count` | No | Match count limits for `contains` mode. |
| `message` | No | One-line custom rule message. |
| `allowIgnore` | No | Lets source comments skip this rule. Default is `false`. |

## Selecting files

`include` and `exclude` are relative to the scan root. Always use `/`
separators, even on Windows.

Common patterns:

- `src/**/*.ts`
- `src/**/*.test.ts`
- `include/**/*.hpp`
- `crates/*/src/**/*.rs`
- `src/service.ts`

Globs support `*`, `?`, `**`, character classes, and brace alternatives.

A file is selected when it matches at least one `include` pattern and no
`exclude` pattern.

## Match modes

`mode` tells `ctc` how to search.

| Mode | What it checks |
|---|---|
| `exact` | The whole source list must match the template. |
| `contains` | The template must appear at least once. |
| `forbid` | The template must not appear at all. |
| `every` | Every candidate node of the chosen kind must match the template. |

`exact` is strict and works only with the `topLevel` scope.

### Several templates per rule

Use several templates when one file is allowed to take more than one shape.

```json
{
  "id": "jquery-imports",
  "template": [
    ".ctmpl/jquery-default.ts.ctmpl",
    ".ctmpl/jquery-side-effect.ts.ctmpl",
    ".ctmpl/jquery-equals.ts.ctmpl"
  ],
  "include": ["src/**/*.ts"],
  "mode": "forbid"
}
```

Rules with several templates must stay in one language. `count` is not allowed
with several templates. An `every` rule with several templates also needs
`kinds`.

### Count matches

`contains` mode passes when the template appears at least once. Add `count` to
set the allowed range.

```json
{
  "id": "one-class-per-file",
  "template": ".ctmpl/class.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "contains",
  "count": { "min": 1, "max": 1 }
}
```

`min` defaults to `1`. `max` is optional.

### `every` mode and `kinds`

`every` checks each candidate node against one template node. The template must
have exactly one top-level node.

Without `kinds`, `every` checks only nodes of the template's own kind. Add
`kinds` when the same template should also check related kinds.

```json
{
  "id": "member-order",
  "template": ".ctmpl/member-order.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "every",
  "scope": "descendants",
  "kinds": ["ClassDeclaration", "AbstractClassDeclaration"]
}
```

Useful class-like kinds:

| Language | Kind |
|---|---|
| TypeScript | `ClassDeclaration` |
| TypeScript | `AbstractClassDeclaration` |
| TypeScript | `Class` |
| C++ | `ClassDeclaration` |
| C++ | `StructDeclaration` |
| C++ | `UnionDeclaration` |
| Rust | `ImplBlock` |

When the template ends with a class-like body, `ctc` treats it as a class
template. The body must match in full. The header is looser:

- keywords such as `class`, `struct`, and `abstract` do not have to match
  exactly
- the source may have extra header parts, such as decorators, base classes, or
  implemented interfaces
- each header part that the template does have must still appear in order

For member-order rules, put adjacent sequence placeholders in the order you
want. Each member goes into the first matching group. See the verified recipes
in the [TypeScript guide](languages/typescript.md#class-member-order), the
[C++ guide](languages/cpp.md#member-order-in-classes-and-structs), and the
[Rust guide](languages/rust.md#every-on-impl-blocks).

## Search scopes

In `.ctc.json`, the scope names are camelCase.

| Scope | Search region |
|---|---|
| `topLevel` | Direct source-file statements |
| `descendants` | The full syntax tree |
| `functionBody` | Inside function, method, and arrow-function bodies |
| `classBody` | Inside class-like bodies, including nested method bodies |

### Search inside chosen nodes

Use the object form when you want to search only inside chosen node kinds.

```json
{
  "id": "no-await-in-loop",
  "template": ".ctmpl/no-await.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "forbid",
  "scope": {
    "inside": ["ForStatement", "ForInStatement", "WhileStatement", "DoStatement"]
  }
}
```

`inside` uses canonical kind names. `stopAtFunctions` is optional and defaults
to `true`.

```json
{
  "inside": ["ForStatement"],
  "stopAtFunctions": false
}
```

That setting also searches inside callbacks nested inside the chosen node.

## Custom rule messages

Add `message` when you want the report to say what the rule wants, not only
what matched.

```json
{
  "id": "no-try",
  "template": ".ctmpl/no-try.ts.ctmpl",
  "include": ["src/domain/**/*.ts"],
  "mode": "forbid",
  "scope": "descendants",
  "message": "Return a Result value instead of catching exceptions."
}
```

The message must be one line of text.

## Ignore comments and `allowIgnore`

Rules refuse ignore comments unless they set `allowIgnore` to `true`.

```json
{
  "id": "generated-exception",
  "template": ".ctmpl/no-throw.cpp.ctmpl",
  "include": ["src/generated/**/*.cpp"],
  "mode": "forbid",
  "scope": "descendants",
  "allowIgnore": true
}
```

Use these comments in source files:

```ts
// ctc-ignore-next-line generated-exception
throw new Error("legacy");
```

```cpp
// ctc-ignore-file generated-exception -- generated code
```

If a comment names a rule that does not allow ignores, `ctc` reports
`CTC5002`. If the comment text is malformed, `ctc` reports `CTC5001`.

## Semantic rules overview

Semantic rules live in `semanticRules`. They do not use template files.

| Kind | Language | Use it for | Details |
|---|---|---|---|
| `returnPaths` | TypeScript | Functions with a given return type must return a value on every path | [TypeScript semantic rules](languages/typescript.md#semantic-rules) |
| `exceptionPolicy` | TypeScript | `try`, `throw`, `Promise.reject`, and configured exception sources | [TypeScript semantic rules](languages/typescript.md#semantic-rules) |
| `companionFile` | Any supported language | Require a related file, such as a test | [C++ guide](languages/cpp.md#companion-files) |
| `headerSourcePairing` | C++ | Check header declarations against source definitions | [C++ guide](languages/cpp.md#header-and-source-pairing) |
| `fileLength` | Any supported language | Limit file size without parsing | [TypeScript](languages/typescript.md#file-length), [C++](languages/cpp.md#function-length), [Rust](languages/rust.md#file-length) |

### `companionFile`

```json
{
  "kind": "companionFile",
  "id": "unit-test-exists",
  "include": ["src/**/*.ts"],
  "exclude": ["src/**/*.test.ts"],
  "companions": ["{dir}/{stem}.test.ts", "test/{subdir}/{stem}.test.ts"]
}
```

### `fileLength`

```json
{
  "kind": "fileLength",
  "id": "short-files",
  "include": ["src/**/*.ts"],
  "maxLines": 800
}
```

### `headerSourcePairing`

```json
{
  "kind": "headerSourcePairing",
  "id": "header-source-pairing",
  "include": ["include/**/*.hpp"],
  "sources": ["{dir}/{stem}.cpp", "src/{subdir}/{stem}.cpp"],
  "missingSource": "report",
  "checkOrder": true
}
```

The path tokens are:

| Token | Meaning |
|---|---|
| `{dir}` | The selected file's directory |
| `{subdir}` | The directory without its first segment |
| `{stem}` | The file name without its last extension |
| `{ext}` | The last extension |
