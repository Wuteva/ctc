# Lua guide

This page covers the Lua adapter: file suffixes, template names, Lua 5.5
syntax support, useful kinds and fields, known limits, and verified recipes.

Related pages:

- [README](../../README.md)
- [Configuration guide](../configuration.md)
- [Template guide](../templates.md)
- [Command guide](../commands.md)
- [TypeScript guide](typescript.md)
- [C++ guide](cpp.md)
- [Rust guide](rust.md)

## File suffixes and template names

| Source files | Template files |
|---|---|
| `.lua` | `.lua.ctmpl` |

Example:

```text
no-load.lua.ctmpl
```

## Lua version

The adapter reads Lua 5.5 syntax. It uses the `tree-sitter-lua` grammar,
version 0.5.0. This grammar has the Lua 5.5 statements.

The table shows the Lua 5.5 and Lua 5.4 forms that we checked. `luac -p` from
Lua 5.5.1 accepts each sample. `ctc` parses each sample without a diagnostic.

| Form | Sample | Version |
|---|---|---|
| Global declaration | `global x` | 5.5 |
| Global declaration with values | `global x, y = 1, 2` | 5.5 |
| Global function | `global function f() end` | 5.5 |
| Collective global declaration | `global *`, `global<const> *` | 5.5 |
| Attribute before the name list | `local<const> a, b = 1, 2`, `global<const> x, y` | 5.5 |
| Attribute after a name | `local x <const> = 1`, `global x <const>, _G` | 5.4 |
| To-be-closed variable | `local f <close> = value` | 5.4 |
| Named vararg table | `function f(...t) return t.n end` | 5.5 |
| Integer `for` loop | `for i = 1, 10, 2 do end` | all |
| `goto` and labels | `goto continue` and `::continue::` | 5.2 |
| Integer division and bit operators | `7 // 2`, `a & b \| c ~ d << 1 >> 2`, `~a` | 5.3 |
| Escapes | `"\u{1F600}"`, `"\z"`, `"\x41"`, `"\65"` | 5.3 |
| Long brackets | `[==[ ... ]==]`, `--[==[ ... ]==]` | all |
| Hexadecimal floats | `0x1p4`, `0xA.8p0` | 5.2 |
| `global` as a name | `local global = 1` | 5.5 with `LUA_COMPAT_GLOBAL` |
| First line that starts with `#` | `#!/usr/bin/env lua` | all |

Lua 5.5 builds with `LUA_COMPAT_GLOBAL` by default. Thus `global` is a
reserved word only in a declaration. The adapter reads it the same way.

We also checked 764 Lua files from other projects that `luac -p` accepts.
`ctc` parsed all of them without a syntax error.

### Syntax errors

A file with a syntax error gets `CTC3001`. The diagnostic points at the
error. The adapter does not run rules on that file.

The grammar accepts some text that Lua rejects. The adapter finds these
forms and reports `CTC3001` with a specific message:

| Form | Example | Message |
|---|---|---|
| A name with a character that is not an ASCII letter, digit, or `_` | `a$b`, `café` | `The source contains an invalid Lua name.` |
| A number suffix or a binary number | `1LL`, `0x10ULL`, `1i`, `0b101` | `The source contains a malformed Lua number.` |
| A line break without a backslash in a quoted string | `"a` and a new line | `The source contains an unfinished Lua string.` |
| A decimal escape above 255, or a `\u{...}` escape above `7FFFFFFF` | `"\256"` | `The source contains an invalid Lua escape sequence.` |

The grammar rejects one valid form: a backslash before a carriage return in a
quoted string. This occurs in files with Windows line endings. The adapter
masks this form before it parses the file, so the file passes.

`luac` also does checks after it parses a file. `ctc` does not do these
checks. Run `luac -p` as well. These are the checks:

- An assignment to the control variable of a `for` loop. This variable is
  read-only in Lua 5.5.
- An assignment to a `<const>` variable.
- An unknown attribute, such as `local x <foo>`.
- `<close>` on a global, or more than one `<close>` in one list.
- A label that occurs two times, a `goto` without a visible label, or a jump
  into the scope of a local.
- `break` outside a loop.

## Canonical kinds worth knowing

The adapter uses a fixed set of shared names. Other named kinds use the
PascalCase form of the grammar name.

| Kind | Use |
|---|---|
| `SourceFile` | The whole file |
| `FunctionDeclaration` | `function f()`, `function M.f()`, `function M:f()`, `local function f()`, `global function f()` |
| `FunctionExpression` | `function() ... end` as a value |
| `StatementBlock` | A function body, and the body of `do`, `while`, `repeat`, `for`, `if`, `elseif`, and `else` |
| `CallExpression` | Calls, including `f "text"`, `f{...}`, and `obj:method()` |
| `VariableDeclaration` | `local x = 1`, `global x` |
| `ImplicitVariableDeclaration` | `global *`, `global<const> *` |
| `AssignmentStatement` | `x = 1`, `t.x = 1` |
| `ReturnStatement` | `return M` |
| `TableConstructor` | `{ ... }` |
| `Field` | One item in a table constructor |
| `DotIndexExpression` | `t.x` |
| `MethodIndexExpression` | `obj:method` |
| `BracketIndexExpression` | `t[k]` |
| `Attribute` | `<const>` or `<close>` |
| `Identifier` | Names |
| `StringLiteral` | Strings in all forms |
| `NumericLiteral` | Numbers |

