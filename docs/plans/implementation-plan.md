# Code Template Check Implementation Plan

The normative MVP behavior is defined in
[`docs/specification.md`](../specification.md).

## Goal

Build a standalone Rust CLI that checks source files against external,
code-shaped rules. Each rule uses a source-language template with
Nunjucks-like placeholders. Matching uses canonical syntax nodes instead of
source text.

The implementation includes TypeScript and C++ language adapters. They use
`tree-sitter-typescript` and `tree-sitter-cpp` in the `ctc` executable.
TypeScript semantic rules use conservative syntax analysis. Compiler-accurate
C++ semantic rules remain future work.

The matcher must support different structural rules without domain-specific
logic. The first reference fixture is a module that:

- Exports one or more public interfaces.
- Keeps its implementation class private.
- Exports a factory function instead of the class.
- Uses related names such as `User`, `UserImplementation`, and `createUser`.

Example source:

```ts
export interface UserOptions {
  name: string;
}

export interface User {
  readonly name: string;
}

class UserImplementation implements User {
  constructor(public readonly name: string) {}
}

export const createUser = (options: UserOptions): User =>
  new UserImplementation(options.name);
```

The class-factory pattern is not a built-in rule. It is one template that proves
the generic matcher can enforce a useful structure.

## Design Principles

1. The core matcher must not depend on ESLint, Oxlint, or an editor.
2. The release artifact must be one native `ctc` executable.
3. The executable must not require Node.js or another language runtime.
4. Language-specific parsing must stay behind a language-adapter interface.
5. Templates must look close to valid source code in their target language.
6. Formatting, comments, and source locations must not affect matching.
7. Repeated placeholders must capture and compare canonical nodes consistently.
8. Errors must point to the closest source node that failed to match.
9. The MVP remains syntax-aware but does not require type checking.
10. The design must allow semantic language adapters to be added later.
11. The compiler and matcher must not contain class-factory or project-specific
    rules.
12. One source file can be checked by multiple independent rules.
13. Matching and diagnostics must be deterministic for the same inputs.
14. Project file selection must remain separate from shareable templates.

The standalone requirement selects Rust over TypeScript, Go, and a bundled
Node.js executable. Rust can statically include Tree-sitter grammars and expose
one language-neutral matcher.

## Core Model

A rule joins four parts:

- A stable rule identifier.
- Include and exclude patterns.
- One external template.
- A match policy.

A template contains literal source code and generic syntax placeholders. A
placeholder can capture one node, capture a node list, derive an identifier, or
apply a syntax constraint.

The MVP matches templates at the source-file scope. The post-MVP search-scope
feature adds descendant, function-body, and class-body searches without changing
template syntax.

The source suffix selects a language adapter. TypeScript uses `.ts`, `.mts`,
and `.cts`. C++ uses `.cpp`, `.cc`, `.cxx`, `.h`, `.hh`, `.hpp`, and `.hxx`.

Each language adapter supplies:

- Supported source suffixes.
- Parser creation.
- Placeholder discovery outside comments and literals.
- Placeholder categories and sentinel rules.
- Canonical node kinds and fields.
- Identifier validation.
- Parse diagnostics.

## Proposed Template Syntax

```ts
{{* Imports | kind("ImportDeclaration") }}

export interface {{ PublicName | suffix("Options") }} {
  {{* optionMembers }}
}

export interface {{ PublicName }} {
  {{* publicMembers }}
}

class {{ PublicName | suffix("Implementation") }}
  implements {{ PublicName }} {
  {{* implementationMembers }}
}

export const {{ PublicName | prefix("create") }} = (
  {{* factoryParameters }}
): {{ PublicName }} =>
  new {{ PublicName | suffix("Implementation") }}(
    {{* constructorArguments }}
  );
```

An unrelated template can require one exported function:

```ts
export function {{ identifier:FunctionName }}(
  {{* Parameters }}
): {{ type:ReturnType }} {
  {{* Statements }}
}
```

### MVP placeholders

| Syntax | Meaning |
|---|---|
| `{{ Name }}` | Capture one syntax node, or require equality when reused |
| `{{ identifier:Name }}` | Capture exactly one identifier |
| `{{ expression:Name }}` | Capture exactly one expression |
| `{{ type:Name }}` | Capture exactly one TypeScript type node |
| `{{* Name }}` | Capture zero or more nodes in the current list |
| `{{? Name }}` | Capture zero or one node in the current list |
| `{{? keyword:async }}` | Match the literal `async` keyword or no keyword |
| `{{ Name \| prefix("create") }}` | Match a derived identifier |
| `{{ Name \| suffix("Implementation") }}` | Match a derived identifier |
| `{{ Name \| kind("VariableStatement") }}` | Require one canonical node kind |
| `{{* Name \| kind("ImportDeclaration") }}` | Require every captured node to have the given kind |
| `{{ Name \| field("typeOnly", "equal", true) }}` | Require a canonical syntax field value |

