# TypeScript guide

This page collects the TypeScript-specific parts of `ctc`: file suffixes,
template naming, useful kinds and fields, semantic rules, limits, and verified
rule recipes.

Related pages:

- [README](../../README.md)
- [Configuration guide](../configuration.md)
- [Template guide](../templates.md)
- [Command guide](../commands.md)
- [C++ guide](cpp.md)
- [Rust guide](rust.md)

## File suffixes and template names

| Source files | Template files |
|---|---|
| `.ts`, `.mts`, `.cts` | `.ts.ctmpl`, `.mts.ctmpl`, `.cts.ctmpl` |

TypeScript declaration files are not supported.

## Canonical kinds worth knowing

The adapter keeps stable `ctc` names for a small core set and exposes many
other syntax kinds in PascalCase. These are the names you will use most often.

| Kind | Use |
|---|---|
| `SourceFile` | Whole-file templates |
| `ImportDeclaration` | `import ... from` and side-effect imports |
| `ImportEqualsDeclaration` | `import X = require("...")` |
| `InterfaceDeclaration` | Interface rules |
| `ClassDeclaration` | Normal classes |
| `AbstractClassDeclaration` | `abstract class` rules in `every` mode |
| `Class` | Class expressions |
| `FunctionDeclaration` | Top-level functions |
| `ArrowFunction` | Arrow functions |
| `VariableStatement` | `const`, `let`, and `var` declarations |
| `CallExpression` | Call matching with `callee` |
| `NewExpression` | `new Function(...)` and similar patterns |
| `AwaitExpression` | `await` checks |
| `ExportStatement` | Export form checks, including `typeOnly` |

## Fields

These fields are useful on placeholders:

| Field | Where it appears | Meaning |
|---|---|---|
| `typeOnly` | imports, type declarations, some exports | `true` only for fully type-only forms |
| `access` | direct class members | `public`, `protected`, or `private` |
| `static` | direct class members | `true` for `static` members and static blocks |
| `constructor` | direct class members | `true` for a real constructor |
| `callee` | `CallExpression`, `NewExpression` | Callee text with white space removed |
| `module` | imports, `export ... from`, `require(...)`, `import(...)` | The module name without quotes |

Every node also supports `lineCount`.

## Supported sequence slots

TypeScript templates support sequence placeholders in:

- source-file statements
- function parameters
- call arguments
- constructor arguments
- interface members
- class members

## Semantic rules

TypeScript has two language-specific semantic rules:

- `returnPaths`
- `exceptionPolicy`

It also supports the generic `companionFile` and `fileLength` semantic rules.

### `returnPaths`

Use this when functions that declare a given return type must return a value on
every path. It works for any wrapper type that your code uses, such as `Result<T>` or `Either<E, T>`. `typeName` is required. It is the name of the
type in the return annotation, and it matches when the annotation contains it,
as in `Result<number>` or `Promise<Result<number>>`.

```json
{
  "kind": "returnPaths",
  "id": "return-paths",
  "include": ["src/**/*.ts"],
  "typeName": "Result"
}
```

It reports `CTC4001` at a function where a path can end without a value. A
return without a value in such a function reports `CTC4002`.

Sample. `falls` breaks the rule on line 4 and `ok` does not:

```ts
export function ok(flag: boolean): Result<number> {
  return flag ? 1 : 2;
}
export function falls(flag: boolean): Result<number> {
  if (flag) {
    return 1;
  }
}
```

### `exceptionPolicy`

Use this when a layer must avoid exceptions.

```json
{
  "kind": "exceptionPolicy",
  "id": "no-exceptions",
  "include": ["src/**/*.ts"],
  "forbidTry": true,
  "forbidThrow": true,
  "forbidPromiseReject": true,
  "exceptionSources": ["fetch", "axios.*", "client.request"]
}
```

`exceptionSources` matches compact callee text. It does not resolve imported
aliases.

## Known limits

- `ctc` does not run the TypeScript compiler.
- It does not resolve imports or prove expression types.
- `returnPaths` is conservative. A `throw` can still lead to a
  fallthrough report.
- `typeOnly` is `false` for `import { type Name }`, because the whole import is
  not a type-only declaration.