An empty body still has an empty `StatementBlock`. Thus `{{* Body }}`
matches `function f() end`.

A string literal has its decoded text as its value. Thus `"a"`, `'a'`,
`[[a]]`, and `"\97"` are equal.

Comments, `;` statements, and the separators `,` and `;` do not take part in
matching.

## Fields

| Field | Where it occurs | Meaning |
|---|---|---|
| `callee` | `CallExpression` | The called text without white space or comments, such as `print`, `string.format`, `self:emit`, or `_G.load` |
| `module` | `CallExpression` of `require` | The decoded text of the string argument, such as `game.parts` |
| `local` | `VariableDeclaration`, `ImplicitVariableDeclaration`, `FunctionDeclaration` | `true` when the declaration starts with `local` |
| `global` | `VariableDeclaration`, `ImplicitVariableDeclaration`, `FunctionDeclaration` | `true` when the declaration starts with `global` |

A plain `function M.run() end` has `local` and `global` set to `false`.

A `require` call has no `module` field when its argument is not a string
literal, or when the string is not UTF-8. `field("module", "notMatches", ...)`
passes when the field is missing. Thus a rule that allows only some modules
also finds `require(name)`.

Every node also supports `lineCount`.

Source nodes have these fields. Template literals do not have them.

## Supported sequence slots

Lua templates support sequence placeholders in:

- source-file statements
- block statements
- function parameters
- call arguments
- table constructor items
- expression lists, such as return values and the values of an assignment
- name lists, such as the names of a declaration

A placeholder alone on a line in a statement list is one statement.

Optional keyword placeholders support `local` and `global`. Use them before
`function`:

```lua
{{? keyword:local }} function {{ Name }}({{* Parameters }})
{{* Body }}
end
```

## Semantic rules

Lua has no Lua-only semantic rules today. It supports the generic
`companionFile` and `fileLength` rules.

## Suppression comments

Lua comments carry suppression comments:

```lua
-- ctc-ignore-next-line no-load -- the sandbox loads trusted text
local chunk = load(text)
--[[ ctc-ignore-next-line no-load ]]
local other = load(text)
```

`ctc guard` finds Lua suppression comments that a change adds.

## Known limits