`kind()` accepts one or more kind names from the selected language adapter. The
template compiler validates each name against the adapter. This constraint
prevents a sequence named `Imports` from capturing exports or other statements.

An untyped scalar placeholder captures a complete list item when it is the only
syntax in that item:

```ts
{{ Declaration | kind("VariableStatement") }}
```

In this example, the placeholder captures one top-level variable statement.
Use an explicit category when the placeholder must capture a smaller node.

### Placeholder grammar

The MVP placeholder grammar is:

```text
{{ [*|?] [category:]Name [| filter("argument", ...)]... }}
```

Rules for this grammar:

- A placeholder must stay on one line.
- `*` means zero or more nodes.
- `?` means zero or one node.
- A capture name must match `[A-Za-z_][A-Za-z0-9_]*`.
- The lexer ignores placeholder text in comments, string literals, regular
  expressions, and template-literal text.
- The supported filters are `prefix`, `suffix`, `kind`, and `field`.
- The supported constraint filters are `kind` and `field`.
- Filter arguments use JSON scalar syntax.
- `prefix` and `suffix` each accept one string.
- `prefix` and `suffix` concatenate exact text without changing letter case.
- A derived result must be a valid TypeScript identifier.
- `kind` accepts one or more valid kind names from the language adapter.
- `field` accepts a canonical field name, an operator, and an optional JSON
  scalar value.
- `field` operators are `equal`, `notEqual`, `exists`, and `notExists`.
- `equal` and `notEqual` require a value.
- `exists` and `notExists` do not accept a value.
- A missing field satisfies `notEqual` and `notExists`.
- A missing field does not satisfy `equal` or `exists`.
- A plain use of a name captures its canonical syntax value.
- A later plain use of the same name requires equality.
- A derived use can occur before its source capture.
- Every derived source name must also have a plain capture in the template.
- Reusing a name with a different category or cardinality is a template error.
- Repeated sequence captures compare canonical node lists.
- `keyword` placeholders must use `?` and one known language keyword.

Each rule evaluation gets a new capture environment. Candidate matches in
`contains` mode also get isolated capture environments.

### Deferred placeholders

The following placeholders and match policies are outside the current feature
set:

- Alternation between multiple valid structures.
- User-defined filters.
- Typed semantic constraints.
- Cross-file captures.
- Cardinalities such as exactly one match and every match.

## Configuration and File Selection

The project root contains `.ctc.json`. The `.ctmpl` directory contains
shareable templates. This split permits `.ctmpl` to be a Git submodule.

```text
project/
  .ctc.json
  .ctmpl/
    no-value-imports.ts.ctmpl
    class-factory.ts.ctmpl
  src/
    index.ts
    interfaces.ts
    adapter.ts
```

The configuration schema is:

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
      "scope": "topLevel"
    },
    {
      "id": "class-factory",
      "template": ".ctmpl/class-factory.ts.ctmpl",
      "include": ["src/**/*.ts"],
      "exclude": ["src/index.ts", "src/interfaces.ts"],
      "mode": "exact",
      "scope": "topLevel"
    }
  ]
}
```

Rule identifiers match `[a-z][a-z0-9-]*` and are unique. Each rule has one
template and at least one include pattern. Exclude patterns are optional.
`mode` defaults to `exact`. `scope` defaults to `topLevel`.

Patterns use `/` separators on all operating systems. They are relative to the
scan root. Absolute paths, backslashes, and parent components are invalid.

The default configuration path is `<root>/.ctc.json`. `--config` selects
another configuration inside the scan root. Template paths remain relative to
the scan root.

The source scan does not enter `.git`, `.ctmpl`, `node_modules`, or `target`.
It does not follow directory symbolic links. Explicit CLI paths limit the
selected sources after configuration validation.

All configured rules that select a source file run independently. Captures do
not pass between rules or files. The rule runner parses each source file once.

Internal paths use normalized absolute paths. Diagnostics use paths relative to
the scan root and `/` separators on all operating systems. Path comparison uses
the case rules of the host file system.

## `service-adapter` Reference

The `tests/fixtures/service-adapter` project is the first integration fixture.
Its template layout is:

```text
service-adapter/
  .ctc.json
  .ctmpl/
    no-value-imports.ts.ctmpl
    class-factory.ts.ctmpl