- In scope objects, `ForInStatement` covers both `for...in` and `for...of`.
- `module` is only set when the module name is a plain string. A computed name
  such as `require(name)` has no `module`, and a template literal argument is
  not read.

## Rule recipes

Every recipe below was checked against `ctc 0.1.0` in a scratch project.

### Value imports

Goal: allow only `import type`.

Template file:

```ts
{{ Import | kind("ImportDeclaration", "ImportEqualsDeclaration") | field("typeOnly", "notEqual", true) }}
```

`.ctc.json` rule:

```json
{
  "id": "no-value-imports",
  "template": ".ctmpl/no-value-imports.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "forbid"
}
```

Violating source:

```ts
import { Logger } from "./logger";
```

Expected diagnostic: `src/service.ts:1:1 CTC3006`

### Class factory in `exact` mode

Goal: require one interface, one implementation class, and one `create...`
factory in a file.

Template file:

```ts
{{* Imports | kind("ImportDeclaration") }}

export interface {{ PublicName | suffix("Options") }} {
  {{* OptionMembers }}
}

export interface {{ PublicName }} {
  {{* PublicMembers }}
}

class {{ PublicName | suffix("Implementation") }}
  implements {{ PublicName }} {
  {{* ImplementationMembers }}
}

export const {{ PublicName | prefix("create") }} = (
  {{* FactoryParameters }}
): {{ PublicName }} =>
  new {{ PublicName | suffix("Implementation") }}(
    {{* ConstructorArguments }}
  );
```

`.ctc.json` rule:

```json
{
  "id": "class-factory",
  "template": ".ctmpl/class-factory.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "exact"
}
```

Violating source:

```ts
export interface ServiceOptions {
  logger: Logger;
}

export interface Service {
  run(): void;
}

class ServiceImplementation implements Service {
  constructor(private readonly logger: Logger) {}

  run(): void {}
}

export const buildService = (
  options: ServiceOptions
): Service =>
  new ServiceImplementation(options.logger);
```

Expected diagnostic: `src/service.ts:15:14 CTC3005`

### Class member order

Goal: keep constructors first, then public members, then protected members,
then private members.

Template file:

```ts
class {{ Name }} {
  {{* Constructors | field("constructor", "equal", true) }}
  {{* PublicMembers | field("access", "equal", "public") }}
  {{* ProtectedMembers | field("access", "equal", "protected") }}
  {{* PrivateMembers | field("access", "equal", "private") }}
}
```

`.ctc.json` rule:

```json
{
  "id": "member-order",
  "template": ".ctmpl/member-order.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "every",
  "scope": "descendants",
  "kinds": ["ClassDeclaration"]
}
```

Violating source:

```ts
export class Service {
  private count = 0;

  run(): number {
    return this.count;
  }
}
```

Expected diagnostic: `src/service.ts:4:3 CTC3003`

### File name must match the class name

Goal: require `order-service.ts` to export `OrderService`.

Template file:

```ts
export class {{ Name | fileName("PascalCase") }} {
  {{* Members }}
}
```

`.ctc.json` rule:

```json
{
  "id": "class-name",
  "template": ".ctmpl/class-name.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "contains"
}
```

Violating source:

```ts
export class OrderManager {
  run(): void {}
}
```

Expected diagnostic: `src/order-service.ts:1:14 CTC3007`

### No `try` or `throw`

Goal: ban exception-based control flow in one layer.

`.ctc.json` rule:

```json
{
  "kind": "exceptionPolicy",
  "id": "no-throw",
  "include": ["src/**/*.ts"],
  "forbidTry": true,
  "forbidThrow": true
}
```

Violating source:

```ts
export function run(): number {
  throw new Error("bad");
}
```

Expected diagnostic: `src/service.ts:2:3 CTC4102`

### Ban `eval`, `$`, and `jQuery` calls

Goal: match calls by their `callee` field.

Template file:

```ts
{{ Call | kind("CallExpression") | field("callee", "matches", "^(window\\.)?(eval|\\$|jQuery)$") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-eval",
  "template": ".ctmpl/no-eval.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```ts
