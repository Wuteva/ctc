# Command guide

This page shows the CLI commands, common options, exit codes, and diagnostic
codes. Use it when you want to run checks, try an ad hoc template, explain a
failure, or guard the rule set.

Related pages:

- [README](../README.md)
- [Configuration guide](configuration.md)
- [Template guide](templates.md)
- [TypeScript guide](languages/typescript.md)
- [C++ guide](languages/cpp.md)
- [Rust guide](languages/rust.md)
- [Lua guide](languages/lua.md)

## Run checks

Basic forms:

```text
ctc
ctc src
ctc src\service.ts
ctc --rule no-value-imports
ctc --config strict.ctc.json
ctc --format json
ctc --color auto
ctc --color always
ctc --color never
```

- Positional paths limit the selected source set.
- `--rule` runs only named configured rules.
- `--config` picks another config file under the scan root.
- `--format` is `human` or `json`.
- `--color` affects human output only.

## Ad hoc templates

Use `--template` when you want a one-off check without `.ctc.json`.

```text
ctc src\service.ts --template .ctmpl\no-try.ts.ctmpl
ctc src\service.ts --template .ctmpl\no-try.ts.ctmpl --mode forbid
ctc src\service.ts --template .ctmpl\no-try.ts.ctmpl --mode forbid --scope descendants
ctc src\service.ts --template .ctmpl\no-try.ts.ctmpl --mode forbid --message "Do not catch exceptions here."
```

For CLI options, the scope names are kebab-case:

| CLI scope | Config scope |
|---|---|
| `top-level` | `topLevel` |
| `descendants` | `descendants` |
| `function-body` | `functionBody` |
| `class-body` | `classBody` |

## `validate-template`

Check one template file without a matching source file.

```text
ctc validate-template .ctmpl\class-factory.ts.ctmpl
```

Use this when you want to catch bad placeholder syntax, bad filters, or an
unsupported template position before you run a full check.

## `explain`

`explain` shows how one rule or template matched one source file.

```text
ctc explain src\service.ts --rule member-order
ctc explain src\service.ts --rule member-order --format json
ctc explain src\service.ts --template .ctmpl\member-order.ts.ctmpl --mode every --scope descendants
```

It is useful when an `every` or `exact` rule fails and you want to see where
the match stopped.

## `coverage`

`coverage` lists supported source files in the watched area that no rule
selects.

```text
ctc coverage
```

This helps you find files that sit under your watched directories but are not
covered by any rule.

## `guard`

`guard` compares the current working tree with a Git base reference and reports
changes that make the rule set weaker.

```text
ctc guard --base origin/main
ctc guard --base origin/main --allow-rule-changes
```

Run `ctc` as well. `guard` does not run normal rule matching.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | All selected checks passed |
| `1` | A rule mismatch, source parse error, invalid or refused ignore comment, uncovered file, or `guard` finding occurred |
| `2` | A command-line, configuration, template, or internal error occurred |

## Diagnostic codes

These codes come from [Section 41 of the specification](specification.md#41-diagnostic-codes).

### Command line

| Code | Meaning |
|---|---|
| `CTC0001` | Invalid command line |

### Input and configuration

| Code | Meaning |
|---|---|
| `CTC1001` | Invalid template filename |
| `CTC1003` | Configured rule selects no files |
| `CTC1005` | Invalid template directive |
| `CTC1006` | No selected source files |
| `CTC1007` | No applicable rule |
| `CTC1008` | Unsupported source type |
| `CTC1009` | Invalid UTF-8 |
| `CTC1010` | Invalid configuration |

### Template problems

| Code | Meaning |
|---|---|
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

### Template matching

| Code | Meaning |
|---|---|
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

### TypeScript semantic rules

| Code | Meaning |
|---|---|
| `CTC4001` | Function can complete without returning a value |
| `CTC4002` | Function has a return without a value |
| `CTC4101` | Forbidden try statement |
| `CTC4102` | Forbidden throw statement |
| `CTC4103` | Promise rejection |
| `CTC4104` | Configured exception-source call |

### Lua semantic rules

| Code | Meaning |
|---|---|
| `CTC4401` | Read of a global variable that the rule does not allow |
| `CTC4402` | Assignment to a global variable that the rule does not allow |
| `CTC4403` | Use of a restricted global name |
| `CTC4404` | `require` without one string literal as its argument |
| `CTC4405` | `_G` or `_ENV` indexed with a key that is not a string literal |

### Cross-file and size rules

| Code | Meaning |
|---|---|
| `CTC4201` | Missing partner file |
| `CTC4202` | Missing member function definition |
| `CTC4203` | Member function definition out of order |
| `CTC4301` | File has more lines than the rule allows |

### Suppression, coverage, and guard

| Code | Meaning |
|---|---|
| `CTC5001` | Invalid suppression comment |
| `CTC5002` | Suppression not allowed for this rule |
| `CTC5101` | Source file is in the watched area but no rule selects it |
| `CTC5201` | A suppression comment was added |
| `CTC5202` | A rule selects fewer files than at the base reference |
| `CTC5203` | A rule was removed or now allows suppression comments |
| `CTC5204` | A rule definition changed |

### Internal error

| Code | Meaning |
|---|---|
| `CTC9001` | Internal tool error |