```

The `no-value-imports` rule applies to all TypeScript source files except
`src/index.ts`. The index file remains the composition root and can use runtime
imports.

The `class-factory` rule applies to class modules. Its exclude patterns remove
the composition root and modules that only define shared interfaces or
constants.

The fixture contains a normal import outside `src/index.ts`. The first
integration run must report this violation. The fixture does not assume that
the project already conforms.

The no-value-import rule reports the import in `src/adapter.ts`.

`src/logger.ts` already uses `import type`. Files without imports also pass this
rule.

The no-value-import template is:

```ts
{{ Import | kind("ImportDeclaration", "ImportEqualsDeclaration") | field("typeOnly", "notEqual", true) }}
```

This rule permits `import type` declarations. It rejects normal imports,
side-effect imports, and value-producing TypeScript import assignments.

This rule does not inspect `require()` calls or dynamic `import()` expressions.
Separate descendant-scope templates can enforce those policies.

## Architecture

```text
CLI
 |
 +-- configuration loader
 |
 +-- file selector
 |    +-- include globs
 |    +-- exclude globs
 |
 +-- rule runner
 |    +-- compiled template cache
 |    +-- parsed source cache
 |    +-- semantic policy evaluator
 |
 +-- language registry
 |    +-- TypeScript adapter
 |         +-- tree-sitter-typescript grammar
 |         +-- placeholder context scanner
 |         +-- canonical node mapper
 |         +-- semantic fact extractor
 |         +-- TypeScript sentinel rules
 |    +-- C++ adapter
 |         +-- tree-sitter-cpp grammar
 |         +-- placeholder context scanner
 |         +-- canonical node mapper
 |         +-- C++ sentinel rules
 |
 +-- template compiler
 |    +-- placeholder lexer
 |    +-- placeholder validator
 |    +-- sentinel generator
 |    +-- language adapter
 |    +-- template offset map
 |
 +-- canonical syntax matcher
 |    +-- scalar node matcher
 |    +-- sequence matcher
 |    +-- capture environment
 |    +-- generic syntax constraints
 |    +-- derived identifier filters
 |
 +-- diagnostic formatter
      +-- human output
      +-- JSON output
```

## Parser Strategy

Use the Rust `tree-sitter` crate behind a language-adapter trait. Statically
link both language grammars into the executable. Pin the grammar and parser
crate versions in `Cargo.lock`.

The tool accepts the TypeScript and C++ suffixes in the specification. It
rejects JavaScript, JSX, TSX, TypeScript declaration files, and `.c` files.

The generic matcher never stores Tree-sitter nodes. Each language adapter
converts parser nodes into owned canonical nodes before matching. This boundary
lets the TypeScript and C++ adapters use separate parsers.

The Rust adapter interface is:

```rust
pub trait LanguageAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn supports_path(&self, path: &Path) -> bool;
    fn scan_placeholders(
        &self,
        source: &str,
        path: &Path,
    ) -> Result<Vec<PlaceholderRange>, Vec<Diagnostic>>;
    fn parse(&self, source: &str, path: &Path) -> Result<ParsedTree, Diagnostic>;
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

The template loader parses leading `ctc` directives for ad hoc templates.
Configured rules get their mode from `.ctc.json`.

Templates with a match mode are not directly valid TypeScript. The compiler
uses this process:

1. Lex all placeholders and record their original ranges.
2. Generate a unique identifier prefix that does not occur in the template.
3. Replace each placeholder with one unique identifier sentinel.
4. Record every generated range and its original template range.
5. Parse the generated TypeScript once with the language adapter.
6. Find each sentinel identifier by its generated range.
7. Infer the scalar or list slot from the sentinel parent.
8. Convert the sentinel syntax node into an internal matcher node.

An identifier is valid in all planned scalar slots. It also forms a valid item
in each planned list. For example, TypeScript parses it as an expression
statement, parameter, argument, property, or type member.

For an explicit `identifier`, `expression`, or `type` placeholder, the compiler
replaces the sentinel node in that slot.

For an untyped scalar placeholder, the compiler first examines its list item.
It replaces the full item when the sentinel is the only syntax in that item.
Otherwise, it replaces only the sentinel identifier node.

For a sequence placeholder, the compiler replaces the nearest supported list
item. It rejects a sequence sentinel when other syntax shares that list item.

The offset map translates parser ranges from generated TypeScript back to the
original template. Template syntax errors and matcher diagnostics must always
use original template positions.

The compiler converts sentinel syntax nodes into language-neutral matcher
nodes:

```rust
pub enum TemplateNode {
    Literal(CanonicalNode),
    Capture {
        name: CaptureName,
        category: CaptureCategory,
        constraints: Vec<NodeConstraint>,
    },
    Sequence {
        name: CaptureName,
        category: SequenceCategory,
        constraints: Vec<NodeConstraint>,
    },
    Derived {
        source: CaptureName,
        filters: Vec<NameFilter>,
    },
}
```

The compiled template stores its match mode separately from its matcher tree.
The configured rule stores file globs. File globs never become syntax matcher
nodes.