export function run(): void {
  window.eval("1");
}
```

Expected diagnostic: `src/service.ts:2:3 CTC3006`

### Ban a module in every form

Goal: block a module however it is loaded. Import declarations, re-exports
(`export ... from`), `import x = require(...)`, `require("...")`, and dynamic
`import("...")` all have a `module` field. It holds the module name without
quotes. One template covers all of these forms, and a regular expression can
match a whole family of names.

Template file `.ctmpl/no-jquery.ts.ctmpl`:

```ts
{{ Import | field("module", "matches", "^jquery") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-jquery",
  "template": ".ctmpl/no-jquery.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```ts
import $ from "jquery";
import plugin from "jquery/dist/jquery";
import ok from "lodash";
```

Expected diagnostics: `src/a.ts:1:1 CTC3006` and `src/a.ts:2:1 CTC3006`.

The regular expression is not anchored, so `^jquery` also matches `jquery-ui`.
Write `^jquery(/|$)` to match only `jquery` and its sub paths.

### Only one file may use Node `fs`

Goal: all file access goes through a `FileSystem` class that wraps Node. Every
other file is banned from loading `fs`, `node:fs`, `fs/promises`, and
`node:fs/promises`.

Template file `.ctmpl/no-direct-fs.ts.ctmpl`:

```ts
{{ Import | field("module", "matches", "^(node:)?fs(/promises)?$") }}
```

`.ctc.json` rule. The only exception is the wrapper file, named in `exclude`:

```json
{
  "id": "use-filesystem-class",
  "template": ".ctmpl/no-direct-fs.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "exclude": ["src/infra/FileSystem.ts"],
  "mode": "forbid",
  "scope": "descendants",
  "message": "Use the FileSystem class instead of Node fs."
}
```

`src/infra/FileSystem.ts` may import `node:fs`. This other file may not:

```ts
import { FileSystem } from "./infra/FileSystem";
import { readFileSync } from "fs";
export const read = readFileSync;
```

Expected diagnostic: `src/report.ts:2:1 CTC3006`, shown with the message
`Use the FileSystem class instead of Node fs.`.

Run `ctc guard --base origin/main` in CI. It fails when someone adds another
file to `exclude` or edits the rule, so the wrapper stays the only exception.

The `module` field is also set on `export ... from "x"` and on `require("x")`
and `import("x")` calls when the first argument is a plain string. A computed
name such as `require(name)` and `require.resolve("fs")` have no `module`.
`import type { T } from "fs"` is flagged too, because it names the module.

### No `await` inside loops

Goal: search only inside loop nodes.

Template file:

```ts
{{ Forbidden | kind("AwaitExpression") }}
```

`.ctc.json` rule:

```json
{
  "id": "no-await-in-loop",
  "template": ".ctmpl/no-await.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "forbid",
  "scope": {
    "inside": ["ForInStatement", "ForStatement", "WhileStatement", "DoStatement"]
  }
}
```

Violating source:

```ts
export async function run(items: string[]): Promise<void> {
  for (const item of items) {
    await work(item);
  }
}
```

Expected diagnostic: `src/service.ts:3:5 CTC3006`

### Function length

Goal: fail functions that run past a line limit.

Template file:

```ts
{{ Long | kind("FunctionDeclaration") | field("lineCount", "greaterThan", 4) }}
```

The limit is 4 lines here only to keep the sample short. Use a real limit, such as 60 or 100.

`.ctc.json` rule:

```json
{
  "id": "short-functions",
  "template": ".ctmpl/long-function.ts.ctmpl",
  "include": ["src/**/*.ts"],
  "mode": "forbid",
  "scope": "descendants"
}
```

Violating source:

```ts
export function run(): number {
  const a = 1;
  const b = 2;
  return a + b;
}
```

Expected diagnostic: `src/service.ts:1:8 CTC3006`

### File length

Goal: limit total file size without parsing syntax.

`.ctc.json` rule:

```json
{
  "kind": "fileLength",
  "id": "short-files",
  "include": ["src/**/*.ts"],
  "maxLines": 3
}
```

Violating source:

```ts
export const a = 1;
export const b = 2;
export const c = 3;
export const d = 4;
```

Expected diagnostic: `src/service.ts:4:1 CTC4301`
