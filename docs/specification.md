# Code Template Check Specification

**Status:** Draft  
**Specification version:** 0.3

## 1. Purpose

Code Template Check (`ctc`) checks source files against code-shaped templates.
The templates contain placeholders that match syntax nodes.

The tool supports TypeScript, C++, Rust, and Lua source files. All adapters use
the same rule loading and matching system.

This document defines the required MVP behavior. The
[implementation plan](plans/implementation-plan.md) describes the work order.
If the two documents conflict, this specification wins.

## 2. Requirement Terms

`MUST` identifies a required behavior.

`MUST NOT` identifies a prohibited behavior.

`CAN` identifies permitted behavior.

## 3. MVP Scope

The MVP MUST provide:

- One native executable named `ctc`.
- TypeScript syntax matching for `.ts`, `.mts`, and `.cts` files.
- C++ syntax matching for common source-file and header-file suffixes.
- Root `.ctc.json` configuration.
- Include and exclude globs.
- Shareable templates under `.ctmpl`.
- Literal syntax matching.
- Scalar and sequence captures.
- Repeated capture equality.
- Identifier prefix and suffix filters.
- Node-kind and canonical-field constraints.
- Exact, contains, forbid, and every match modes.
- Suppression comments.
- Companion-file and C++ header and source pairing semantic rules.
- Human and JSON diagnostics.
- Stable exit codes.

The MVP MUST NOT require:

- Node.js.
- A TypeScript installation.
- A C++ compiler or Clang installation.
- A `compile_commands.json` file.
- A `tsconfig.json` file.
- Type checking.
- Module resolution.
- Network access.

## 4. Deferred Scope

The MVP does not include:

- JavaScript, JSX, or TSX parsing.
- TypeScript declaration files.
- C parsing for `.c` files.
- Clang integration.
- Cross-file captures. The only rules that read two files are the semantic
  rules in sections 34.5 and 34.6.
- Template alternation.
- Template imports.
- Automatic source changes.
- User-defined filters.
- Semantic type constraints.

## 5. Executable and Runtime

The release executable MUST be named `ctc`.

The executable MUST run without Node.js or another language runtime.

The executable MUST include the TypeScript, C++, Rust, and Lua Tree-sitter
grammars.

Each operating system and processor target CAN have a separate executable.

The command `ctc --version` MUST write this format:

```text
ctc <semantic-version>
```

The command `ctc --help` MUST describe all MVP commands and options.

## 6. Implementation Architecture

The implementation MUST use a Rust Cargo workspace.

All workspace crates MUST use the Rust 2024 edition.

The workspace MUST contain these logical components:

- A language-neutral core matcher.
- A TypeScript language adapter.
- A C++ language adapter.
- A CLI executable.

The language-neutral matcher MUST NOT contain Tree-sitter node types.

The language-neutral matcher MUST NOT contain TypeScript-specific or
C++-specific logic.

Each language adapter MUST own:

- Source parsing.
- Placeholder context detection.
- Sentinel generation rules.
- Canonical node conversion.
- Canonical node-kind names.
- Canonical field names.
- Identifier validation.
- Parse diagnostics.

The CLI MUST register the TypeScript, C++, Rust, and Lua adapters at startup.

## 7. Language Adapter Contract

A language adapter MUST provide:

```rust
pub trait LanguageAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn supports_path(&self, path: &Path) -> bool;
    fn scan_placeholders(
        &self,
        source: &str,
        path: &Path,
    ) -> Result<Vec<PlaceholderRange>, Vec<Diagnostic>>;
    fn parse(&self, source: &str, path: &Path)
        -> Result<ParsedTree, Diagnostic>;
    fn compile_template(
        &self,
        source: &str,
        path: &Path,
        placeholders: &[Placeholder],
    ) -> Result<TemplateTree, Vec<Diagnostic>>;
    fn validate_identifier(&self, value: &str) -> bool;
    fn known_kind(&self, value: &str) -> bool;
    fn known_field(&self, value: &str) -> bool;
}
```

An adapter MUST return owned canonical nodes to the core matcher.

An adapter MUST NOT expose parser object lifetimes through the public core API.

An adapter MUST give the same canonical result for equivalent source
formatting.