The adapter rejects source or generated template trees that contain
Tree-sitter error or missing nodes. It does not use a `tsconfig.json` file,
module resolution, or type checking.

Before the full compiler, create a parser spike for identifiers, expressions,
types, statements, parameters, arguments, interface members, and class members.
If a planned slot cannot use one identifier sentinel safely, revise the
placeholder syntax.

## Matching Rules

### Canonical syntax form

Build an owned canonical tree from language-adapter nodes and tokens. Do not
store raw parser objects in the matcher.

For each node, record its canonical kind, fields, scalar value, children, and
source range. Walk structural children in source order. The adapter removes
parser-only container nodes.

```rust
pub struct CanonicalNode {
    pub kind: Arc<str>,
    pub value: Option<CanonicalScalar>,
    pub fields: BTreeMap<Arc<str>, CanonicalScalar>,
    pub children: Vec<CanonicalNode>,
    pub range: SourceRange,
}
```

Use decoded values for identifiers and literals. Store each keyword modifier as
a boolean canonical field. Keep decorator order because decorator order can
change behavior.

Canonical nodes also expose named fields for generic constraints. These fields
form a versioned tool contract and do not expose raw parser objects. The MVP
includes `typeOnly` for import declarations and import assignments.

`field()` reads these canonical fields. An unknown field name is a template
error. Add a field only with canonicalization tests for every node kind that
can contain it.

The TypeScript adapter exposes stable PascalCase kinds such as
`ImportDeclaration` and `ClassDeclaration`. These are `ctc` API names, not raw
Tree-sitter node names.

Ignore:

- Source positions.
- Raw source text.
- Comments.
- Parent pointers.
- Parser bookkeeping.
- Byte-order marks and line endings.
- Optional semicolon tokens.
- Trailing commas.

Keep:

- Syntax kind.
- Identifier text.
- Decoded literal values.
- Operators and required punctuation.
- Modifier presence, such as `export`, `readonly`, and `private`.
- Type annotations.
- Child node order.
- Optional child presence.

Keep source ranges for diagnostics, but exclude them from equality.

An explicit `ParenthesizedExpression` remains significant because parentheses
can change program meaning. Modifier order does not affect matching, but
modifier presence does.

The canonicalizer must support every literal node and token that the bundled
TypeScript adapter accepts. It must report an internal error for an unsupported
adapter kind.

### Captures

The first use of a placeholder stores its canonical syntax value:

```text
{{ PublicName }} -> User
```

Later uses must match the stored value. Derived placeholders operate on
identifier captures:

```text
{{ PublicName | prefix("create") }}          -> createUser
{{ PublicName | suffix("Implementation") }} -> UserImplementation
```

A derived placeholder can occur before its plain capture. In that case, store
its observed identifier as a pending constraint. Resolve the constraint when
the plain capture occurs.

Applying a naming filter to a non-identifier capture is a template error.
Filters run from left to right. A `kind()` filter tests a node but does not
change its captured value.

### Sequence matching

`{{* Name }}` matches a list of zero or more sibling nodes. Match fixed nodes
before and after the sequence placeholder, then capture the nodes between them.

Use left-to-right, non-greedy matching. When one list contains multiple sequence
placeholders, use memoized backtracking. Include the capture-environment
fingerprint in each memoization key.

Reject all adjacent sequence placeholders in the MVP. Their capture boundaries
are not clear, even when constraints differ.

Apply sequence constraints to every captured node. A repeated sequence name must equal its first canonical node list.

Limit each rule and source pair to 100,000 sequence states. If the matcher
reaches this limit, report a template complexity error.

### Match modes

`exact` compares the complete template statement list with the complete source
statement list. Every additional statement requires an explicit sequence
placeholder that accepts its syntax kind.

`contains` searches each possible start position in the top-level source
statement list. It allows statements before and after the matched region. It
does not skip statements inside that region unless a sequence placeholder
accepts them.

Search candidate starts in source order.

`contains` does not search nested syntax lists in the MVP. A template must include
the enclosing declaration for a nested structure.

One valid candidate makes a `contains` rule pass. Captures from failed
candidates do not affect later candidates.

`forbid` uses the same top-level search as `contains`. The rule passes only when
no candidate matches. Each matching candidate produces one diagnostic at its
first source node.

Forbid candidates use isolated capture environments. Diagnostics use the source
start position as their final stable sort key.

A contains or forbid template must consume at least one source statement.
Reject a search template that can match an empty statement list.

### Search scopes

Configured and ad hoc rules support:

- `topLevel` for direct source-file statements.
- `descendants` for the complete syntax tree.
- `functionBody` for syntax inside function-like bodies.
- `classBody` for syntax inside class bodies and their methods.

`topLevel` is the default. Exact mode rejects other scopes.

### Semantic policies

