# C++ guide

This page covers the C++ adapter: file suffixes, template names, useful kinds
and fields, cross-file rules, limits, and verified rule recipes.

Related pages:

- [README](../../README.md)
- [Configuration guide](../configuration.md)
- [Template guide](../templates.md)
- [Command guide](../commands.md)
- [TypeScript guide](typescript.md)
- [Rust guide](rust.md)
- [Lua guide](lua.md)

## File suffixes and template names

| Source files | Template files |
|---|---|
| `.cpp`, `.cc`, `.cxx` | matching source suffix before `.ctmpl` |
| `.h`, `.hh`, `.hpp`, `.hxx` | matching header suffix before `.ctmpl` |

Examples:

```text
class-factory.cpp.ctmpl
header-layout.hpp.ctmpl
```

## Canonical kinds worth knowing

The C++ adapter keeps stable core names and exposes many raw grammar kinds in
PascalCase. These are the most useful ones.

| Kind | Use |
|---|---|
| `SourceFile` | Whole-file templates |
| `FunctionDeclaration` | Functions and out-of-class method definitions |
| `ArrowFunction` | Lambdas |
| `ClassDeclaration` | `class` |
| `StructDeclaration` | `struct` |
| `UnionDeclaration` | `union` |
| `EnumDeclaration` | `enum` |
| `NamespaceDeclaration` | `namespace` blocks |
| `ClassBody` | Member grouping in `every` mode |
| `StatementBlock` | Function bodies |
| `IncludeDirective` | `#include` rules |
| `ThrowStatement` | `throw` checks |
| `CallExpression` | `callee` checks such as `malloc` and `free` |
| `PointerDeclarator` | Raw pointer declarations |
| `NewExpression` | `new` expressions |

## Fields

Member fields apply to each direct child of a class body.

| Field | Meaning |
|---|---|
| `access` | `public`, `protected`, or `private` |
| `constructor` | `true` on constructors |
| `static` | `true` on `static` members |
| `destructor` | `true` on destructors |
| `virtual` | `true` only when `virtual` is written |
| `pure` | `true` on `= 0` member functions |
| `override` | `true` when `override` is written |
| `final` | `true` when `final` is written |
| `operator` | `true` on operator functions |
| `friend` | `true` on `friend` declarations |
| `deleted` | `true` on `= delete` |
| `defaulted` | `true` on `= default` |
| `const` | `true` on trailing `const` member functions |
| `callee` | Compact callee text on `CallExpression` |
| `module` | The included file on `IncludeDirective`, without quotes or angle brackets |

Every node also supports `lineCount`.

In a template, an access label such as `public:` filters the literal members
and placeholders that follow it until the next access label.

## Supported sequence slots

C++ templates support sequence placeholders in:

- translation-unit declarations
- function statements
- function parameters
- call arguments
- class and structure members
- initializer lists
- template parameters and template arguments

## Semantic rules

C++ supports two cross-file semantic rules:

- `companionFile`
- `headerSourcePairing`

It also supports the generic `fileLength` rule.

## Known limits

- The adapter is syntax-only. It does not use Clang.
- It does not read `compile_commands.json`.
- It does not resolve includes or expand macros.
- `headerSourcePairing` compares owners, names, and normalized parameter text.
- For some patterns, a literal template is simpler than a kind-based rule.

## Rule recipes

Every recipe below was checked against `ctc 0.1.0` in a scratch project.

### Companion files

Goal: require one related test file for each source file.

`.ctc.json` rule:

```json
{
  "kind": "companionFile",
  "id": "has-test",
  "include": ["src/**/*.cpp"],
  "companions": ["test/{stem}.test.cpp"]
}
```

Violating source:

```cpp
int run() { return 1; }
```

Expected diagnostic: `src/widget.cpp:1:1 CTC4201`

### Header and source pairing

Goal: require a matching source definition for each declared member function.

`.ctc.json` rule:

```json
{
  "kind": "headerSourcePairing",
  "id": "header-source",
  "include": ["include/**/*.hpp"],
  "sources": ["src/{stem}.cpp"],
  "missingSource": "report"
}
```

Violating source:

```cpp
class Widget {
public:
  void run();
};
```

```cpp
void Widget::stop() {}
```

Expected diagnostic: `include/widget.hpp:3:3 CTC4202`

### Member order in classes and structs

Goal: keep public members before private members with one `every` rule.

Template file:

```cpp
class {{ Name }} {
public:
  {{* Constructors | field("constructor", "equal", true) }}
  {{* PublicMembers }}
private:
  {{* PrivateMembers }}
};
```

`.ctc.json` rule:

```json
{
  "id": "member-order",
  "template": ".ctmpl/member-order.hpp.ctmpl",
  "include": ["src/**/*.hpp"],
  "mode": "every",
  "scope": "descendants",
  "kinds": ["ClassDeclaration", "StructDeclaration"]
}
```

