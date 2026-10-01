# Code Template Check

Code Template Check (`ctc`) checks source files against code-shaped templates.
It matches syntax trees, not formatted source text. Today it supports
TypeScript, C++, and Rust in one native Rust executable.

See also:

- [Configuration guide](docs/configuration.md)
- [Template guide](docs/templates.md)
- [Command guide](docs/commands.md)
- [TypeScript guide](docs/languages/typescript.md)
- [C++ guide](docs/languages/cpp.md)
- [Rust guide](docs/languages/rust.md)
- [Specification](docs/specification.md)

## Build

```text
cargo build --release
```

The executable is:

```text
target\release\ctc.exe
```

Use `target\debug\ctc.exe` while developing.

## Five-minute quick start

`ctc` checks the shape of your code. This rule says that every `*.service.ts`
file must hold one class, and that the class name must match the file name.
Nothing else may be in the file except imports. The same idea for C++ and Rust
is in [C++ member order](docs/languages/cpp.md#member-order-in-classes-and-structs)
and [Rust `impl` blocks](docs/languages/rust.md#every-on-impl-blocks).

Project layout:

```text
project\
  .ctc.json
  .ctmpl\
    service-class.ts.ctmpl
  src\
    user.service.ts
    order.service.ts
    billing.service.ts
```

Template. `{{* Imports }}` and `{{* Members }}` accept any number of imports
and class members. `{{ Name | fileName("PascalCase") }}` requires the class name
to be the file name in PascalCase:

```ts
{{* Imports | kind("ImportDeclaration") }}

export class {{ Name | fileName("PascalCase") }} {
  {{* Members }}
}
```

Configuration. Mode `exact` means the whole file must match the template:

```json
{
  "schemaVersion": 1,
  "rules": [
    {
      "id": "service-shape",
      "template": ".ctmpl/service-class.ts.ctmpl",
      "include": ["src/**/*.service.ts"],
      "mode": "exact"
    }
  ]
}
```

Source files. `user.service.ts` follows the rule. The other two break it:

```ts
// src/user.service.ts
import { Db } from "./db";

export class UserService {
  constructor(private db: Db) {}
}
```

```ts
// src/order.service.ts: the class name does not match the file name
import { Db } from "./db";

export class OrdersService {
  constructor(private db: Db) {}
}
```

```ts
// src/billing.service.ts: the file holds something besides the class
import { Db } from "./db";

export class BillingService {
  constructor(private db: Db) {}
}

export function helper() {}
```

Run the check:

```text
ctc
```

Output:

```text
src/billing.service.ts:7:1 [service-shape] The source contains an unexpected syntax node.
  Code: CTC3003
  Expected: end of syntax list
  Found: ExportStatement
  Template: .ctmpl/service-class.ts.ctmpl:1:1
src/order.service.ts:3:14 [service-shape] Capture `Name` must match the file name `order.service.ts`. The file name in PascalCase is `OrderService`.
  Code: CTC3007
  Expected: OrderService
  Found: OrdersService
  Template: .ctmpl/service-class.ts.ctmpl:3:14
```

The exit code is `1` when a rule fails. Other things you can do with templates:
forbid a structure (such as `try` blocks, `eval` calls, or raw pointers), check
the order of class members, require a matching header and source file, and limit
function or file sizes. See [docs/commands.md](docs/commands.md) for command
options and exit codes.
## Supported languages

The template suffix selects the language adapter.

| Source files | Template suffixes | Guide |
|---|---|---|
| `.ts`, `.mts`, `.cts` | `.ts.ctmpl`, `.mts.ctmpl`, `.cts.ctmpl` | [TypeScript](docs/languages/typescript.md) |
| `.cpp`, `.cc`, `.cxx`, `.h`, `.hh`, `.hpp`, `.hxx` | matching C++ suffix before `.ctmpl` | [C++](docs/languages/cpp.md) |
| `.rs` | `.rs.ctmpl` | [Rust](docs/languages/rust.md) |

TypeScript declaration files and C files are not supported.

## Documentation index

- [docs/configuration.md](docs/configuration.md): How `.ctc.json` works, how files are selected, match modes, scopes, ignore comments, and semantic rule setup.
- [docs/templates.md](docs/templates.md): Placeholder syntax, sequence and optional captures, name filters, field constraints, `callee`, and `lineCount`.
- [docs/commands.md](docs/commands.md): CLI commands, ad hoc templates, `explain`, `coverage`, `guard`, exit codes, and diagnostic codes.
- [docs/languages/typescript.md](docs/languages/typescript.md): TypeScript file suffixes, kinds, fields, semantic rules, limits, and verified recipes.
- [docs/languages/cpp.md](docs/languages/cpp.md): C++ file suffixes, kinds, fields, cross-file rules, limits, and verified recipes.
- [docs/languages/rust.md](docs/languages/rust.md): Rust file suffixes, kinds, fields, limits, and verified recipes based on this repo's rules.
- [docs/specification.md](docs/specification.md): Full behavior and diagnostic reference.

## Check this repository

`ctc` checks its own Rust code. The rules are in `.ctc.json` and `.ctmpl\`.

- Tests live in `tests.rs` files, not in inline `mod tests { ... }` blocks.
- Production code has no `unwrap()`, `panic!`, `todo!`, `unimplemented!`, or
  `dbg!`.
- No `#[allow(...)]` or `#[expect(...)]` attributes. Change `clippy.toml`
  instead.
- Functions are limited to 100 lines.
- Source files are limited to 800 lines.
- Test files are limited to 1500 lines.
- Every crate root starts with `#![forbid(unsafe_code)]`.

Run all three checks after building:

```text
ctc
ctc coverage
ctc guard --base origin/main
```