- The adapter does syntax checks only. It does not resolve names. Thus it
  cannot tell an assignment to a local from an assignment to a global. Use
  Lua 5.5 global declarations for this check. See
  [Package module with read-only globals](#package-module-with-read-only-globals).
- `ctc` reads only UTF-8 files. A file with other bytes gets `CTC1009`. Use
  escapes such as `"\xff"` for other bytes in strings.
- In a template, `{{` starts a placeholder. Write a nested table constructor
  with a space, such as `{ {1, 2}, {3} }`.
- Each positional table item is a `Field` node with the value in it. A file
  with very large data tables can stop a `forbid` or `contains` search with
  `CTC2009`, because the search tries every list item. Exclude such data files
  from search rules.
- `{{? keyword:local }}` and `{{? keyword:global }}` work only before
  `function`. `local x = 1` and `x = 1` have different kinds.
- The adapter does not check the semantic errors that `luac -p` finds. See
  [Syntax errors](#syntax-errors).

## Rule recipes

Every recipe below was checked against `ctc 0.1.0` in a scratch project.

### No unrestricted loading

Goal: ban `load`, `loadstring`, `loadfile`, and `dofile`, also through `_G`
and `_ENV`.

Template file `.ctmpl/no-load.lua.ctmpl`:

```lua
{{ Call | kind("CallExpression") | field("callee", "matches", "^((_G|_ENV)[.])?(load|loadstring|loadfile|dofile)$") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-unrestricted-load",
  "template": ".ctmpl/no-load.lua.ctmpl",
  "include": ["packages/**/*.lua"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```lua
local Loader = {}

function Loader.run(text)
  local chunk = load(text)
  return chunk()
end

function Loader.include(path)
  return _G.dofile(path)
end

return Loader
```

Expected diagnostics: `packages/loader.lua:4:17` and
`packages/loader.lua:9:10`, both `CTC3006`.

The rule does not find a call through another name, such as
`local l = load; l(text)`. The runtime limits of the game must still remove
these functions.

### Package module structure

Goal: each package file makes one local table, adds to it, and returns it.

Template file `.ctmpl/module-shape.lua.ctmpl`:

```lua
local {{ Module }} = {}

{{* Body }}

return {{ Module }}
```

`.ctc.json` rule:

```json
{
  "id": "module-shape",
  "template": ".ctmpl/module-shape.lua.ctmpl",
  "include": ["packages/**/*.lua"],
  "mode": "exact"
}
```

Passing source:

```lua
local Parts = {}

local function total(...values)
  local sum = 0
  for i = 1, values.n do
    sum = sum + values[i]
  end
  return sum
end

function Parts.mass(part)
  return total(2, part.scale or 1)
end

return Parts
```

Violating source. The file returns another name:

```lua
local Parts = {}

return Other
```

Expected diagnostic: `packages/parts.lua:1:1 CTC3002`. The report says that
a `ReturnStatement` is missing. `{{* Body }}` takes `return Other`, so no
statement is left for `return Parts`.

### Package module with read-only globals

Goal: stop accidental global variables. In Lua 5.5, `global<const> *` makes
all free names read-only. Then `luac` rejects an assignment to an undeclared
global. This rule makes sure that each package file starts with this
declaration.

Template file `.ctmpl/strict-module.lua.ctmpl`:

```lua
global<const> *

local {{ Module }} = {}

{{* Body }}

return {{ Module }}
```

`.ctc.json` rule:

```json
{
  "id": "strict-module",
  "template": ".ctmpl/strict-module.lua.ctmpl",
  "include": ["packages/**/*.lua"],
  "mode": "exact"
}
```

Violating source. The declaration is missing:

```lua
local Parts = {}

return Parts
```

Expected diagnostic: `packages/parts.lua:1:1 CTC3002`. The message says that
the rule expected `ImplicitVariableDeclaration` and found
`VariableDeclaration`.

With the declaration, `luac -p` rejects `count = 1` in this file, because
`count` is read-only.

### No global declarations

Goal: ban `global x`, `global function f() end`, and `global *`.

Template file `.ctmpl/no-global.lua.ctmpl`:

```lua
{{ Declaration | field("global", "equal", true) }}
```

`.ctc.json` rule:

```json
{
  "id": "no-global-declarations",
  "template": ".ctmpl/no-global.lua.ctmpl",
  "include": ["packages/**/*.lua"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```lua
local M = {}
global Score
global function reset() end
return M
```

Expected diagnostics: `packages/a.lua:2:1` and `packages/a.lua:3:1`, both
`CTC3006`.

Do not use this rule together with the read-only globals recipe.

### No global functions

Goal: ban `function name() end`, which sets a global. Allow
`local function name()` and `function M.name()`.

Template file `.ctmpl/no-global-function.lua.ctmpl`:

```lua
function {{ Name | kind("Identifier") }}({{* Parameters }})
{{* Body }}
end
```

`.ctc.json` rule:

```json
{
  "id": "no-global-functions",
  "template": ".ctmpl/no-global-function.lua.ctmpl",
  "include": ["packages/**/*.lua"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```lua
local M = {}
local function helper() end
function M.run() end
function update() end
return M
```

Expected diagnostic: `packages/a.lua:4:1 CTC3006`

### Allowed modules for `require`

Goal: a package can load only modules under `game.`. The rule also finds a
`require` call whose argument is not a string literal.

Template file `.ctmpl/package-require.lua.ctmpl`:

```lua
{{ Call | field("callee", "equal", "require") | field("module", "notMatches", "^game\\.") }}
```

`.ctc.json` rule:

```json
{
  "id": "package-requires",
  "template": ".ctmpl/package-require.lua.ctmpl",
  "include": ["packages/**/*.lua"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```lua
local Shapes = require("game.shapes")
local Json = require("dkjson")
local name = "os"
local Os = require(name)
```

Expected diagnostics: `packages/a.lua:2:14` and `packages/a.lua:4:12`, both
`CTC3006`.

### Function length

Goal: fail long functions with `lineCount`.

Template file:

```lua
{{ Long | kind("FunctionDeclaration", "FunctionExpression") | field("lineCount", "greaterThan", 4) }}
```

The limit is 4 lines here only to keep the sample short. Use a real limit,
such as 60 or 100.

`.ctc.json` rule:

```json
{
  "id": "short-functions",
  "template": ".ctmpl/long-function.lua.ctmpl",
  "include": ["packages/**/*.lua"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```lua
local M = {}
M.run = function()
  local a = 1
  local b = 2
  return a + b
end
return M
```

Expected diagnostic: `packages/a.lua:2:9 CTC3006`

### File length

Goal: limit total file size without parsing syntax.

`.ctc.json` rule:

```json
{
  "kind": "fileLength",
  "id": "short-files",
  "include": ["packages/**/*.lua"],
  "maxLines": 3
}
```

Violating source:

```lua
local M = {}
M.a = 1
M.b = 2
return M
```

Expected diagnostic: `packages/a.lua:4:1 CTC4301`