The TypeScript adapter extracts function control-flow, exception, rejection,
and call facts during its existing parse.

`resultOrControlFlow` checks functions whose return annotation contains a
configured result type. It reports fallthrough paths and bare returns.

`exceptionPolicy` can report `try`, `throw`, promise rejection, rejection
callbacks, and configured call patterns.

This analysis is conservative. It does not replace TypeScript type checking or
resolve imported symbols.

### Failure selection

A failed rule reports one primary mismatch. Rank candidate failures by:

1. The number of matched literal and capture nodes.
2. The greatest source offset reached.
3. The earliest candidate start position.

This ranking keeps diagnostics stable during sequence backtracking.

## Diagnostics

Human-readable output:

```text
src/adapter.ts:12:14 [class-factory] template mismatch
  Expected factory name: createServiceAdapter
  Found: makeServiceAdapter
  Template: .ctmpl/class-factory.ts.ctmpl:17:1
```

A forbidden import diagnostic is:

```text
src/adapter.ts:1:1 [no-value-imports] forbidden structure
  Found: ImportDeclaration without typeOnly=true
  Template: .ctmpl/no-value-imports.ts.ctmpl:1:1
```

Each diagnostic has a stable code, category, message, rule identifier, and
optional source and template ranges. Offsets are zero-based UTF-8 bytes. Lines
and Unicode scalar columns are one-based. End positions are exclusive.

JSON output uses one versioned document:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPosition {
    pub offset: u64,
    pub line: u32,
    pub column: u32,
}

#[derive(Serialize)]
pub struct TextRange {
    pub path: String,
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub category: DiagnosticCategory,
    pub message: String,
    pub rule_id: Option<String>,
    pub source: Option<TextRange>,
    pub template: Option<TextRange>,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub schema_version: u32,
    pub matches: bool,
    pub diagnostics: Vec<Diagnostic>,
}
```

Diagnostic categories:

- Invalid command line.
- Invalid configuration.
- Invalid template filename.
- Configured rule selects no files.
- Invalid template directive.
- No selected source files.
- No applicable rule.
- Unsupported source file type.
- Invalid UTF-8.
- Invalid template syntax.
- Unsupported placeholder context.
- Invalid node-kind constraint.
- Invalid canonical field constraint.
- Derived source name has no capture.
- Placeholder category mismatch.
- Ambiguous sequence placeholder.
- Template complexity limit reached.
- Empty search template.
- Source parse error.
- Missing required syntax node.
- Unexpected syntax node.
- Forbidden structure matched.
- Captured value mismatch.
- Derived identifier mismatch.
- ResultOr function can fall through.
- ResultOr function has a bare return.
- Forbidden try statement.
- Forbidden throw statement.
- Promise rejection.
- Configured exception-source call.
- Internal tool error.

Exit codes:

- `0`: all files match.
- `1`: a rule mismatch or source parse error occurred.
- `2`: a CLI, configuration, template, or internal tool error occurred.

Sort diagnostics by source path, rule identifier, source position, and
diagnostic code. Write formatted reports to standard output. Write only
unexpected internal faults to standard error.

Support `--format human` and `--format json` in the MVP. JSON mode must write
exactly one `RunReport` document to standard output.

Human output uses ANSI colors on terminals. Support
`--color auto|always|never`. Auto mode obeys `NO_COLOR`. JSON output never uses
color sequences.

## CLI

Initial commands:

```text
ctc
ctc src
ctc src/adapter.ts
ctc --root tests/fixtures/service-adapter
ctc --config strict.ctc.json
ctc --rule no-value-imports
ctc src/adapter.ts --template one-off.ts.ctmpl
ctc --format json
ctc validate-template ".ctmpl/class-factory.ts.ctmpl"
```

The release executable is named `ctc`.
The hidden `ctc check` form remains a compatibility alias during `0.x`.

`--root` sets the scan root. It defaults to the current directory.

`--rule` is repeatable and limits a discovered run to the named rule
identifiers. An unknown rule identifier is a configuration error.

`--template` creates one temporary source-file rule named `command-line`. It
requires one or more explicit source files. It accepts `--mode` and cannot be
combined with `--rule`.

`--mode` overrides a mode directive in an ad hoc template. It is invalid
without `--template`.

Without `--template`, explicit source files still use every applicable
discovered rule.

## Repository Layout

```text
code-template-check/
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml
  crates/
    ctc-cli/
      Cargo.toml
      src/
        main.rs
        commands/
          check.rs
          validate_template.rs
    ctc-core/
      Cargo.toml
      src/
        canonical/
          mod.rs
          node.rs
        diagnostics/
          mod.rs
          human.rs
          json.rs
        discovery/
          mod.rs
          config.rs
          globs.rs
        matcher/
          mod.rs
          captures.rs
          node.rs
          sequence.rs
        template/
          mod.rs
          lexer.rs
          offsets.rs
          sentinels.rs
        engine.rs
        language.rs
        lib.rs
    ctc-language-typescript/
      Cargo.toml
      src/
        canonicalize.rs
        fields.rs
        kinds.rs
        lib.rs
        sentinels.rs
  docs/
    plans/
      implementation-plan.md
    specification.md
  tests/
    fixtures/
      class-factory/
      exported-function/
      service-adapter/
    cli/