Violating source:

```cpp
class Widget {
private:
  int hidden_;

public:
  int shown() const;
};
```

Expected diagnostic: `src/widget.hpp:6:3 CTC3003`

### No `throw`

Goal: ban `throw` anywhere in a function body.

Template file:

```cpp
{{ Forbidden | kind("ThrowStatement") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-throw",
  "template": ".ctmpl/no-throw.cpp.ctmpl",
  "include": ["src/**/*.cpp"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```cpp
int run() {
  throw 1;
}
```

Expected diagnostic: `src/widget.cpp:2:3 CTC3006`

### Ban raw pointers

Goal: ban raw pointer declarations in class bodies.

Template file:

```cpp
{{ Ptr | kind("PointerDeclarator") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-raw-pointers",
  "template": ".ctmpl/no-pointer.hpp.ctmpl",
  "include": ["include/**/*.hpp"],
  "mode": "forbid",
  "scope": "classBody"
}
```

Violating source:

```cpp
class Widget {
  int* value_;
};
```

Expected diagnostic: `include/widget.hpp:2:6 CTC3006`

The same template also worked with `scope: "functionBody"` on
`int* value = nullptr;`, which reported `src/widget.cpp:2:6 CTC3006`.

### No `new` or `delete`

Goal: ban manual heap management with two simple templates.

Template files:

```cpp
{{ Expr | kind("NewExpression") }}
```

```cpp
delete {{ Value }};
```

`.ctc.json` rule:

```json
{
  "id": "no-manual-heap",
  "template": [".ctmpl/no-new.cpp.ctmpl", ".ctmpl/no-delete.cpp.ctmpl"],
  "include": ["src/**/*.cpp"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```cpp
int* make() {
  return new int(1);
}
```

Expected diagnostic: `src/widget.cpp:2:10 CTC3006`

The `delete` form was also checked and reported `src/widget.cpp:2:3 CTC3006`.

### Ban C-style casts

Goal: ban one simple cast form with a literal template.

Template file:

```cpp
return ({{ Type }}){{ Value }};
```

`.ctc.json` rule:

```json
{
  "id": "no-c-cast",
  "template": ".ctmpl/no-c-cast.cpp.ctmpl",
  "include": ["src/**/*.cpp"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```cpp
int run(double value) {
  return (int)value;
}
```

Expected diagnostic: `src/widget.cpp:2:3 CTC3006`

### Ban `malloc` and `free`

Goal: match C allocation calls by `callee`.

Template file:

```cpp
{{ Call | kind("CallExpression") | field("callee", "matches", "^(malloc|free)$") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-malloc",
  "template": ".ctmpl/no-malloc.cpp.ctmpl",
  "include": ["src/**/*.cpp"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```cpp
void* alloc() {
  return malloc(4);
}
```

Expected diagnostic: `src/widget.cpp:2:10 CTC3006`

### No `using namespace` in headers

Goal: keep headers free of `using namespace`.

Template file:

```cpp
using namespace {{ Ns }};
```

`.ctc.json` rule:

```json
{
  "id": "no-using-namespace",
  "template": ".ctmpl/no-using.hpp.ctmpl",
  "include": ["include/**/*.hpp"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```cpp
using namespace std;
class Widget {};
```

Expected diagnostic: `include/widget.hpp:1:1 CTC3006`

### Ban a header

Goal: block `#include` of headers that the project does not allow, such as
`<iostream>` in a library, or an old internal folder. An include has a `module`
field. It holds the included file without the angle brackets or quotes.

Template file `.ctmpl/no-legacy-includes.cpp.ctmpl`:

```cpp
{{ Include | field("module", "matches", "^(iostream|cstdio|legacy/.*)$") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-legacy-includes",
  "template": ".ctmpl/no-legacy-includes.cpp.ctmpl",
  "include": ["src/**/*.cpp", "src/**/*.hpp"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```cpp
#include <iostream>
#include <vector>
#include "legacy/util.h"
#include "own.h"
int main() { return 0; }
```

Expected diagnostics: `src/a.cpp:1:1 CTC3006` and `src/a.cpp:3:1 CTC3006`.

Angle brackets and quotes are not part of `module`, so `<cstdio>` and `"cstdio"`
are the same to this rule.

### Function length

Goal: fail long functions with `lineCount`.

Template file:

```cpp
{{ Long | kind("FunctionDeclaration") | field("lineCount", "greaterThan", 4) }}
```

The limit is 4 lines here only to keep the sample short. Use a real limit, such as 60 or 100.

`.ctc.json` rule:

```json
{
  "id": "short-functions",
  "template": ".ctmpl/long-function.cpp.ctmpl",
  "include": ["src/**/*.cpp"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```cpp
int run() {
  int a = 1;
  int b = 2;
  return a + b;
}
```

Expected diagnostic: `src/widget.cpp:1:1 CTC3006`
