# Lua guide

This page covers the Lua adapter: file suffixes, template names, Lua 5.5
syntax support, useful kinds and fields, the Lua project rules, known limits,
and verified recipes.

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
| `module` | `CallExpression` of `require` | The decoded text of the string argument, such as `app.parts` |
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

Lua supports the generic `companionFile` and `fileLength` rules. It also has
two rules for global variables:

| Kind | Use it for | Details |
|---|---|---|
| `accidentalGlobals` | Reads and assignments of global variables that the project did not allow | [Accidental globals](#accidental-globals) |
| `restrictedGlobals` | Uses of functions that load code or reach the host | [Unrestricted loading](#unrestricted-loading) |

## Suppression comments

Lua comments carry suppression comments:

```lua
-- ctc-ignore-next-line no-load -- the sandbox loads trusted text
local chunk = load(text)
--[[ ctc-ignore-next-line no-load ]]
local other = load(text)
```

`ctc guard` finds Lua suppression comments that a change adds.

The rules `accidentalGlobals` and `restrictedGlobals` accept suppression
comments when they set `allowIgnore` to `true`. A comment skips the diagnostics
of the next line. `fileLength` does not parse the file, so a comment has no
effect on it.

## Lua project rules

Lua scripts need checks that go beyond the shape of the code. This section
gives four checks and the rules that do them. A project turns the rules on in
`.ctc.json`.

| Check | Rule | Codes |
|---|---|---|
| Accidental globals | `accidentalGlobals` semantic rule | `CTC4401`, `CTC4402` |
| Unrestricted loading | `restrictedGlobals` semantic rule | `CTC4403`, `CTC4404`, `CTC4405` |
| Package module structure | Templates | `CTC3002`, `CTC3003`, `CTC3006` |
| Large functions | Template | `CTC3006` |

A template cannot tell a local variable from a global variable. Thus the first
two checks are semantic rules, and the adapter resolves names for them. The
other two checks match syntax, so they are templates. Change the templates for
your project.

The folder [tests/fixtures/lua-rules](../../tests/fixtures/lua-rules) has a
complete setup: the template files, a `.ctc.json` file, and Lua files that pass
and Lua files that fail each rule. File size, coverage, and `guard` are in
[File size, coverage, and guard](#file-size-coverage-and-guard).

### Accidental globals

The `accidentalGlobals` rule finds each use of a global variable. A global
variable is a name that no declaration binds. These declarations bind a name:

- `local x` and `local function f`.
- A function parameter, `self` in a method, and the name after `...`.
- A loop variable of a `for` statement.
- A Lua 5.5 declaration: `global x`, `global function f`, and `global *`.

A binding starts after its declaration and ends with its block. Thus
`local x = x` reads an outer `x`. A function that calls a local function
before the declaration of this local function reads a global variable.

The rule reports these uses:

| Code | Use |
|---|---|
| `CTC4401` | The code reads a global variable that `allow` does not list. |
| `CTC4402` | The code assigns a global variable that `allowWrite` does not list. |

`function name() end` assigns the global variable `name`.
`function M.name() end` reads `M` and assigns a field of `M`.

These fields configure the rule:

| Field | Default | Meaning |
|---|---|---|
| `allow` | `[]` | Names that the code can read. Use the names that the host gives, such as `ipairs` and `math`. |
| `allowWrite` | `[]` | Names that the code can assign. |

A name has ASCII letters, digits, and `_`, and it does not start with a digit.
`allow` lists names, not paths: `math` in `allow` allows `math.floor`.
`allowWrite` does not allow reads. When the code reads and assigns a name, put
the name in both fields.

`.ctc.json` rule:

```json
{
  "kind": "accidentalGlobals",
  "id": "no-accidental-globals",
  "include": ["packages/**/*.lua"],
  "allow": ["ipairs", "math", "pairs", "require", "string", "table", "tostring"]
}
```

Violating source:

```lua
local counter = {}

function counter.run()
  return helper(1)
end

local function helper(value)
  total = value
  return total
end

return counter
```

Expected diagnostics: `packages/counter.lua:4:10 CTC4401` for `helper`, which
is a local variable only after line 7, then `packages/counter.lua:8:3 CTC4402`
and `packages/counter.lua:9:10 CTC4401` for `total`.

These rules apply:

- A field access reads the first name. `math.floor` reads `math`.
- `_G.name` and `_ENV.name` read the global variable `name`, and `_G.name = 1`
  assigns it. The rule does not report the use of `_G` in `_G.name`. Any other
  use of `_G` is a read of `_G`. Put `_G` in `allow` to accept it.
- `_G[key]` is a global variable that no rule can name when the key is not a
  string literal. The assignment `_G[key] = value` gives `CTC4402`. A read
  gives nothing here, but `restrictedGlobals` reports it.
- A local `_ENV` replaces the global table. The rule reports nothing in the
  scope of a local `_ENV`.
- `global *` and `global<const> *` declare every free name in their block. The
  rule reports nothing there. Do not use the rule on files that start with this
  declaration.

### Unrestricted loading

The `restrictedGlobals` rule finds the uses of functions that load code or
reach the host. It checks each use of a name, not only calls. Thus it also
finds `pcall(load, text)` and `local f = load`. A local variable with the same
name is not a use of the global variable.

The rule reports these uses:

| Code | Use |
|---|---|
| `CTC4403` | The code uses a name that `forbid` lists. |
| `CTC4404` | The code uses `require` in another way than `require("name")`. |
| `CTC4405` | The code indexes `_G` or `_ENV` with a key that is not a string literal. |

These fields configure the rule:

| Field | Default | Meaning |
|---|---|---|
| `forbid` | See the list below | Names and paths that the code cannot use. |
| `forbidDynamicRequire` | `true` | Report `CTC4404`. |

The default `forbid` list is `load`, `loadfile`, `loadstring`, `dofile`,
`setfenv`, `collectgarbage`, `string.dump`, `debug`, `io`, `os`, and `package`.
A `forbid` list in the rule replaces the default list. An empty list turns the
name check off.

A name in `forbid` forbids itself and every name below it. `debug` forbids
`debug.getinfo`. `string.dump` forbids only `string.dump`. The rule also
reports a value that holds a forbidden name, such as `local s = string` and
`string[key]` when `string.dump` is forbidden.

`require("app.parts")` and `require "app.parts"` pass. `require(name)`,
`require("a" .. b)`, and `pcall(require, "x")` give `CTC4404`. To limit the
module names, add a template rule. See
[Allowed modules for `require`](#allowed-modules-for-require).

`.ctc.json` rule:

```json
{
  "kind": "restrictedGlobals",
  "id": "no-unrestricted-loading",
  "include": ["packages/**/*.lua"]
}
```

Violating source:

```lua
local Loader = {}

function Loader.run(text, name)
  local chunk = load(text)
  local clock = os.clock()
  local module = require(name)
  return chunk, clock, module
end

return Loader
```

Expected diagnostics: `packages/loader.lua:4:17 CTC4403` for `load`,
`packages/loader.lua:5:17 CTC4403` for `os.clock`, and
`packages/loader.lua:6:18 CTC4404` for `require(name)`.

`accidentalGlobals` also reports a forbidden name that its `allow` list does
not have. Both reports are correct. Do not put a forbidden name in `allow`.

### Module structure rules

A package module is a file that makes a local table, adds fields to it, and
returns it. These rules check the structure with templates:

- The top level has no code that runs when the module loads. It has local
  definitions and fields of the module table.
- The file ends with `return` and one table.

The templates do not need one table named in the first line. Thus they fit an
entry point that has several local tables and a table that maps module names to
them, as the core package has. For a file with exactly one local table, use the
exact template in
[Package module structure](#package-module-structure).

Template file `.ctmpl/module-top-level-statement.lua.ctmpl`. It finds a call,
a `do`, `while`, `repeat`, `if`, or `for` statement, a `goto`, and a label:

```lua
{{ Statement | kind("CallExpression", "DoStatement", "WhileStatement", "RepeatStatement", "IfStatement", "ForStatement", "GotoStatement", "LabelStatement") }}
```

Template file `.ctmpl/module-global-assignment.lua.ctmpl`. It finds an
assignment to names:

```lua
{{* Targets | kind("Identifier") }} = {{* Values }}
```

Template file `.ctmpl/module-global-function.lua.ctmpl`. It finds
`function name() end`, which assigns a global variable:

```lua
function {{ Name | kind("Identifier") }}({{* Parameters }})
{{* Body }}
end
```

Template file `.ctmpl/module-return-table.lua.ctmpl`:

```lua
return {{ Module | kind("Identifier", "TableConstructor") }}
```

`.ctc.json` rules. The first rule lists three templates. The rule fails a file
when any one of them matches:

```json
{
  "id": "module-no-top-level-effects",
  "template": [
    ".ctmpl/module-top-level-statement.lua.ctmpl",
    ".ctmpl/module-global-assignment.lua.ctmpl",
    ".ctmpl/module-global-function.lua.ctmpl"
  ],
  "include": ["packages/**/*.lua"],
  "mode": "forbid",
  "scope": "topLevel"
},
{
  "id": "module-returns-table",
  "template": ".ctmpl/module-return-table.lua.ctmpl",
  "include": ["packages/**/*.lua"],
  "mode": "contains"
}
```

Passing source:

```lua
local good = {}

local limit = 3

local function clamp(value)
  return math.min(value, limit)
end

function good.run(value)
  return clamp(value)
end

good.version = 1

return good
```

Violating source:

```lua
local effects = {}

print("loading")
effects.ready = true
counter = 0

function register()
  return effects
end

return effects
```

Expected diagnostics: `packages/effects.lua:3:1`, `packages/effects.lua:5:1`,
and `packages/effects.lua:7:1`, all `CTC3006`. `effects.ready = true` passes,
because it adds a field to the module table.

Violating sources for the second rule:

```lua
local first = {}
local second = {}

return first, second
```

Expected diagnostic: `packages/two.lua:4:15 CTC3003`. A file without a
`return` gets `CTC3002`, and the report says that a `ReturnStatement` is
missing.

The rules do not know the type of the returned name. `return count` passes when
`count` is a number. They also do not find an assignment to a field of a
different table, such as `string.dump = nil`. `restrictedGlobals` reports this
case.

### Module names for `require`

Template file `.ctmpl/package-require.lua.ctmpl`. It finds a `require` call
whose argument is not a string literal of the form `package.module`, with
lowercase letters, digits, and `_`:

```lua
{{ Call | field("callee", "equal", "require") | field("module", "notMatches", "^[a-z0-9_]+(\\.[a-z0-9_]+)+$") }}
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

`require("dkjson")` and `require(name)` fail this rule. `require("core.math3")`
passes.

### Large functions

Template file `.ctmpl/long-function.lua.ctmpl`. The number 100 is the limit.
Change it to change the limit:

```lua
{{ Long | kind("FunctionDeclaration", "FunctionExpression") | field("lineCount", "greaterThan", 100) }}
```

`.ctc.json` rule:

```json
{
  "id": "short-lua-functions",
  "template": ".ctmpl/long-function.lua.ctmpl",
  "include": ["packages/**/*.lua"],
  "mode": "forbid",
  "scope": "descendants",
  "message": "Split this function. A function can span at most 100 lines."
}
```

The template counts all lines of the function, from the line of `function` (or
`local`) to the line of `end`. Comments and blank lines count. A function of 100
lines passes. A function of 101 lines fails with `CTC3006` at its first line. A
function in a function counts in the outer function too.

### Hook functions of a module

A module can export a function that the host calls. This template requires a
function with two parameters. The names of the parameters do not matter.

Template file `.ctmpl/pre-step.lua.ctmpl`:

```lua
function {{ Module }}.pre_step({{ Ctx }}, {{ Part }})
{{* Body }}
end
```

`.ctc.json` rule:

```json
{
  "id": "part-has-pre-step",
  "template": ".ctmpl/pre-step.lua.ctmpl",
  "include": ["packages/*/scripts/server/parts/*.lua"],
  "mode": "contains",
  "scope": "topLevel",
  "message": "A part behavior must define pre_step(ctx, part)."
}
```

A module with `function wheel.pre_step(ctx)` fails with `CTC3002`. The report
says that the capture `Part` is missing.

## File size, coverage, and guard

These parts of `ctc` work for Lua files in the same way as for the other
languages:

- `fileLength` counts the lines of a file and does not parse it. A Lua file with
  more lines than `maxLines` gets `CTC4301` at its first line over the limit.
  See [File length](#file-length).
- `ctc coverage` lists each `.lua` file in the watched area that no rule
  selects. A Lua template rule, a Lua semantic rule, and a `fileLength` or
  `companionFile` rule select a Lua file with their `include` patterns. A
  template rule of another language does not select it. An uncovered file gets
  `CTC5101`.
- `ctc guard` reads the Lua comments of each source file. A
  `-- ctc-ignore-next-line` comment that a change adds gets `CTC5201`. A rule
  that selects fewer files, a rule that was removed, and a changed rule
  definition get `CTC5202` to `CTC5204`. A change of a field such as `allow` or
  `forbid` is a changed rule definition.

Use all three after each change:

```text
ctc
ctc coverage
ctc guard --base origin/main
```

## Known limits

- Templates see syntax only. They cannot tell an assignment to a local from an
  assignment to a global. The adapter resolves names for the semantic rules
  `accidentalGlobals` and `restrictedGlobals`. Use these rules for checks that
  need a name.
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
- A template is a list of statements. An expression such as
  `{{ A }} .. {{ B }}` is not a statement, so it is not a valid template. Use a
  placeholder with a kind, such as
  `{{ Operation | kind("BinaryExpression", "UnaryExpression") }}`. A template
  cannot test the operator.
- The adapter does not check the semantic errors that `luac -p` finds. See
  [Syntax errors](#syntax-errors).
- The global rules follow names. They do not follow values. A call such as
  `ctx.load(text)` or `rawget(_G, name)` is outside the rules. The host's
  runtime limits stay necessary.

## Rule recipes

Every recipe below was checked against `ctc 0.1.0` in a scratch project. The
templates of the [Lua project rules](#lua-project-rules) are checked by the
tests of this repository.

### No unrestricted loading

Goal: ban `load`, `loadstring`, `loadfile`, and `dofile`, also through `_G`
and `_ENV`.

This recipe checks calls only. For the full check, use the rule
[`restrictedGlobals`](#unrestricted-loading). Use the template when you need a
call pattern of your own, such as a method call.

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
`local l = load; l(text)`. The host's runtime limits must still remove these
functions.

### Package module structure

Goal: each package file makes one local table, adds to it, and returns it.

This exact template fits a file with one local table. For a file with several
local tables, use the [module structure rules](#module-structure-rules).

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

Lua scripts use Lua 5.4 syntax for now, and Lua 5.4 does not have this
declaration. For these scripts, use the rule
[`accidentalGlobals`](#accidental-globals). It reports nothing in a file that
starts with `global<const> *`.

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

Goal: a package can load only modules under `app.`. The rule also finds a
`require` call whose argument is not a string literal.

Template file `.ctmpl/package-require.lua.ctmpl`:

```lua
{{ Call | field("callee", "equal", "require") | field("module", "notMatches", "^app\\.") }}
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
local Shapes = require("app.shapes")
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
such as 60 or 100. The [large functions](#large-functions) rule uses 100.

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