```

## Initial Rust Dependencies

Use these runtime crates:

- `clap` for command-line parsing.
- `globset` for portable include and exclude patterns.
- `serde` and `serde_json` for JSON output.
- `tree-sitter`, `tree-sitter-typescript`, and `tree-sitter-cpp` for parsing.

Use these development crates:

- `assert_cmd` for CLI tests.
- `tempfile` for isolated configuration fixtures.

Do not add parallel execution until profiling shows that it improves a real
fixture. The design permits parallel file checks later.

## Rust Library API

Keep the matcher in `ctc-core` so the CLI does not own matching logic. Expose
owned Rust types and do not expose Tree-sitter node lifetimes.

```rust
pub enum MatchMode {
    Exact,
    Contains,
    Forbid,
}

pub struct Engine {
    languages: LanguageRegistry,
}

impl Engine {
    pub fn discover_rules(
        &self,
        options: &DiscoverRulesOptions,
    ) -> Result<Vec<DiscoveredRule>, Vec<Diagnostic>>;

    pub fn compile_template(
        &self,
        options: &CompileTemplateOptions,
    ) -> Result<CompiledTemplate, Vec<Diagnostic>>;

    pub fn parse_source(
        &self,
        options: &ParseSourceOptions,
    ) -> Result<ParsedSource, Vec<Diagnostic>>;

    pub fn match_template(
        &self,
        options: &MatchTemplateOptions,
    ) -> CheckResult;