A parsed source MUST include the byte range of each comment node in the raw
parser tree. The core reads suppression comments from these ranges. See
[Section 38.1](#381-suppression-comments).

## 8. Source Encoding

Source files and templates MUST use UTF-8.

A UTF-8 byte-order mark is permitted.

The tool MUST report invalid UTF-8 as an input diagnostic.

The tool MUST accept LF and CRLF line endings.

Line-ending differences MUST NOT affect matching.

## 9. Scan Root

`ctc` MUST use the current directory as the default scan root.

The `--root <path>` option MUST replace the default scan root.

The tool MUST resolve the scan root to an absolute normalized path.

An explicit source path MUST be inside the scan root.

The tool MUST NOT require the scan root to be a Git repository.

## 10. Project Configuration

The default configuration path is `<root>/.ctc.json`.

`--config <path>` MUST select another configuration file inside the scan root.

A relative `--config` path resolves from the scan root.

The configuration MUST use UTF-8 JSON.

The root object MUST contain:

- `schemaVersion`
- `rules` or `semanticRules`

`schemaVersion` MUST equal `1`.

`rules` and `semanticRules` MUST contain at least one rule in total.

Unknown object fields MUST produce an invalid-configuration diagnostic.

## 11. Rule Configuration

Each rule object supports:

```json
{
  "id": "no-value-imports",
  "template": ".ctmpl/no-value-imports.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "exclude": ["src/index.ts"],
  "mode": "forbid",
  "scope": "topLevel",
  "message": "Use `import type` outside the composition root."
}
```

`id`, `template`, and `include` are required.

`exclude` is optional and defaults to an empty array.

`mode` is optional and defaults to `exact`.

`scope` is optional and defaults to `topLevel`.

`message` is optional. See section 11.1.

`kinds` is optional. It is a non-empty array of canonical node kinds that an
every rule checks. See section 34.4. A rule that sets `kinds` without `every`
mode, sets it to an empty array, or lists a kind that the template's language
adapter does not know MUST produce an invalid-configuration diagnostic.

`count` is optional. It is an object with `min` and `max` integers that limits
the number of matches of a contains rule. See section 33.1. A rule that sets
`count` without contains mode, sets `min` to `0`, or sets `max` below `min` MUST
produce an invalid-configuration diagnostic. `min` defaults to `1`. `max` has no
default.

`allowIgnore` is optional and MUST be a Boolean. It defaults to `false`. See
section 38.1.

`scope` is either a scope name or a scope object. See section 34.1.

`template` is a path or a non-empty array of paths. A rule with several
templates accepts a file when any of its templates accepts it. See section
17.1.

A rule identifier MUST match:

```text
[a-z][a-z0-9-]*
```

Rule identifiers MUST be unique and case-sensitive.

Each rule MUST contain at least one include pattern.

Every selected rule MUST include at least one supported source file before CLI
source filtering.

### 11.1 Rule Messages

Template rules and semantic rules accept an optional `message` string.

A `message` value MUST be a JSON string. It MUST contain at least one
character that is not white space. It MUST NOT contain a line break. A value
that breaks these rules MUST produce an invalid-configuration diagnostic.

A rule failure is a diagnostic with exit class `1`, a rule identifier, and a
code in the `CTC3xxx` or `CTC4xxx` range that a rule reports for a source file.

The tool MUST attach the rule message to each rule failure of that rule. It
MUST NOT attach the rule message to any other diagnostic, such as
command-line, configuration, template (`CTC1xxx` and `CTC2xxx`), source parse,
or internal diagnostics.

The rule message MUST NOT replace the engine message. Section 42 and section 43
define how each output format shows both messages.

A rule without `message` MUST report only the engine message, with no
`ruleMessage` field and no `Detail` line.

## 12. Shareable Templates

Template paths resolve from the scan root.

A template path MUST stay inside the scan root.

A template filename MUST contain one supported source suffix before `.ctmpl`.

Examples:

```text
.ctmpl/no-value-imports.ts.ctmpl
.ctmpl/class-factory.ts.ctmpl
```

The `.ctmpl` directory CAN be a Git submodule. Project-specific include and
exclude patterns remain in `.ctc.json`.

### 12.1 Semantic Rules

Semantic rules do not use templates. The supported semantic kinds are:

- `returnPaths`
- `exceptionPolicy`
- `companionFile`
- `headerSourcePairing`
- `fileLength`
- `accidentalGlobals`
- `restrictedGlobals`

All kinds require `id` and `include`. `exclude`, `message`, and `allowIgnore` are optional.
`message` follows section 11.1.

`returnPaths` and `exceptionPolicy` select TypeScript files only.
`companionFile` selects any supported source file. `headerSourcePairing`
selects C++ files only. `accidentalGlobals` and `restrictedGlobals` select Lua
files only. A configured kind whose language adapter is not
registered MUST produce an invalid-configuration diagnostic.

`returnPaths` requires `typeName`, a non-empty string. A rule without it MUST produce an
invalid-configuration diagnostic.

`exceptionPolicy` accepts:

- `forbidTry`
- `forbidThrow`
- `forbidPromiseReject`
- `exceptionSources`

The Boolean fields default to `false`. `exceptionSources` defaults to an empty
array.

`companionFile` requires `companions`, a non-empty array of partner patterns.

`fileLength` requires `maxLines`, a positive integer. It selects any supported
source file. It counts the lines of the file, the same way a text editor does,
and does not parse the file. A file with more than `maxLines` lines MUST produce
one `CTC4301` diagnostic with exit class `1`, at the start of the first line
over the limit. The diagnostic carries `at most <n> lines` as expected and
`<n> lines` as actual. Because the file is not parsed, suppression comments do
not apply to a `fileLength` rule. `maxLines` of `0` MUST produce an
invalid-configuration diagnostic.

`accidentalGlobals` accepts:

- `allow`: an array of global names. It defaults to an empty array.
- `allowWrite`: an array of global names. It defaults to an empty array.

`restrictedGlobals` accepts:

- `forbid`: an array of global names and dotted paths. When it is missing, the
  rule MUST use the default list of section 34.7.
- `forbidDynamicRequire`: a Boolean. It defaults to `true`.

A global name consists of ASCII letters, digits, and `_`, and it MUST NOT start
with a digit. A dotted path consists of global names that dots separate. An
entry that does not follow these forms MUST produce an invalid-configuration
diagnostic. The Lua semantic kinds do not accept the fields of other kinds.

`headerSourcePairing` requires `sources`, a non-empty array of partner
patterns. It accepts:

- `missingSource`: `"skip"` or `"report"`. It defaults to `"skip"`.
- `checkOrder`: a Boolean. It defaults to `true`.

A partner pattern is a path relative to the scan root with `/` separators.
These tokens MUST be replaced with values from the selected file's path
relative to the scan root:

- `{dir}`: the directory of the file.
- `{subdir}`: `{dir}` without its first segment.
- `{stem}`: the file name before its last dot.
- `{ext}`: the text after the last dot of the file name.

After replacement, empty and `.` segments MUST be removed.

A pattern MUST produce an invalid-configuration diagnostic when it is empty,
contains `\`, starts with `/`, contains a `..` segment, contains an unknown
token, or contains an unmatched brace.

Candidates are tried in configured order. A candidate equal to the selected
file and a repeated candidate MUST be skipped. The partner is the first
candidate that is a regular file inside the scan root. No path component below
the scan root MAY be a symbolic link.

A partner file does not need to match any rule's `include` pattern.

## 13. Language Selection

The template source suffix determines the language adapter.

The MVP mappings are:

| Template suffix | Adapter |
|---|---|
| `.ts.ctmpl` | TypeScript |
| `.mts.ctmpl` | TypeScript |
| `.cts.ctmpl` | TypeScript |
| `.cpp.ctmpl` | C++ |
| `.cc.ctmpl` | C++ |
| `.cxx.ctmpl` | C++ |
| `.h.ctmpl` | C++ |
| `.hh.ctmpl` | C++ |
| `.hpp.ctmpl` | C++ |
| `.hxx.ctmpl` | C++ |
| `.rs.ctmpl` | Rust |
| `.lua.ctmpl` | Lua |

Selected source files MUST use the same language adapter as their template.

An unsupported template or source suffix MUST produce a diagnostic.

TypeScript declaration files are outside the MVP.

C files with the `.c` suffix are outside the MVP.

## 14. Glob Syntax

`include` and `exclude` patterns are relative to the scan root.

Patterns MUST use `/` separators on all operating systems.

Patterns MUST NOT be absolute.

Patterns MUST NOT contain backslashes or parent components.

The MVP supports standard glob tokens:

- `*`
- `?`
- `**`
- Character classes
- Brace alternatives

Matching follows the case rules of the host operating system.

## 15. Rule Selection

A source file is selected when it matches one include pattern and no exclude
pattern for a rule.

All rules that select one source file MUST run independently.

Captures MUST NOT pass between rules or source files.

The tool MUST de-duplicate normalized source paths.

## 16. Source Scan

The tool MUST recursively scan the root for supported source files.

Source expansion MUST NOT enter:

- `.git`
- `.ctmpl`
- `node_modules`
- `target`

Source expansion MUST NOT follow directory symbolic links.

The tool MUST parse each selected source file once per command.

## 17. Configured Match Mode

Configured rules get their match mode from `.ctc.json`.

The supported values are:

- `exact`
- `contains`
- `forbid`
- `every`

The configured mode overrides a template mode directive.

Template mode directives remain available for ad hoc templates.

### 17.1 Several Templates

A rule can list several templates. All of them MUST be for the same language.
The rule cannot set `count`, and an every rule needs `kinds`. A rule that breaks
these limits MUST produce an invalid-configuration diagnostic. The templates
run against each selected file, and the results combine by mode:

- `exact` and `contains` pass when any template passes. When none passes, the
  tool reports the diagnostics of the template that failed the furthest into
  the source file, judged by the largest source offset of its diagnostics. The
  first listed template wins a tie. This is a heuristic: the diagnostic names
  its template, so the reader can see which alternative it describes.
- `forbid` reports the matches of every template.
- `every` fails a candidate only when every template fails it. The candidate
  kinds come from `kinds`, so all templates check the same candidates.

A template error in any template is reported once and stops the rule for that
file. `explain --rule` supports rules with one template only.

## 18. Explicit CLI Source Filters

Positional paths after `ctc` limit the configured source set.

A positional path can name a file or directory.

A directory path includes matching source files below that directory.

Configuration validation occurs before this CLI filter.

An explicit file with no applicable rule MUST produce an error.

An explicit filter that leaves no selected source files MUST produce an error.

The filter selects only the files that rules check. A rule that reads a partner
file (sections 34.5 and 34.6) MUST read it even when the filter does not select
it.

## 19. Rule Filters

`--rule <id>` limits execution to one configured rule identifier.

The option is repeatable.

An unknown rule identifier MUST produce an error.

Rule filtering occurs before source parsing.

## 20. Ad Hoc Templates

`--template` bypasses `.ctc.json`.

Ad hoc mode requires one or more explicit source paths.

`--mode` overrides a mode directive in an ad hoc template.

`--message` CAN set a rule message for the ad hoc rule. See section 11.1.

An ad hoc every template checks only nodes of the template node's kind, because
`kinds` is available only in `.ctc.json`.

`--template` MUST NOT occur with `--config` or `--rule`.

A `fileName` filter in an ad hoc template uses the file name of each explicit
source path.

## 21. Template Lexing

The language adapter MUST identify placeholder candidates outside protected
language regions.

For TypeScript, protected regions include:

- Line comments.
- Block comments.
- String literals.
- Regular-expression literals.
- Template-literal text.

TypeScript template-literal expressions are source code. The adapter MUST scan
their expression contents for placeholders.

For C++, protected regions include:

- Line comments.
- Block comments.
- String literals.
- Character literals.
- Raw string literals.

C++ preprocessor directives are source code. The adapter MUST scan their
contents for placeholders.

For Lua, protected regions include:

- Line comments that start with `--`.
- Long comments such as `--[[ ... ]]` and `--[==[ ... ]==]`.
- Quoted strings with `"` or `'`.
- Long strings such as `[[ ... ]]` and `[==[ ... ]==]`.
- A first line that starts with `#`.

A placeholder MUST stay on one line.

An opening `{{` outside a protected region starts a placeholder.

A placeholder without a closing `}}` on the same line is invalid.

A placeholder body that does not match the placeholder grammar is invalid.

## 22. Placeholder Grammar

The grammar is:

```text
placeholder :=
  "{{" whitespace?
  cardinality-marker?
  category?
  capture-name
  filter*
  whitespace? "}}"

cardinality-marker := sequence-marker | optional-marker
sequence-marker := "*" whitespace?
optional-marker := "?" whitespace?
category := category-name ":" whitespace?
filter := whitespace? "|" whitespace? filter-call
filter-call := filter-name "(" arguments? ")"
arguments := json-scalar ("," whitespace? json-scalar)*
```

A capture name MUST match:

```text
[A-Za-z_][A-Za-z0-9_]*
```

The MVP categories are:

- `identifier`
- `expression`
- `type`
- `keyword`

An omitted category requests context inference.

The sequence marker changes the placeholder cardinality from one node to zero
or more list items.

The optional marker changes the placeholder cardinality from one node to zero
or one list item.

`keyword` requires the optional marker and a known language keyword.

JSON arrays and objects are not valid filter arguments.

## 23. Scalar Captures

The first plain use of a capture name stores its canonical node.

A later plain use of the same name MUST equal the stored canonical node.

Source ranges do not participate in capture equality.

A repeated name MUST keep the same category and cardinality.

A category change MUST produce a template diagnostic.

## 24. Sequence Captures

A sequence placeholder matches zero or more sibling nodes in one syntax list.

The TypeScript MVP supports sequence placeholders in:

- Source-file statements.
- Function parameters.
- Call arguments.
- Constructor arguments.
- Interface members.
- Class members.

The C++ adapter supports sequence placeholders in:

- Translation-unit declarations.
- Function statements.
- Function parameters.
- Call arguments.
- Class and structure members.
- Initializer lists.
- Template parameters and arguments.

A sequence placeholder MUST occupy one complete list item.

Other syntax MUST NOT share that item.

Two sequence placeholders CAN be adjacent. Each placeholder in a run of adjacent
sequence placeholders, except the last one, MUST have a `kind` or `field`
constraint. A missing constraint MUST produce an ambiguous-sequence diagnostic.

In a run of adjacent sequence placeholders, a source node belongs to the first
placeholder whose constraints it passes. A later placeholder MUST NOT capture a
node that passes the constraints of an earlier placeholder in the same run. So
the run requires the source nodes to appear in the same order as the
placeholders.

A repeated sequence name MUST equal the first canonical node list.

Constraints on a sequence apply to every captured node.

### 24.1 Optional Captures

An optional placeholder matches zero or one sibling node.

The matcher MUST try the absent form before the present form.

An optional capture that is present stores one canonical node.

`{{? keyword:async }}` matches the literal `async` token or no token.

An optional keyword MUST NOT match another keyword.

An unknown language keyword MUST produce a template diagnostic.

An optional keyword MUST NOT use filters.

## 25. Derived Identifier Filters

The MVP supports:

- `prefix`
- `suffix`
- `removePrefix`
- `removeSuffix`
- `fileName` (section 25.1)

Examples:

```text
{{ Name | prefix("create") }}
{{ Name | suffix("Implementation") }}
{{ InterfaceName | removePrefix("I") | prefix("create") }}
{{ Name | removeSuffix("Service") }}
```

Each filter requires one JSON string.

`prefix` and `suffix` concatenate exact text without changing letter case.

`removePrefix` and `removeSuffix` remove exact text without changing letter
case. The derived use does not match when the value does not start or end with
that text.

For example, when `InterfaceName` captures `ILogger`,
`{{ InterfaceName | removePrefix("I") | prefix("create") }}` expects
`createLogger`.

Filters run from left to right.

The derived value MUST be a valid identifier for the selected language.

The source capture can occur before or after a derived use.

Every derived source name MUST have one plain capture in the template.

A derived use before its source capture MUST defer comparison until the source
capture is known.

A derived mismatch diagnostic MUST include the exact expected identifier and
the actual identifier.

A naming filter on a non-identifier capture MUST produce a template diagnostic.

### 25.1 File Name Filter

`fileName` requires a captured identifier to match the source file name:

```text
{{ Name | fileName("PascalCase") }}
{{ Name | removePrefix("I") | fileName("PascalCase") }}
```

`fileName` requires one JSON string. The string MUST be one of:

- `PascalCase`
- `camelCase`
- `snake_case`
- `asIs`

`kebab-case` is not supported, because an identifier cannot contain `-`.

`fileName` is a name filter. The rules for name filters in this section and in
section 26 apply to it.

`fileName` MUST be the last filter in its placeholder. A placeholder MUST NOT
use `fileName` more than once. A violation MUST produce a `CTC2008` template
diagnostic.

A placeholder with `fileName` is a plain capture, not a derived use. It
captures the source identifier. Later uses of the same name compare with that
identifier, as described in section 23. The placeholder can be the only plain
capture for derived uses such as `{{ Name | prefix("create") }}`.

The tool MUST compute the expected name as follows:

1. Take the final component of the source path.
2. Remove the last extension. So `user.service.ts` gives `user.service`, and
   `user-service.test.ts` gives `user-service.test`.
3. For `asIs`, use this stem without change.
4. Otherwise, split the stem into words. Each character that is not a letter
   or digit ends a word. A word also ends before an uppercase letter that
   follows a lowercase letter or digit. A word also ends before the last
   uppercase letter of an uppercase run when a lowercase letter follows that
   letter. Digits stay in the word before them.
5. For `PascalCase`, write each word with an uppercase first letter and
   lowercase other letters, and join the words.
6. For `camelCase`, do the same, but write the first word in lowercase.
7. For `snake_case`, write each word in lowercase, and join the words with `_`.

Examples:

| File | `PascalCase` | `camelCase` | `snake_case` | `asIs` |
|---|---|---|---|---|
| `widget.hpp` | `Widget` | `widget` | `widget` | `widget` |
| `user-service.ts` | `UserService` | `userService` | `user_service` | `user-service` |
| `user_service.hpp` | `UserService` | `userService` | `user_service` | `user_service` |
| `user.service.ts` | `UserService` | `userService` | `user_service` | `user.service` |
| `HTTPServer.ts` | `HttpServer` | `httpServer` | `http_server` | `HTTPServer` |
| `base64Encoder.ts` | `Base64Encoder` | `base64Encoder` | `base64_encoder` | `base64Encoder` |

The filters before `fileName` run from left to right on the captured
identifier. The result MUST equal the expected name. For example, with
`{{ Name | removePrefix("I") | fileName("PascalCase") }}`, the identifier
`ILogger` matches in `logger.ts`.

A mismatch MUST produce a `CTC3007` diagnostic. The message MUST name the
capture, the file name, and the expected name in the chosen case. The
`expected` value MUST be the identifier that would match: the tool reverses the
earlier filters on the expected name. When the filters cannot be reversed,
`expected` is the expected name. The `actual` value is the source identifier.

The file name comes from the path of the parsed source. This works the same for
configured rules and for ad hoc templates.

### 25.2 Pattern Filters

`matches` and `notMatches` check a captured identifier against a regular
expression:

```text
{{ Name | matches("^I[A-Z]") }}
{{ Name | notMatches("Impl$") }}
{{ Name | removePrefix("I") | matches("^[A-Z]") }}
```

Each filter requires one JSON string that is a valid regular expression in the
syntax of the Rust `regex` crate. An invalid expression MUST produce a `CTC2008`
template diagnostic. The expression is not anchored, so `matches("Service")`
matches `UserServiceImpl`. Use `^` and `$` to anchor it.

They are name filters, so the rules for name filters in sections 25 and 26
apply: they need an identifier capture, cannot occur with node constraints, and
cannot be used on a sequence placeholder. They are check filters, like
`fileName`. A placeholder with a check filter is a plain capture, not a derived
use. The filters before a check filter run on the captured text first, and the
result is tested. A check filter MUST NOT be followed by `prefix`, `suffix`,
`removePrefix`, or `removeSuffix`. `fileName` MUST still be the last filter.

`matches` passes when the expression matches the text. `notMatches` passes when
it does not. A placeholder can use several pattern filters. A `removePrefix` or
`removeSuffix` that does not apply to the captured text makes the filter fail.

A failed pattern filter MUST produce a `CTC3010` diagnostic with exit class
`1`. The diagnostic carries the pattern in `expected` (`match <pattern>` or
`not match <pattern>`) and the tested text in `actual`.

## 26. Node Constraints

The MVP supports:

- `kind`
- `field`

Name filters and node constraints MUST NOT occur in the same placeholder.

### 26.1 `kind`

`kind` requires one or more JSON strings.

Each string names one canonical kind from the selected language adapter.

The constraint passes when the node kind equals one listed kind.

An unknown kind MUST produce a template diagnostic.

### 26.2 `field`

`field` uses one of these forms:

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

The field name MUST exist in the selected language adapter field registry.

`equal` passes when the field exists and equals the given JSON scalar.

`notEqual` passes when the field is missing or has a different value.

`exists` passes when the field exists.

`notExists` passes when the field is missing.

An ordering operator (`lessThan`, `lessThanOrEqual`, `greaterThan`, or
`greaterThanOrEqual`) requires a JSON number. It passes when the field exists,
is a number, and compares as named with the given number. A field that is
missing or not a number fails.

`matches` and `notMatches` require a JSON string that is a valid regular
expression in the syntax of the Rust `regex` crate. An invalid expression, or an
argument that is not a string, MUST produce a template diagnostic. They test the
text of the field: a string as it is, and a number or Boolean as its JSON text.
The expression is not anchored, so `"eval"` also matches `evaluate`. `matches`
passes when the field exists and its text matches. `notMatches` passes when the
field is missing or its text does not match. A failed test reports the same
diagnostic as the other field operators.

Every node has the field `lineCount`. It is the number of source lines that
the node spans, counted from the line of its first character to the line of its
last character. It is computed from the node range and does not need to be in
the field registry of a language adapter. Template nodes do not carry it, so it
only constrains a placeholder. For example, this reports functions of more than
60 lines when used in forbid mode with the `descendants` scope:

```text
{{ Long | kind("FunctionDeclaration") | field("lineCount", "greaterThan", 60) }}
```

An invalid operator or argument count MUST produce a template diagnostic.

Call nodes have the field `callee`. It is the callee text with all white space
and comments removed, such as `eval`, `window.eval`, or `std::system`. The
TypeScript adapter sets it on `CallExpression` and `NewExpression` nodes, and
the C++ and Lua adapters set it on `CallExpression` nodes. Like the member fields, it
belongs to source nodes only. Template literals do not carry it, so a template
such as `eval({{* Args }});` still matches by structure. Use it on a placeholder:

```text
{{ Call | kind("CallExpression") | field("callee", "equal", "eval") }}
```

Import-like nodes have the field `module`. It is the module name that the node
names, as a string:

- TypeScript: on `import_statement` nodes (including `import x = require("y")`)
  and on `export ... from "y"` nodes, the decoded string of the source. On a call
  `require("y")` or a dynamic `import("y")` whose first argument is a plain
  string, the decoded text of that string. A computed argument gives no field.
- C++: on `#include` nodes, the included path without angle brackets or quotes.
- Rust: on `use` declarations, the path text with white space removed.
- Lua: on a `require` call whose first argument is a string literal, the
  decoded text of that string.

Like `callee`, `module` belongs to source nodes only. Template literals do not
carry it, so a template such as `import {{ B }} from "jquery";` still matches by
structure.

## 27. Template Compilation

The compiler MUST use collision-safe sentinels.

A generated sentinel prefix MUST NOT occur in the original template.

The compiler MUST record:

- Each original placeholder range.
- Each generated sentinel range.
- The mapping between the two ranges.

The language adapter MUST parse each generated template candidate once.

The C++ adapter CAN retry with declaration-form sentinels for isolated
placeholders when the first candidate does not parse.

The adapter MUST reject a generated tree with an error node.

The adapter MUST reject a generated tree with a missing node.

The compiler MUST locate each sentinel by its generated range.

The compiler MUST infer the sentinel syntax slot from its parent.

An unsupported slot MUST produce a template diagnostic.

Template diagnostics MUST use original template ranges.

The compiler MUST NOT execute template content.

## 28. Canonical Node Model

The matcher uses this logical model:

```rust
pub struct CanonicalNode {
    pub kind: Arc<str>,
    pub value: Option<CanonicalScalar>,
    pub fields: BTreeMap<Arc<str>, CanonicalScalar>,
    pub children: Vec<CanonicalNode>,
    pub range: SourceRange,
}
```

Canonical equality compares:

- Node kind.
- Scalar value.
- Canonical fields.
- Child count.
- Child order.
- Child values.

Canonical equality ignores:

- Source range.
- Raw source text.
- Comments.
- Parser bookkeeping.
- Byte-order marks.
- Line endings.
- Optional semicolons.
- Trailing commas.

The adapter MUST keep operators and required punctuation.

The adapter MUST keep decorator order.

The adapter CAN store each modifier as a boolean canonical field.

Only a node with no syntax children has a scalar value. A node whose only
children are dropped punctuation, such as an empty `{}` body, has no scalar
value and no children.

The adapter MUST keep every explicit parenthesized-expression node.

## 29. TypeScript Adapter

The TypeScript adapter MUST use the pinned `tree-sitter-typescript` grammar.

The adapter MUST expose stable canonical kind names. These names are `ctc`
names, not raw Tree-sitter names.

The MVP kind registry MUST include:

- `SourceFile`
- `ImportDeclaration`
- `ImportEqualsDeclaration`
- `InterfaceDeclaration`
- `ClassDeclaration`
- `FunctionDeclaration`
- `VariableStatement`
- `Identifier`

The adapter CAN expose additional documented kinds.

The adapter MUST expose the canonical boolean field `typeOnly` for:

- `ImportDeclaration`
- `ImportEqualsDeclaration`

`typeOnly` is `true` for a declaration that starts with `import type`.

`typeOnly` is `false` for a normal import.

`typeOnly` is `false` for a side-effect import.

`typeOnly` is `false` for `import { type Name }`.

`typeOnly` is `false` for a mixed type and value import.

The adapter MUST also expose `typeOnly` for:

- `InterfaceDeclaration`, where it is always `true`.
- `TypeAliasDeclaration`, where it is always `true`.
- `ExportStatement`, where it is `true` for `export interface`,
  `export type Name = ...`, and `export type { ... }`. It is `false` for all
  other exports.

The adapter MUST expose these fields on each direct member of a `ClassBody`:

- `access`: the text of the accessibility modifier. Without a modifier, it is
  `"private"` for a `#name` member and `"public"` otherwise.
- `static`: `true` for a `static` member or a static block.
- `constructor`: `true` for a non-static method named `constructor`.

A decorator written before a class member MUST become the first child of that
member, in source order.

Template literal nodes MUST NOT carry `access`, `static`, or `constructor`,
because those values follow from the written syntax.

The adapter MUST validate TypeScript identifiers after prefix and suffix
filters.

The adapter MUST report all Tree-sitter error and missing nodes as source parse
errors.

### 29.1 Conservative Semantic Facts

The TypeScript adapter MUST extract semantic facts during the same parse that
creates canonical syntax nodes.

Function facts MUST include:

- The function name.
- Declared return-type identifiers.
- Whether all conservative control-flow paths return a value.
- Bare return ranges.

Exception facts MUST include:

- `try` statements.
- `throw` statements.
- `Promise.reject(...)`.
- Calls to a Promise executor rejection callback.

Call facts MUST include compact dotted callee text for configured
exception-source matching.

The analyzer MUST NOT claim TypeScript type inference or imported-symbol
resolution.

### 29.2 C++ Adapter

The C++ adapter MUST use the pinned `tree-sitter-cpp` grammar.

The adapter MUST expose stable canonical kind names. These names are `ctc`
names, not raw Tree-sitter names.

The kind registry MUST include:

- `SourceFile`
- `FunctionDeclaration`
- `ArrowFunction`
- `ClassDeclaration`
- `StructDeclaration`
- `UnionDeclaration`
- `EnumDeclaration`
- `NamespaceDeclaration`
- `ClassBody`
- `StatementBlock`
- `IncludeDirective`
- `Identifier`
- `StringLiteral`
- `CharacterLiteral`
- `NumericLiteral`

The adapter MUST validate C++ identifiers after prefix and suffix filters.

The adapter MUST drop access labels such as `public:` from class bodies. It
MUST NOT drop access specifiers in base-class lists.

The adapter MUST expose these fields on each member of a `ClassBody`, including
members inside preprocessor blocks in the body:

- `access`: the access of the last label before the member. Without a label, it
  is `"private"` in a `class` and `"public"` in a `struct` or `union`.
- `constructor`: `true` when the member declares a function whose name equals
  the class name. A template declaration counts by its inner declaration.
- `static`: `true` when the member has the `static` storage class.
- `destructor`: `true` when the member declares a destructor such as
  `~Widget()`. A destructor MUST NOT count as a constructor.
- `virtual`: `true` when a member function has the `virtual` specifier. This
  includes pure virtual functions.
- `pure`: `true` when a member function ends with `= 0`.
- `override`: `true` when a member function has the `override` specifier.
- `final`: `true` when a member function has the `final` specifier.
- `operator`: `true` when the member declares an operator function. This
  includes conversion operators such as `operator bool()` and copy or move
  assignment operators.
- `friend`: `true` for a `friend` declaration.
- `deleted`: `true` when a member function ends with `= delete`.
- `defaulted`: `true` when a member function ends with `= default`.
- `const`: `true` when a member function has a `const` qualifier after its
  parameter list. A `const` return type does not count.

All fields except `access` MUST be present on each member, with the value
`false` when they do not apply. They follow the written syntax only; for example,
`override` without `virtual` leaves `virtual` as `false`. A data member with a
function pointer type is not a member function. A template declaration counts by
its inner declaration. A `friend` declaration has only `friend` set to `true`;
the function fields describe the class's own member functions.

The TypeScript adapter MUST NOT expose the C++-only fields `destructor`,
`virtual`, `pure`, `override`, `final`, `operator`, `friend`, `deleted`,
`defaulted`, and `const`.

In a template, an access label applies to the members and placeholders after
it. A literal member after a label MUST require that access. A placeholder after
a label MUST also require that access, unless it uses name filters or already
has an `access` field constraint. Members and placeholders before the first
label have no access requirement.

Because each source member carries its real access, one every template with
`kinds` set to `ClassDeclaration` and `StructDeclaration` can check classes and
structs.

Template literal nodes MUST NOT carry `constructor`, `static`, or the C++-only
member fields above, because those values follow from the written syntax.

The adapter MUST expose member function facts for `headerSourcePairing`:

- A declaration fact for each member function declared in the body of a named,
  non-template class, struct, or union, including nested classes and members
  inside preprocessor blocks. Declarations with an inline body, `= default`,
  `= delete`, or `= 0`, declarations marked `inline`, `constexpr`, or
  `consteval`, member templates, friend declarations, and conversion operators
  MUST NOT produce a fact.
- A definition fact for each function definition outside a class whose name is
  qualified by one or more namespace or class names, such as
  `Widget::value`. Template definitions and names qualified by a template type
  MUST NOT produce a fact.

Each fact has an owner, a name, and a signature. The owner is the list of
enclosing named namespaces, then the class names from the qualified name or
the class nesting. An anonymous namespace adds no owner segment.

The signature is the list of parameter types, followed by any trailing
`const`, `volatile`, `&`, or `&&` qualifiers. A parameter type drops the
parameter name, the default value, and a top-level `const` or `volatile`. A
single `void` parameter means no parameters. Token text is joined with single
spaces, and a space next to punctuation is removed.

The adapter MUST report all Tree-sitter error and missing nodes as source parse
errors.

The adapter MUST NOT claim compiler-accurate parsing, type analysis, include
resolution, or macro expansion.

### 29.3 Rust Adapter

The Rust adapter MUST use the pinned `tree-sitter-rust` grammar. It selects
files with the suffix `.rs` and templates with the suffix `.rs.ctmpl`.

The adapter MUST NOT claim type analysis, name resolution, or macro expansion.
Tokens inside a macro invocation are opaque, so a template cannot look inside
`println!(...)` arguments.

The kind registry uses the PascalCase form of the Tree-sitter node kind, with
these fixed names so that the shared scopes work:

| Canonical kind | Tree-sitter node kinds |
|---|---|
| `SourceFile` | `source_file` |
| `Identifier` | `identifier`, `type_identifier`, `field_identifier` |
| `StringLiteral` | `string_literal`, `raw_string_literal` (one node with a value) |
| `NumericLiteral` | `integer_literal`, `float_literal` |
| `CharacterLiteral` | `char_literal` |
| `FunctionDeclaration` | `function_item` (functions and methods) |
| `ArrowFunction` | `closure_expression` |
| `StatementBlock` | `block` |
| `ClassBody` | `declaration_list`, `field_declaration_list`, `enum_variant_list` |
| `StructDeclaration` | `struct_item` |
| `EnumDeclaration` | `enum_item` |
| `TraitDeclaration` | `trait_item` |
| `ImplBlock` | `impl_item` |
| `ModuleDeclaration` | `mod_item` |
| `UseDeclaration` | `use_declaration` |

Other named node kinds keep their PascalCase name, such as `MacroInvocation`,
`AttributeItem`, or `CallExpression`. A function body is a `StatementBlock`
child of the function, so the `functionBody` scope works. Impl blocks, trait
bodies, module bodies, struct bodies, and enum bodies are `ClassBody` nodes, so
the `classBody` scope and `every` class templates work. A tuple-struct field
list is not normalized to `ClassBody`.

The adapter sets these fields on source nodes only. Template literals do not
carry them:

- `callee`: on `call_expression`, the callee text without white space, such as
  `Vec::new`. For a method call it is the method name, such as `unwrap`. On
  `macro_invocation` it is the macro path followed by `!`, such as `panic!`.
- `attribute`: on `attribute_item` and `inner_attribute_item`, the attribute
  path, such as `allow`, `derive`, or `clippy::foo`.
- `public`: `true` on an item or field with a plain `pub` modifier.
- `visibility`: the visibility text, such as `pub(crate)`, on items and fields
  that have one.
- `async`, `unsafe`, `const`: Booleans on `function_item`.
- `mutable`: on `let` declarations and parameters.

The adapter MUST expose `line_comment` and `block_comment` ranges as comments
for suppression comments. Doc comments count as comments.

The scanner MUST treat these regions as protected: line comments, block
comments (which nest), string literals, byte strings, raw strings with any
number of `#`, and character and byte literals. A lifetime such as `'a` is not
a character literal.

The adapter supports sequence placeholders in source-file items, block
statements, function parameters, call arguments, struct fields, enum variants,
impl, trait, and module items, use-list items, generic parameters and arguments,
match arms, and item attributes. The optional keywords are `async`, `const`,
`default`, `extern`, `mut`, `pub`, and `unsafe`. Two adjacent optional keywords
in one signature are not supported.

The adapter has no semantic facts. `companionFile` and `fileLength` work on
`.rs` files. The other semantic kinds do not select them.

### 29.4 Lua Adapter

The Lua adapter MUST use the pinned `tree-sitter-lua` grammar. It selects files
with the suffix `.lua` and templates with the suffix `.lua.ctmpl`. The adapter
reads Lua 5.5 syntax, including `global` declarations, attributes before a name
list, and named vararg parameters. `global` is a name outside a declaration, as
in a Lua 5.5 build with `LUA_COMPAT_GLOBAL`.

The adapter MUST report all Tree-sitter error and missing nodes as source parse
errors. It MUST also report these forms as source parse errors, because the
grammar accepts them and Lua does not:

- A name with a character that is not an ASCII letter, digit, or `_`.
- A number with a suffix such as `LL` or `i`, or a binary number.
- A line break without a backslash in a quoted string.
- A decimal escape above 255 or a `\u{...}` escape above `7FFFFFFF`.

Before it parses a file, the adapter MUST replace a carriage return that
follows a backslash in a quoted string with `z`. The grammar rejects this valid
form. The replacement keeps the byte length, so node ranges and node text
come from the original source.

The adapter MUST NOT claim name resolution or the checks that `luac` does
after it parses, such as assignments to `<const>` variables or to the control
variable of a `for` loop.

The kind registry uses the PascalCase form of the Tree-sitter node kind, with
these fixed names:

| Canonical kind | Tree-sitter node kinds |
|---|---|
| `SourceFile` | `chunk` |
| `StatementBlock` | `block` |
| `FunctionDeclaration` | `function_declaration` (all `function` statements) |
| `FunctionExpression` | `function_definition` |
| `CallExpression` | `function_call` |
| `Identifier` | `identifier`, and the word `global` outside a declaration |
| `StringLiteral` | `string` (one node with a value) |
| `NumericLiteral` | `number` |

Supertype kinds such as `statement` and `expression` are not in the registry.

The canonical tree differs from the Tree-sitter tree in these ways:

- A body that the grammar leaves out because it is empty, such as in
  `function f() end`, is an empty `StatementBlock`. This applies to functions
  and to the bodies of `do`, `while`, `repeat`, `for`, `if`, `elseif`, and
  `else`.
- The `assignment_statement` inside `local x = 1` or `global x = 1` is not a
  node. Its children are children of the `VariableDeclaration`.
- `empty_statement` nodes, comments, and the tokens `(`, `)`, `[`, `]`, `{`,
  `}`, `,`, and `;` are dropped.
- A string literal has its decoded text as its value. A string that does not
  decode to UTF-8 keeps its source text.

The adapter sets these fields on source nodes only. Template literals do not
carry them:

- `callee`: on `function_call`, the called text without white space or
  comments, such as `string.format` or `self:emit`. A string literal in the
  callee keeps its text.
- `module`: see section 26.2.
- `local`, `global`: Booleans on `variable_declaration`,
  `implicit_variable_declaration`, and `function_declaration`. Each is `true`
  when the declaration starts with that keyword.

The adapter MUST expose `comment` ranges as comments for suppression comments.

The adapter supports sequence placeholders in source-file statements, block
statements, function parameters, call arguments, table constructor items,
expression lists, and name lists. A placeholder alone on a line in a statement
list is one statement. The optional keywords are `local` and `global`.

The adapter extracts global facts (section 29.5). `companionFile` and
`fileLength` work on `.lua` files. `accidentalGlobals` and `restrictedGlobals`
select only `.lua` files. The other semantic kinds do not select them.

### 29.5 Lua Global Facts

The Lua adapter MUST extract global facts during the same parse that creates
canonical syntax nodes. A global fact describes one use of a name that no
declaration binds.

The adapter MUST follow the scoping rules of Lua. These constructs bind a name:

- `local` variables and `local function`. The name of `local function` is bound
  in the function body. A name in `local x = x` is bound after the statement.
- Function parameters, `self` in a method declared with `:`, and the name after
  `...`.
- The variables of numeric and generic `for` statements. They are bound in the
  body.
- `global` declarations of Lua 5.5, including `global function`. A name that a
  declaration binds is not a global fact.
- `global *` and `global<const> *`. They bind every name that no other
  declaration binds, in the block of the declaration.

A binding starts after the declaration and ends with the block. The condition
of `repeat ... until` is in the scope of the body. A local named `_ENV`
replaces the global table. The adapter MUST NOT produce global facts in the
scope of a local `_ENV`.

A name that no binding covers is a global variable. The adapter MUST produce one
fact for each use. A fact has these parts:

- The kind: read, write, dynamic read, or dynamic write.
- The name, such as `os`.
- The path: the name followed by the string keys that follow it, such as
  `os.time`. A key is a string key in `a.b` and in `a["b"]`. The path ends at
  the first key that is not a string literal, and the fact records that the key
  is dynamic.
- Whether the code reached the name with `_G` or `_ENV`.
- The call shape: not called, called with one string literal argument, or
  called with other arguments. This applies when the path is the callee of a
  call.
- The source range, from the start of the name to the end of the last key of
  the path.

A read fact applies to the use of a value, also when code reaches a field below
the name. An assignment to a plain name, and `function name() end`, are write
facts. An assignment to a field, such as `package.path = "x"`, is a read fact
with the path `package.path`.

`_G.name` and `_ENV.name` produce facts for the global `name`, with the table
flag set. Other uses of `_G` and `_ENV` are facts for the names `_G` and
`_ENV`. `_G[key]` with a key that is not a string literal produces a dynamic
read, or a dynamic write when it is the target of an assignment. A local named
`_G` is an ordinary variable.

The adapter MUST NOT claim type inference or the resolution of names that
come from `require`.

## 30. Literal Matching

A literal template node MUST equal the corresponding canonical source node.

Each canonical field on a literal template node MUST equal the same source
field. A source field that the template node does not carry does not affect the
match.

A kind mismatch fails at the source node.

A scalar mismatch fails at the source node.

A missing source child fails at the closest existing source ancestor.

An additional source child fails at that additional child.

## 31. Sequence Matching

Sequence matching MUST proceed from left to right.

The first attempt MUST use the shortest sequence capture.

The matcher CAN backtrack when a later fixed node does not match.

The matcher MUST memoize sequence states.

The memoization key MUST include:

- Template list position.
- Source list position.
- Capture-environment fingerprint.

One rule and source pair has a limit of 100,000 sequence states.

Reaching the limit MUST produce a template-complexity diagnostic.

## 32. Exact Mode

Exact mode compares the complete template statement list with the complete
source statement list.

Every source statement MUST have a matching template node.

Additional statements require a sequence placeholder that accepts them.

One mismatch makes the rule fail.

The rule reports one primary mismatch.

## 33. Contains Mode

Contains mode searches each start position in the top-level source statement
list.

The matcher MUST examine candidate starts in source order.

Statements before and after the candidate region do not affect the match.

The matcher MUST NOT skip statements inside the candidate region.

A sequence placeholder can consume statements inside the candidate region.

The selected search scope controls whether contains mode searches nested lists.

The first successful candidate makes the rule pass.

Each candidate MUST use a new capture environment.

A contains template MUST consume at least one source statement.

### 33.1 Match Counts

A contains rule with `count` counts the successful candidates in each source
file. The matcher MUST examine every candidate and MUST NOT stop at the first
match. Each candidate start that matches is one match.

The rule passes when the number of matches is at least `min` and, when `max`
is set, at most `max`.

When there are fewer matches than `min`, the tool MUST produce one `CTC3008`
diagnostic at the start of the file. When there are more matches than `max`,
the tool MUST produce one `CTC3009` diagnostic for each match after the first
`max` matches, at the position of that match. Both diagnostics carry the
expected and the found count.

A contains rule without `count` behaves as a rule with `min` of `1` and no
`max`.

## 34. Forbid Mode

Forbid mode uses the same candidate starts as contains mode.

The rule passes when no candidate matches.

The rule fails when one or more candidates match.

Each matching candidate MUST produce one diagnostic.

Each candidate MUST use a new capture environment.

Diagnostics MUST use the first source node of each matched candidate.

A forbid template MUST consume at least one source statement.

A contains or forbid template that can match an empty statement list is
invalid.

### 34.1 Search Scopes

`topLevel` searches direct source-file statements.

`descendants` searches every syntax list in the source tree.

`functionBody` searches syntax inside function, method, generator, and
arrow-function block bodies.

`classBody` searches syntax inside class bodies, including method bodies.

A scope object has the form `{ "inside": [<kind>...], "stopAtFunctions": <bool> }`.
It searches every node whose canonical kind is listed in `inside`, and
everything below that node. `inside` MUST contain at least one kind, and every
kind MUST be known to the language adapter of the template. A rule that breaks
these limits MUST produce an invalid-configuration diagnostic. `stopAtFunctions`
is optional and defaults to `true`. When it is `true`, the search does not enter
function, method, generator, and arrow-function nodes below the listed node.
The function nodes themselves stay in the lists of their parents. A listed
node that is itself a function is still searched, but the functions nested in
it are skipped. A node inside two listed nodes is searched once. The string
`inside` is not a scope name.

Nested scope regions MUST NOT produce duplicate diagnostics for one syntax
node.

Candidate lists MUST use deterministic source order.

Exact mode MUST reject a scope other than `topLevel`.

### 34.2 Return Paths

`returnPaths` selects functions whose declared return annotation
contains `typeName`.

Every selected function MUST return a value on all conservative control-flow
paths.

A bare return MUST produce a separate diagnostic.

Expression type compatibility remains the responsibility of the TypeScript
compiler.

Loops and unsupported control-flow constructs MUST be treated conservatively as
possible fallthrough.

### 34.3 Exception Policy

`exceptionPolicy` reports enabled explicit exception constructs.

`forbidPromiseReject` MUST report both `Promise.reject(...)` and calls to a
Promise executor rejection callback.

`exceptionSources` uses case-sensitive glob patterns against compact callee
text.

Configured call patterns MUST NOT be described as type-resolved symbols.

### 34.4 Every Mode

An every template MUST contain exactly one top-level node, and that node MUST
be a literal node. Any other shape MUST produce an invalid-every-template
diagnostic.

Every mode uses the candidate lists of the selected search scope.

Without `kinds`, the candidates are the nodes with the same kind as the
template node. With `kinds`, the candidates are the nodes whose kind is in the
list. The template node's own kind is a candidate kind only when it is in the
list.

Each candidate MUST match the template node, with these rules:

- The candidate's kind is not compared with the template node's kind. Its value
  and fields MUST match.
- When the last child of the template node is a literal `ClassBody` node, the
  template is a class template. A candidate without a `ClassBody` child is not
  checked. The candidate's last `ClassBody` child MUST match the template's
  class body in full. The children before the body form the header. `Token`
  nodes in the template header and in the candidate header are ignored. Each
  remaining template header item MUST match a candidate header node, in order.
  The candidate header can contain other nodes between and after them. An
  optional template header item can match no node. Children after the body are
  ignored. The header and the body share one capture environment, and the
  matcher MUST try other header choices before it reports a failure.
- For any other template, the candidate's children MUST match the template
  node's children as in exact matching.

Each candidate MUST use a new capture environment.

The rule passes when every candidate matches, including when there is no
candidate.

Each candidate that does not match MUST produce its own diagnostic.

Diagnostics MUST follow source order.

### 34.5 Companion Files

`companionFile` checks each selected file without parsing it. When no
`companions` candidate names a partner file, the rule MUST report `CTC4201` at
the start of the selected file. The message MUST list every candidate path.

### 34.6 Header and Source Pairing

`headerSourcePairing` parses the selected header and reads its member function
facts (section 29.2). A header declaration is required unless a definition
fact in the same header matches it.

When no `sources` candidate names a partner file:

- With `missingSource` set to `"skip"`, the rule MUST report nothing.
- With `missingSource` set to `"report"`, the rule MUST report `CTC4201` at the
  start of the header when the header has at least one required declaration.

A partner file that does not use the header's language adapter MUST produce
`CTC1008`. The partner MUST be parsed at most once per run. A parse error in the
partner MUST be reported once, and the rule then reports nothing else for that
header.

A definition matches a declaration when the names and signatures are equal and
the owners are equal. Definitions with equal owners are paired first. Then a
definition whose owner is a trailing part of the declaration owner can match a
declaration that is still unpaired. This allows definitions after
`using namespace`. Each definition matches at most one declaration.

Each required declaration without a matching definition in the partner MUST
produce `CTC4202` at the declaration. When the partner defines a function with
the same owner and name but another signature, the message MUST name the first
such definition and its location.

When `checkOrder` is `true`, the matched definitions MUST follow header
declaration order. The rule keeps the longest group of matched definitions
whose source order already follows header order. Every other matched
definition MUST produce `CTC4203` at the definition. The message MUST name the
header declaration and the neighbor it should follow or precede. The expected
and actual values MUST give the header position and the source position among
the matched definitions.

### 34.7 Lua Global Rules

The rules of this section read the global facts of section 29.5.

`accidentalGlobals` MUST report:

- `CTC4401` for each read fact whose name is not in `allow`.
- `CTC4402` for each write fact whose name is not in `allowWrite`, and for each
  dynamic write fact.

A dynamic read fact produces no diagnostic from this rule. Each diagnostic is
attached to the source range of the fact. The message names the global, with
`_G.` before the name when the code reached it with `_G` or `_ENV`.

`restrictedGlobals` MUST test each read fact and write fact against the entries
of `forbid` in the configured order. The first entry that applies decides. An
entry applies when:

- the path of the fact is the entry or is below it (the path starts with the
  entry and a dot), or
- the entry is below the path, and the code uses the path as a value (the path
  is not the callee of a call) or the next key is not a string literal. Then
  the value can reach the entry.

An entry that applies MUST produce `CTC4403`. When the configuration has no
`forbid`, the default list is: `load`, `loadfile`, `loadstring`, `dofile`,
`setfenv`, `collectgarbage`, `string.dump`, `debug`, `io`, `os`, and `package`.
An empty list turns the name check off.

When `forbidDynamicRequire` is `true` and no entry applied, the rule MUST
produce `CTC4404` for each read fact with the path `require` whose call shape
is not one string literal. This includes `require` as a value.

The rule MUST produce `CTC4405` for each dynamic read fact and dynamic write
fact, also when `forbidDynamicRequire` is `false` and `forbid` is empty.

All diagnostics have exit class `1`. Both rules honor suppression comments
(section 38.1) when the rule sets `allowIgnore` to `true`.

## 35. Failure Selection

Exact and failed contains rules report one primary mismatch.

The matcher MUST rank candidate failures by:

1. The number of matched literal and capture nodes.
2. The greatest source byte offset reached.
3. The earliest candidate start offset.

The matcher MUST use the first unequal rank.

Sequence backtracking MUST NOT change the selected failure for the same inputs.

## 36. Type-Only Import Rule

This template forbids value imports:

```ts
// ctc: mode=forbid
{{ Import | kind("ImportDeclaration", "ImportEqualsDeclaration") | field("typeOnly", "notEqual", true) }}
```

The rule permits:

```ts
import type { ILogger } from "./interfaces";
```

The rule rejects:

```ts
import { ILogger } from "./interfaces";
import "./register";
import Logger = require("./logger");
```

The rule also rejects:

```ts
import { type ILogger } from "./interfaces";
```

The MVP rule does not inspect:

- `require()` call expressions.
- Dynamic `import()` expressions.
- Re-export declarations.

Use separate templates with `descendants` for rules that target these nested
constructs.

## 37. `service-adapter` Rule Layout

The reference layout is:

```text
service-adapter/
  .ctc.json
  .ctmpl/
    no-value-imports.ts.ctmpl
    class-factory.ts.ctmpl
```

The project configuration excludes `src/index.ts` from the no-value-import
rule. It excludes `src/index.ts` and `src/interfaces.ts` from the
class-factory rule.

## 38. Source Parsing

The tool MUST parse each selected source file once per command.

All rules for that source MUST reuse the parsed canonical tree.

A source parse error makes the command fail with exit code `1`.

The matcher MUST NOT process a source tree that contains parser errors.

### 38.1 Suppression Comments

A source comment can skip diagnostics of named rules. The same syntax applies
to TypeScript, C++, and Rust sources. Lua sources use Lua comments with the
same text, such as `-- ctc-ignore-next-line <rule-id>` or
`--[[ ctc-ignore-file <rule-id> ]]`:

```text
// ctc-ignore-next-line <rule-id>[, <rule-id>]... [-- <reason>]
// ctc-ignore-file <rule-id>[, <rule-id>]... [-- <reason>]
/* ctc-ignore-next-line <rule-id>[, <rule-id>]... [-- <reason>] */
/* ctc-ignore-file <rule-id>[, <rule-id>]... [-- <reason>] */
```

The tool MUST read suppression comments only from comment nodes in the raw
parser tree. Text inside string literals, template literals, or other
non-comment tokens MUST NOT act as a suppression.

To read a comment, the tool removes the comment delimiters, extra leading `/`
characters of a line comment, leading `*` characters of a block comment, extra
leading `-` characters of a Lua line comment, and surrounding white space. The comment is a suppression candidate when the first
word of the remaining text starts with `ctc-ignore`.

The first word MUST be `ctc-ignore-next-line` or `ctc-ignore-file`.

The optional reason starts at the first `--` that has white space or the start
of the text before it and white space or the end of the text after it. The
tool ignores the reason.

The text before the reason is a comma-separated list of rule identifiers.
White space around each identifier is ignored. The list MUST contain at least
one identifier. Each identifier MUST name a rule in `rules` or `semanticRules`
of the loaded configuration. A rule that the command line did not select with
`--rule` is still a valid name.

`ctc-ignore-next-line` targets the next code line. The next code line is the
line of the first character after the comment that is not white space and is
not inside another comment. When no such character exists, the comment
targets nothing.

`ctc-ignore-file` targets the whole file. It CAN appear anywhere in the file.

A diagnostic is suppressed when all of these are true:

- It has a rule identifier and a source range.
- Its source path is the file that contains the comment.
- The comment names its rule identifier.
- The comment is `ctc-ignore-file`, or the diagnostic source starts on the
  target line of a `ctc-ignore-next-line` comment.
- The rule sets `allowIgnore` to `true`.

A rule allows suppression comments only when it sets `allowIgnore` to `true`.
A comment that names a rule that does not allow suppression MUST NOT suppress
any diagnostic of that rule, and MUST produce one `CTC5002` diagnostic at the
comment for each such rule identifier. A comment can name allowed and
protected rules at the same time. The allowed rules are suppressed as usual.
A `CTC5002` diagnostic cannot be suppressed.

A suppressed diagnostic MUST NOT appear in human or JSON output and MUST NOT
affect `matches` or the exit code. Diagnostics without a rule identifier, such
as source parse errors, cannot be suppressed.

An invalid suppression comment MUST produce a `CTC5001` diagnostic with the
`invalid-suppression` category and the comment as its source range. It has no
rule identifier and uses exit class `1`. These comments are invalid:

- The first word starts with `ctc-ignore` but is not a known directive.
- The rule list is empty.
- The rule list contains an empty entry.
- The rule list names a rule that the configuration does not define.

Valid names in a comment still apply when another name in the same comment is
invalid.

A suppression that skips no diagnostic MUST NOT produce a diagnostic, because
the result depends on the rules and files that a command selects.

Ad hoc template checks with `--template` MUST NOT read suppression comments.

## 39. Template Caching

The tool MUST compile one selected template once per command.

All selected source files MUST reuse that compiled template.

Cache behavior MUST NOT change diagnostics or diagnostic order.

Persistent caching between commands is outside the MVP.

## 40. Diagnostic Positions

A diagnostic position contains:

- A zero-based UTF-8 byte offset.
- A one-based line number.
- A one-based Unicode scalar column.

An end position is exclusive.

Source and template paths MUST use `/` separators.

Paths MUST be relative to the scan root when they are inside that root.

A path outside the scan root MUST use its normalized absolute form.

## 41. Diagnostic Codes

The MVP defines these codes:

| Code | Category |
|---|---|
| `CTC0001` | Invalid command line |
| `CTC1001` | Invalid template filename |
| `CTC1003` | Configured rule selects no files |
| `CTC1005` | Invalid template directive |
| `CTC1006` | No selected source files |
| `CTC1007` | No applicable rule |
| `CTC1008` | Unsupported source type |
| `CTC1009` | Invalid UTF-8 |
| `CTC1010` | Invalid configuration |
| `CTC2001` | Invalid template syntax |
| `CTC2002` | Unsupported placeholder context |
| `CTC2003` | Invalid node kind |
| `CTC2004` | Invalid canonical field |
| `CTC2005` | Derived source has no capture |
| `CTC2006` | Placeholder category mismatch |
| `CTC2007` | Sequence placeholder needs a constraint |
| `CTC2008` | Invalid filter |
| `CTC2009` | Template complexity limit |
| `CTC2010` | Empty search template |
| `CTC2011` | Invalid every template |
| `CTC3001` | Source parse error |
| `CTC3002` | Missing required syntax node |
| `CTC3003` | Unexpected syntax node |
| `CTC3004` | Captured value mismatch |
| `CTC3005` | Derived identifier mismatch |
| `CTC3006` | Forbidden structure matched |
| `CTC3007` | Identifier does not match the file name |
| `CTC3008` | Too few matches for the template |
| `CTC3009` | Too many matches for the template |
| `CTC3010` | Identifier does not match the pattern filter |
| `CTC4001` | Function can complete without returning a value |
| `CTC4002` | Function has a return without a value |
| `CTC4101` | Forbidden try statement |
| `CTC4102` | Forbidden throw statement |
| `CTC4103` | Promise rejection |
| `CTC4104` | Configured exception-source call |
| `CTC4401` | Read of a global variable that the rule does not allow |
| `CTC4402` | Assignment to a global variable that the rule does not allow |
| `CTC4403` | Use of a restricted global name |
| `CTC4404` | `require` without one string literal as its argument |
| `CTC4405` | `_G` or `_ENV` indexed with a key that is not a string literal |
| `CTC4201` | Missing partner file |
| `CTC4202` | Missing member function definition |
| `CTC4203` | Member function definition out of order |
| `CTC4301` | File has more lines than the rule allows |
| `CTC5001` | Invalid suppression comment |
| `CTC5002` | Suppression not allowed for this rule |
| `CTC5101` | Source file is in the watched area but no rule selects it |
| `CTC5201` | A suppression comment was added |
| `CTC5202` | A rule selects fewer files than at the base reference |
| `CTC5203` | A rule was removed or now allows suppression comments |
| `CTC5204` | A rule definition changed |
| `CTC9001` | Internal tool error |

Codes MUST keep the same meaning within specification version `0.x`.

## 42. Human Diagnostics

Human output MUST start with:

```text
<source-path>:<line>:<column> [<rule-id>] <message>
```

When a diagnostic has a rule message, `<message>` MUST be the rule message, and
the next line MUST contain the engine message:

```text
Detail: <engine-message>
```

The code, expected and actual values, and template range MUST still appear.
When a diagnostic has no rule message, `<message>` is the engine message and
the `Detail` line MUST NOT appear.

A diagnostic with a template range MUST include:

```text
Template: <template-path>:<line>:<column>
```

The human formatter CAN include expected and actual values.

## 43. JSON Diagnostics

JSON output MUST contain exactly one JSON object.

The object shape is:

```json
{
  "schemaVersion": 1,
  "matches": false,
  "diagnostics": [
    {
      "code": "CTC3006",
      "category": "forbidden-structure",
      "message": "Value imports are not permitted.",
      "ruleId": "no-value-imports",
      "source": {
        "path": "src/adapter.ts",
        "start": {
          "offset": 0,
          "line": 1,
          "column": 1
        },
        "end": {
          "offset": 39,
          "line": 1,
          "column": 40
        }
      },
      "template": {
        "path": ".ctmpl/no-value-imports.ts.ctmpl",
        "start": {
          "offset": 0,
          "line": 1,
          "column": 1
        },
        "end": {
          "offset": 105,
          "line": 1,
          "column": 106
        }
      }
    }
  ]
}
```

Optional fields MUST be omitted when they have no value.

`message` MUST always contain the engine message. When a diagnostic has a rule
message, the diagnostic MUST also contain a `ruleMessage` string field with
that text. For example:

```json
{
  "code": "CTC3006",
  "category": "forbidden-structure",
  "message": "The source contains a forbidden structure.",
  "ruleId": "no-try",
  "ruleMessage": "Return a Result value instead of catching exceptions."
}
```

The other fields, such as `source` and `template`, stay the same. This example
leaves them out to stay short.

Diagnostic category values MUST use kebab-case.

`matches` is `true` only when no diagnostic affects the command result.

## 44. Diagnostic Order

The tool MUST sort diagnostics by:

1. Source path.
2. Rule identifier.
3. Source start offset.
4. Diagnostic code.
5. Template path.
6. Template start offset.

Missing values sort before present values.

Parallel execution MUST NOT change this order.

## 45. Standard Output and Standard Error

Expected diagnostics MUST go to standard output.

Human and JSON formats MUST follow this rule.

An internal failure that cannot create a report CAN write to standard error.

JSON mode MUST NOT write logs or progress text to standard output.

## 46. Exit Codes

The MVP exit codes are:

| Code | Meaning |
|---|---|
| `0` | All selected rules match |
| `1` | A rule mismatch, source parse error, invalid or refused suppression comment, uncovered file, or `guard` finding occurred |
| `2` | A CLI, configuration, template, or internal error occurred |

The tool MUST use the highest applicable exit class.

Thus, an exit-code `2` error takes precedence over an exit-code `1` error.

## 47. CLI Commands

The MVP command forms are:

```text
ctc [paths...] [--root <path>] [--rule <id>]...
  [--config <path>] [--format human|json]
  [--color auto|always|never]

ctc <paths...> --template <path> [--mode exact|contains|forbid|every]
  [--scope top-level|descendants|function-body|class-body]
  [--message <text>]
  [--root <path>] [--format human|json] [--color auto|always|never]

ctc validate-template <path> [--format human|json]
  [--color auto|always|never]

ctc explain <path> --rule <id> [--root <path>] [--config <path>]
  [--format human|json] [--color auto|always|never]

ctc explain <path> --template <path> [--mode exact|contains|forbid|every]
  [--scope top-level|descendants|function-body|class-body]
  [--root <path>] [--format human|json] [--color auto|always|never]

ctc coverage [--root <path>] [--config <path>] [--format human|json]
  [--color auto|always|never]

ctc guard --base <git-ref> [--allow-rule-changes] [--root <path>]
  [--config <path>] [--format human|json] [--color auto|always|never]

ctc --version

ctc --help
```

`ctc check` CAN remain as a hidden compatibility alias during version `0.x`.

`--format` defaults to `human`.

`--color` defaults to `auto`.

`auto` MUST enable colors only when standard output is a terminal.

`auto` MUST disable colors when the `NO_COLOR` environment variable exists.

`always` MUST enable colors in human output.

`never` MUST disable colors.

JSON output MUST NOT contain ANSI color sequences.

`--template` requires one or more explicit source paths.

`--template` and `--rule` MUST NOT occur together.

`--template` and `--config` MUST NOT occur together.

`--mode` requires `--template`.

`--scope` requires `--template`.

`--message` requires `--template`.

`--message` sets the rule message of the ad hoc rule. It follows the same rules
as a configured `message`. An invalid `--message` value MUST produce an
invalid-command-line diagnostic.

`--mode` overrides the mode directive in an ad hoc template.

An ad hoc template filename MUST contain a supported source suffix before
`.ctmpl`.

All explicit sources in one ad hoc command MUST use the selected adapter.

### 47.1 `explain`

`explain` shows how the matcher read one source file against one template rule.

The command takes exactly one source file and exactly one of `--rule` or
`--template`. `--rule` and `--template` MUST NOT occur together.

`--rule` MUST name a configured template rule that selects the source file.
A semantic rule MUST fail with `CTC0001`. A rule that does not select the file
MUST fail with `CTC1007`.

`--template` follows the same rules as an ad hoc check. `--mode` and `--scope`
require `--template`. `--template` and `--config` MUST NOT occur together.

`explain` MUST use the same matcher as a normal check. The match result, the
diagnostics, and the exit code MUST be the same as a check of the same file and
rule.

The report has one section for each candidate:

| Mode | Candidates |
|---|---|
| `exact` | The whole file. |
| `every` | Each candidate node. |
| `contains` | The first match, or the best failed attempt when nothing matches. |
| `forbid` | Each forbidden match, or the best failed attempt when nothing matches. |

For each candidate, the report lists the steps of the final successful match,
or of the best failed attempt. The best attempt is the one the normal
diagnostic reports. Attempts that the matcher abandoned while backtracking are
not shown. Each step has a template range, a step kind, and the source nodes it
matched:

| Step kind | Meaning |
|---|---|
| `literal` | A literal template node matched one source node. |
| `capture` | A single placeholder captured one source node. |
| `sequence` | A sequence placeholder captured zero or more source nodes. |
| `derived` | A derived placeholder matched one identifier. |
| `optional-absent` | An optional template node matched nothing. |

A step also has a `depth`, which is its nesting level in the template.

Each source node has its kind, its range, a scalar value when it has one, and
a short text taken from the first line of its source.

A failed candidate also has the diagnostic of the best attempt. When a source
node could not join an adjacent sequence group, the failure SHOULD contain a
`reason`. The reason says either that the node passes an earlier group while a
later group already started, or which filter each group rejected.

Human output starts with the rule, template, source, and result. Each candidate
section then prints the steps, indented by depth, and a `Stopped at` block for
a failure.

JSON output has this shape:

```json
{
  "schemaVersion": 1,
  "ruleId": "member-order",
  "mode": "every",
  "scope": "descendants",
  "template": ".ctmpl/member-order.ts.ctmpl",
  "source": "src/service.ts",
  "matches": false,
  "candidates": [
    {
      "source": { "path": "src/service.ts", "start": {}, "end": {} },
      "text": "class PrivateFirst {",
      "templateMatched": false,
      "steps": [
        {
          "depth": 2,
          "kind": "sequence",
          "name": "PrivateMembers",
          "template": {},
          "nodes": [
            {
              "kind": "PublicFieldDefinition",
              "range": {},
              "text": "private count = 0"
            }
          ]
        }
      ],
      "failure": {
        "code": "CTC3003",
        "reason": "The node at line 19 (MethodDefinition) passes group `PublicMembers`, but the later group `PrivateMembers` already started at line 17."
      }
    }
  ],
  "diagnostics": []
}
```

The `failure` object contains all diagnostic fields plus `reason`. Ranges use
the same shape as diagnostic ranges.

Exit code `0` means the rule passes. Exit code `1` means the rule fails. In
`forbid` mode, the rule fails when the template matches. Exit code `2` means a
command, configuration, template, or internal error. Errors use the normal
diagnostic report format.

A normal check MUST NOT record explain steps.

### 47.2 `coverage`

```text
ctc coverage [--root <path>] [--config <path>] [--format human|json]
  [--color auto|always|never]
```

`coverage` lists source files that sit in the watched area and that no rule
selects. It needs no configuration beyond the `include` and `exclude` patterns
of the rules. It MUST NOT read or match templates beyond loading the
configuration.

The fixed directory of an include pattern is the sequence of path segments
before the first segment that contains a wildcard character (`*`, `?`, `[`,
`]`, `{`, or `}`). A pattern without a wildcard names a file, so its fixed
directory is the parent directory. The fixed directory of `**/*.ts` is the scan
root.

The watched area is the union of the fixed directories of every `include`
pattern of every template rule and semantic rule.

A source file is uncovered when all of these are true:

- Its path is inside the watched area.
- It has a supported source suffix.
- No rule selects it by `include`. A template rule selects a file only when the
  language of its template matches the file. A semantic rule selects a file
  only when the file has the language that the rule kind requires.

A file that a rule includes and then excludes is covered. The `exclude` entry
is a visible decision in the configuration, and `guard` reports new excludes.

Section 16 still applies, so the scan does not enter `.git`, `.ctmpl`,
`node_modules`, or `target`.

Each uncovered file MUST produce one `CTC5101` diagnostic with exit class `1`
at line 1 column 1 of the file. The diagnostic has no rule identifier. The
diagnostics MUST follow the order in section 44. A run without uncovered files
prints no diagnostics and exits with `0`. Configuration errors follow the
normal exit class `2` rules.

### 47.3 `guard`

```text
ctc guard --base <git-ref> [--allow-rule-changes] [--root <path>]
  [--config <path>] [--format human|json] [--color auto|always|never]
```

`guard` compares the working tree with the configuration and the sources at a
Git reference, and reports changes that make the rules weaker. It does not run
any rule.

The command runs `git` from the `PATH`. A base reference that starts with `-`,
that Git cannot resolve, or a missing `git` executable MUST produce a `CTC0001`
diagnostic with exit class `2`. The base version of a file is the content at
`<git-ref>:./<path>`, with the scan root as the working directory.

`guard` reads rules from `rules` and `semanticRules` as JSON. It compares the
rules of the base configuration with the rules of the current configuration by
identifier. When the base reference has no configuration file, or the file is
not valid JSON, the rule checks below are skipped. The suppression check always
runs. Line endings are ignored when file contents are compared.

Findings have exit class `1`:

| Code | Finding |
|---|---|
| `CTC5201` | A suppression comment for a rule was added. |
| `CTC5202` | A rule selects fewer files than at the base reference. |
| `CTC5203` | A rule was removed, or now sets `allowIgnore` to `true`. |
| `CTC5204` | The definition of a rule changed. |

`CTC5201` compares the number of suppression comments for each rule in each
source file that contains `ctc-ignore` with the number in the base version of
the file. Each comment above the base count, counted from the end of the file,
MUST produce one diagnostic at the comment. A file that is missing at the base
has a base count of `0`. Moving or editing existing comments is not a finding.

`CTC5202` uses the current source files. A file counts when the base version
of the rule selected it (`include` matched and `exclude` did not) and the
current version does not. The command MUST report one diagnostic for each rule
with the number of files and one example. Rewriting a pattern without changing
the selected files is not a finding. Diagnostics of `CTC5202` to `CTC5204` are
attached to line 1 column 1 of the configuration file and carry the rule
identifier.

`CTC5204` compares every rule key except `id`, `include`, `exclude`,
`message`, and `allowIgnore`. It also compares the content of each template that the base and the current rule both list. When the base reference has no such template
file because its directory is a Git submodule, the command compares the commit
of the submodule at the base reference with the commit that is checked out (or
staged). A different commit is a `CTC5204` finding. `--allow-rule-changes`
removes `CTC5204` findings. It does not affect the other findings.

Adding a rule, removing suppression comments, and widening `include` are never
findings.

Removing a file is not a finding, and renaming a file that has suppression
comments reports those comments as added.

## 48. `validate-template`

`validate-template` MUST:

- Validate UTF-8.
- Parse directives.
- Validate the template filename.
- Select the language adapter.
- Lex placeholders.
- Validate filters.
- Generate sentinels.
- Parse the generated template.
- Validate placeholder contexts.

The command MUST NOT require a matching source file.

The command MUST NOT run source matching.

A `fileName` filter needs a source file, so this command checks only its
syntax, its case name, and its position.

## 49. Determinism

The same executable, inputs, and paths MUST produce the same:

- Exit code.
- Match result.
- Diagnostic codes.
- Diagnostic order.
- Diagnostic ranges.
- JSON document.

Hash-map iteration order MUST NOT affect observable output.

File-system enumeration order MUST NOT affect observable output.

## 50. Safety

The tool MUST NOT execute source files.

The tool MUST NOT execute templates.

The tool MUST NOT load project JavaScript.

The tool MUST NOT read files selected through followed directory symbolic
links.

The tool MUST NOT access the network during a check.

## 51. Performance Requirements

The tool MUST parse each source file at most once per command.

The tool MUST compile each selected template at most once per command.

The matcher MUST enforce the 100,000-state sequence limit.

The implementation CAN process independent source files in parallel.

Parallel work MUST preserve diagnostic order.

No wall-clock performance threshold is part of the MVP.

## 52. Standalone Release Verification

A release test MUST:

1. Build `ctc` in release mode.
2. Copy only the executable and a fixture project to a clean environment.
3. Remove Node.js from `PATH`.
4. Run `ctc`.
5. Compare the exit code and JSON report with the fixture expectation.

The release test MUST run for each published target.

## 53. C++ Adapter

The C++ syntax adapter uses `tree-sitter-cpp`.

The adapter maps C++ parser nodes to the same canonical node model.

C++ templates can use suffixes such as:

```text
class-factory.cpp.ctmpl
header-layout.hpp.ctmpl
```

The C++ adapter defines its own:

- Canonical kinds.
- Canonical fields.
- Identifier rules.
- Placeholder categories.
- Sentinel contexts.

Compiler-accurate C++ checks require Clang and a compilation database. Those
checks remain outside the syntax-only adapter.

## 54. MVP Conformance

An implementation conforms to this specification when:

- All MVP acceptance fixtures pass.
- Every required diagnostic code has one fixture.
- Human and JSON golden reports pass on Windows and Linux.
- Glob results match on Windows, Linux, and macOS.
- The standalone release test passes.
- The canonical matcher contains no language-specific or Tree-sitter types.
- The `service-adapter` fixture reports value imports outside
  `src/index.ts`.
- The C++ fixture matches one class-factory rule and reports one nested
  `throw` statement.