    pub fn check(&self, options: &CheckOptions) -> RunReport;
}
```

`ctc-language-typescript` registers the MVP adapter with the engine.
`CompiledTemplate` and `ParsedSource` contain only owned canonical nodes.

Expected input errors return diagnostics. Internal invariant failures return a
typed Rust error that the CLI converts to exit code `2`.

## ESLint and Oxlint Adapters

Additional language adapters are post-MVP features.

The ESLint adapter exposes one rule:

```text
code-template-check/conforms-to-template
```

The ESLint adapter must:

- Load the project configuration for the current filename.
- Select all applicable rules for the current filename.
- Reuse the source text already loaded by ESLint.
- Invoke the native `ctc` binary with the source text on standard input.
- Request one JSON report for all selected rules.
- Translate core diagnostics into ESLint reports.

The first adapter can invoke one process per file. Phase 4 must measure this
cost. If it is too high, add a `napi-rs` binding or a persistent protocol
without changing the Rust matcher.

Phase 5 loads the JavaScript adapter through the plugin API in Oxlint. Support
remains optional until the adapter passes the same contract tests under both
linters.

## Testing Strategy

Use test-first development for each matcher feature.

### Unit tests

- Placeholder tokenization.
- Invalid and unterminated placeholders.
- Placeholders ignored in comments, strings, regular expressions, and
  template-literal text.
- Invalid capture names and filter arguments.
- Template directive parsing and conflicts.
- Sentinel-name collision handling.
- Sentinel generation for every supported syntax slot.
- Generated-to-template offset mapping with LF and CRLF files.
- Canonical syntax-tree generation.
- Tree-sitter error-node and missing-node rejection.
- Formatting equivalence for comments, literal spelling, semicolons, and
  trailing commas.
- Significant explicit parentheses.
- Single-node captures.
- Repeated captures.
- Repeated sequence captures.
- Derived identifiers before and after their source captures.
- Derived identifier filters.
- Generic `kind()` constraints.
- Canonical `field()` constraints.
- Zero-length and multi-node sequence captures.
- Ambiguous sequence rejection.
- Sequence state limit.
- Exact, contains, and forbid modes.
- Empty contains and forbid template rejection.
- One diagnostic for each forbidden candidate.
- Capture isolation between candidates, rules, and files.
- Stable failure selection.
- JSON configuration validation.
- Portable include and exclude globs.
- Duplicate rule identifier rejection.
- Missing and stale rule detection.

### Golden fixture tests

Each template fixture must have:

- One baseline valid source file.
- Valid formatting variations.
- One invalid fixture per expected diagnostic.
- Expected human diagnostic output.
- Expected JSON diagnostic output.

The MVP must include at least two unrelated template families. Use the
class-factory fixture and an exported-function fixture. No matcher code can
refer to names or concepts from either fixture.

Add one fixture file that must pass two independent discovered rules. Add one
class-factory fixture that exports the implementation through an export
declaration. The constrained `Imports` sequence must reject that file.

Keep the `service-adapter` file shapes in an integration fixture. Do not make
tests depend on an external working directory.

### CLI integration tests

- Default `.ctc.json` loading.
- Explicit `--config` path.
- Include and exclude globs.
- Flat shareable templates.
- Overlapping rules and multiple rules per file.
- Duplicate rule identifiers.
- A configured rule with no source files.
- An explicit file with no applicable rules.
- Ad hoc `--template` mode and option conflicts.
- Invalid source and template syntax.
- Unsupported source extensions.
- Exit codes.
- One-document JSON output.
- Deterministic diagnostic order.
- Windows and POSIX path normalization.

### Adapter contract tests

Run the same valid and invalid fixtures through:

- The Rust core API.
- The TypeScript language adapter.
- ESLint.
- Oxlint when supported.

The reported file, line, column, and message must be equivalent.

## Delivery Phases

The MVP ends after Phase 3. Phases 4 through 6 add integrations and release
work without changing the core match contract.

### Phase 0: Project scaffold and parser spike

- Initialize a Rust 2024 Cargo workspace.
- Add `ctc-core`, `ctc-cli`, and `ctc-language-typescript`.
- Pin the Rust toolchain in `rust-toolchain.toml`.
- Pin `tree-sitter` and `tree-sitter-typescript` in `Cargo.lock`.
- Add `rustfmt`, Clippy, and Cargo tests.
- Define fixture-based test helpers.
- Prove that `cargo build --release` creates one `ctc` executable.
- Prove that the executable starts without Node.js on `PATH`.
- Prove that one unique identifier sentinel works in every planned syntax slot.
- Prove that generated ranges map back to original template ranges.
- Parse one class-factory template and one unrelated exported-function template.
- Parse the no-value-import template and its `field()` constraint.
- Record unsupported syntax before continuing.

Exit criterion: all three reference templates compile into generic matcher
nodes. Every diagnostic points to the original template range. The executable
has no Node.js runtime dependency.

### Phase 1: Template compiler

- Implement the placeholder lexer.
- Implement placeholder validation.
- Implement template directives.
- Generate collision-safe sentinels.
- Implement the template offset map.
- Parse templates through the TypeScript language adapter.
- Convert sentinels into internal matcher nodes.
- Implement generic `kind()` constraints.
- Implement canonical `field()` constraints.
- Validate capture reuse and filter compatibility.
- Add useful template syntax diagnostics.

Exit criterion: all template compiler unit and fixture tests pass.

### Phase 2: Canonical matcher

- Build owned canonical source and template trees.
- Implement literal node matching.
- Implement captures and repeated capture equality.
- Implement prefix and suffix filters.
- Implement generic syntax constraints.
- Implement sequence matching with memoization.
- Enforce deterministic sequence choices and the state limit.
- Implement exact, contains, and forbid modes.
- Implement stable failure selection.
- Expose the compile, parse, match, and convenience APIs.

Exit criterion: all three template families accept their valid fixtures and
reject their invalid fixtures at the expected locations. The two positive
families remain structurally unrelated. The matcher contains no fixture names.
The matcher contains no Tree-sitter or TypeScript-specific types.

### Phase 3: Configuration and CLI

- Implement `.ctc.json` loading and schema validation.
- Implement include and exclude globs.
- Implement `--config`.
- Resolve flat template paths from the scan root.
- Apply all matching rules to each source file.
- Cache compiled templates and parsed sources.
- Add human and JSON diagnostic formats.
- Implement the `check` and `validate-template` commands.
- Implement discovered and ad hoc template modes.
- Define stable exit codes.

Exit criterion: one CI command checks a fixture project where one file uses two
independent rules. The `service-adapter` fixture reports value imports
outside `src/index.ts`. `cargo build --release` produces the `ctc` executable.

### Phase 4: ESLint adapter

- Publish a small JavaScript package that locates the platform `ctc` binary.
- Add the `conforms-to-template` ESLint rule.
- Add standard-input support to `ctc`.
- Measure one-process-per-file performance.
- Add a native binding or persistent protocol only if the measurement requires
  it.
- Run adapter contract tests.

Exit criterion: editor and command-line ESLint diagnostics match core CLI
diagnostics.

### Phase 5: Oxlint compatibility

- Load the ESLint-compatible adapter through Oxlint.
- If APIs are unsupported, document them.
- Add Oxlint compatibility tests.
- If Oxlint cannot expose required source locations, keep ESLint as the
  fallback.

Exit criterion: the same fixtures pass under Oxlint, or the limitation is
clearly documented without weakening the core tool.

### Phase 6: Hardening and release

- Add template authoring documentation.
- Add Windows, Linux, and macOS CI.
- Build Windows x64, Linux x64 musl, macOS x64, and macOS ARM64 executables.
- Add executable archive and checksum tests.
- Test each binary on a machine without Node.js.
- Add sequence-matcher benchmarks and malformed-template fuzz tests.
- Add package provenance and release automation.
- Publish an initial prerelease.

Exit criterion: a clean project can download `ctc`, copy an example `.ctmpl`
rule directory, and enforce it in CI without configuration code.

## MVP Acceptance Criteria

1. A release build must produce one native executable named `ctc`.
2. The executable must run without Node.js or another language runtime.
3. The CLI must load project rules from root `.ctc.json`.
4. Include and exclude globs must work on Windows, Linux, and macOS.
5. `--config` must select another configuration inside the scan root.
6. Every applicable rule must run when several rules select one file.
7. Templates must support identifier, expression, type, and sequence captures.
8. Repeated scalar and sequence captures must match.
9. Prefix, suffix, `kind()`, and `field()` constraints must work.
10. Exact, contains, and forbid modes must follow their documented behavior.
11. Two unrelated positive template families must work without matcher
    changes.
12. The no-value-import rule must permit `import type` outside `src/index.ts`.
13. The no-value-import rule must report each normal import outside
    `src/index.ts`.
14. The class-factory rule must reject an exported implementation.
15. Extra statements must fail in exact mode unless an explicit sequence
    placeholder accepts them.
16. Diagnostics must identify the rule, source range, template range, and
    expected structure.
17. JSON output and diagnostic order must remain stable for the same inputs.
18. The canonical matcher must not depend on TypeScript or Tree-sitter types.
19. The Rust API must support rule loading and compiled input reuse.
20. Tests must run without a TypeScript project or type checking.

## Deferred Scope

- Full TypeScript type checking.
- Verifying structural compatibility between a class and an interface.
- Checking that factory arguments map semantically to constructor arguments.
- Cross-file templates.
- Automatic source generation.
- Automatic fixes or rewrites.
- Arbitrary JavaScript inside templates.
- Network-loaded templates.
- JavaScript, JSX, TSX, and declaration-file parsing.
- A C language adapter.
- Compiler-backed C++ semantic rules.
- Clang integration and `compile_commands.json`.
- Counts inside each class or function. Contains rules support a count for each file.
- Template imports and composition.
- Configuration composition and inheritance.

## Main Risks

### Placeholder context ambiguity

One identifier sentinel produces different parent nodes in different syntax
lists.
The parser spike must prove that the compiler can find and replace each full
list item. Unsupported slots must produce a template diagnostic.

### Sentinel offsets and collisions

Generated identifiers change source offsets and can resemble literal template
identifiers. Use a compile-specific prefix that does not occur in the template.
Use the offset map for all template diagnostics.

### Sequence matching cost

Multiple sequence placeholders can cause excessive backtracking. Reject
adjacent sequences, memoize full match states, and enforce the state limit.

### Glob portability

Operating systems use different path separators and case rules. Normalize
paths to `/` before matching. Reject absolute patterns, backslashes, and parent
components.

### TypeScript syntax coverage

Decorators, overloads, generics, computed names, and declaration files add
syntax shapes. Keep the MVP fixture set explicit. Report unsupported
placeholder contexts instead of matching them incorrectly.

### Tree-sitter grammar changes

Grammar releases can change parser node shapes. Pin the grammar and run the
full canonicalization suite before each update.

### Tree-sitter error recovery

Tree-sitter can produce a tree for invalid source. Reject `ERROR` and missing
nodes before matching so invalid source cannot pass a rule.

### Canonical field growth

`field()` can become an unstable copy of a parser API. Expose only documented
canonical fields that support real rules. Keep parser fields behind the
language adapter.

### Fixture overfitting

The first fixture uses a class-factory structure. A second unrelated fixture and
the no-domain-name review prevent matcher logic from copying that structure.

### Linter API differences

ESLint and Oxlint can expose different parser nodes or source APIs. Keep all
matching in the Rust binary so adapters remain thin.

### Native distribution

Each target needs a separate release binary. Pin the Rust toolchain and test the
released archives on clean systems without Node.js.

### Template language growth

Prevent the template language from becoming a general programming language.
If a concrete template requires a new constraint, add one small declarative
feature.

## First Implementation Slice

Build the smallest vertical path:

1. Compile a source-file template with one identifier capture.
2. Parse one TypeScript source file.
3. Match literal nodes and repeated identifier captures.
4. Map one mismatch to its source and template ranges.
5. Expose it through `ctc source.ts --template template.ts.ctmpl`.

Next, add generic `kind()` and `field()` constraints. Then add forbid mode and
sequence captures. Make all three reference templates pass before configuration
work starts.
