# CONTEXT.md — LLM Onboarding for MOTLY

This document contains everything you need to work effectively on this repository.

## What This Repo Is

This is the Rust implementation of the MOTLY parser. MOTLY is a lightweight configuration language from [Malloy](https://github.com/malloydata/malloy). The goal is to eventually hot-swap between TS and Rust implementations in Malloy.

The repo produces three things:
1. **A Rust library and CLI** — parses MOTLY, validates against schemas, outputs JSON
2. **`motly-ts-interface` package** (`bindings/typescript/interface/`) — shared TypeScript types (private, not published)
3. **`@malloydata/motly-ts-parser` npm package** (`bindings/typescript/parser/`) — pure TypeScript reimplementation of the parser, zero native deps

Both the Rust library and the TS parser expose an identical `MOTLYSession` API. Consumers pick one at build time. A WASM backend will return in a future release (see `VISION.md`).

## Repository Structure

```
src/
  ast.rs           — AST types: ScalarValue, Statement, TagValue, ArrayElement, RefPathSegment
  parser.rs        — Recursive descent parser (~1050 lines), produces Vec<Statement>
  interpreter.rs   — Executes statements against a MOTLYValue tree (~300 lines)
  tree.rs          — Output types: MOTLYValue, MOTLYNode (= MOTLYValue | MOTLYLink), Scalar, EqValue
  validate.rs      — Reference validation + schema validation (~940 lines)
  error.rs         — MOTLYError with Position spans (line, column, offset)
  json.rs          — JSON serialization (compact, pretty, wire format with $date)
  from_json.rs     — JSON deserialization, wire format parsing
  lib.rs           — Public API: parse_motly(), MotlySession, WASM FFI functions
  main.rs          — CLI: reads stdin, outputs JSON to stdout, errors to stderr
  tests.rs         — 5 shared fixture runners + 22 implementation-specific tests (~550 lines)

bindings/typescript/
  interface/           — "motly-ts-interface" package (shared types, private)
    src/
      types.ts         — TypeScript types (MOTLYValue, MOTLYNode, MOTLYRef, etc.)

  parser/              — "@malloydata/motly-ts-parser" npm package (pure TypeScript)
    src/
      index.ts         — re-exports MOTLYSession + types
      session.ts       — MOTLYSession wrapping TS parser + interpreter + validator
      ast.ts           — TypeScript port of src/ast.rs
      parser.ts        — TypeScript port of src/parser.rs (~990 lines)
      interpreter.ts   — TypeScript port of src/interpreter.rs (~310 lines)
      validate.ts      — TypeScript port of src/validate.rs (~740 lines)
    test/test.ts       — 210 tests (hand-written + shared fixtures)

docs/
  language.md      — Complete MOTLY language reference with EBNF grammar
  schema.md        — Schema validation reference

test-data/
  fixtures/        — Shared JSON test fixtures (both implementations run these)
    parse.json         — 103 entries: parse input → expected value
    parse-errors.json  — 13 entries: parse input → expected errors
    schema.json        — ~62 entries: schema + input → expected validation errors
    refs.json          — 15 entries: input → expected reference validation errors
    session.json       — 10 entries: multi-step session operations
  motly-schema.motly       — MOTLY meta-schema (schema that validates schemas)
  k8s-deployment-schema.motly  — Example: Kubernetes deployment schema
  k8s-deployment-sample.motly  — Example: Kubernetes deployment config
```

## The MOTLY Language

Full reference: `docs/language.md`. EBNF grammar is at the bottom of that file.

### Key syntax

- **Bare strings**: `name = hello` — no quotes needed for `[A-Za-z0-9_]` + extended Latin
- **Quoted strings**: `"double"` (escapes), `'single'` (raw), `"""triple"""`, `'''triple raw'''`
- **Backtick identifiers**: `` `content-type` = json `` — for property names with special chars
- **Numbers**: `42`, `-3.14`, `.5`, `1.5e10` — tokens starting with digits but continuing with letters are bare strings (e.g., `v2`)
- **Booleans**: `@true`, `@false` — the `@` prefix prevents ambiguity with bare strings
- **Dates**: `@2024-01-15`, `@2024-01-15T10:30:00Z` — ISO 8601 with `@` prefix
- **Arrays**: `[a, b, c]` — trailing comma allowed, elements can have properties
- **References**: `$root.path` (absolute), `$^sibling` (up one), `$^^grandparent` (up two), `$arr[0]` (indexed)
- **Flags**: bare name with no value creates a presence-only node: `hidden`
- **Deletion**: `-name` deletes a property, `-...` clears all properties in scope
- **Comments**: `# line comment`

### Replace vs Merge (critical concept)

- **Colon** `name: { props }` or `name = { props }` — **replaces** all existing properties
- **Space** `name { props }` — **merges** with existing properties
- **Preserve value** `name = ... { new_props }` — replaces properties, keeps scalar value
- **Preserve properties** `name = new_val { ... }` — changes scalar value, keeps existing properties

### Dot notation

`database.connection.pool.max = 100` creates nested objects.

## Parser Pipeline

Both implementations (Rust and pure TS) follow the same pipeline:

```
source text → Parser → Vec<Statement> → Interpreter → MOTLYValue tree
                                                            ↓
                                              Validator (schema + references)
```

1. **Parser** (`parser.rs` / `parser.ts`): recursive descent, produces a list of `Statement` AST nodes
2. **Interpreter** (`interpreter.rs` / `interpreter.ts`): executes statements against a `MOTLYValue`, handling merge/replace/delete semantics
3. **Validator** (`validate.rs` / `validate.ts`): optional schema validation and reference resolution

### Key types

**AST** (intermediate, not exposed):
- `Statement` — enum: `SetEq`, `ReplaceProperties`, `UpdateProperties`, `Define`, `ClearAll`
- `ScalarValue` — enum: `String`, `Number`, `Boolean`, `Date`, `Reference`
- `TagValue` — scalar or array
- `ArrayElement` — value + optional properties

**Output tree** (public API):
- `MOTLYValue` — has optional `eq` (scalar/array), optional `properties` (map of children), optional `deleted` flag
- `MOTLYNode` = `MOTLYValue | MOTLYLink` (Rust) / `MOTLYValue | MOTLYRef` (TS)
- `MOTLYLink`/`MOTLYRef` — holds a `linkTo` string like `$^sibling`

### Property key ordering

Rust uses `BTreeMap` for properties (sorted keys). The pure TS implementation does **not** sort — properties are in insertion order. Test comparison is order-independent: the `deepEqual` helpers sort keys before comparing, and error list comparisons sort by (code, path).

## Schema Validation

Full reference: `docs/schema.md`.

Schemas are themselves MOTLY files with three sections:
- `Required { name = type }` — properties that must exist
- `Optional { name = type }` — properties that may exist
- `Types { TypeName { ... } }` — custom reusable types (root level only)
- `Additional` / `Additional = allow` / `Additional = TypeName` — controls unknown properties

Built-in types: `string`, `number`, `boolean`, `date`, `tag`, `flag`, `any`

**IMPORTANT GOTCHA**: Array types MUST be quoted: `items = "string[]"`, `ports = "number[]"`. The brackets `[]` are not valid bare-string characters, so unquoted `string[]` causes a parse error.

Other schema features: enum values (`eq = [red, green, blue]`), pattern matching (`matches = "^regex$"`), union types (`oneOf = [string, number]`), nested schemas, custom type arrays (`"TypeName[]"`), recursive types.

Error codes: `missing-required`, `wrong-type`, `unknown-property`, `invalid-schema`, `invalid-enum-value`, `pattern-mismatch`

## MOTLYSession API

```typescript
class MOTLYSession {
  parse(source: string): MOTLYError[];           // parse + apply to value
  parseSchema(source: string): MOTLYError[];     // parse as schema
  reset(): void;                                  // clear value, keep schema
  getValue(): MOTLYValue;                         // deep clone of current value
  validateSchema(): MOTLYSchemaError[];           // validate value against schema
  validateReferences(): MOTLYValidationError[];   // check all $-references resolve
  dispose(): void;                                // free resources / mark dead
}
```

After `dispose()`, all methods throw. `dispose()` itself is idempotent.

## Build & Test

### Rust
```sh
cargo test              # 39 tests (5 fixture runners + 22 impl-specific + 12 from_json)
cargo build --release   # library + CLI binary
echo 'name = hello' | cargo run   # CLI usage
```

### Interface package (`bindings/typescript/interface/`)
```sh
cd bindings/typescript/interface
npm install
npm run build         # tsc — must be built before the parser package
```

### Parser package (`bindings/typescript/parser/`)
```sh
cd bindings/typescript/parser
npm install
npm run build         # tsc
npm test              # 210 tests via node --test
npm run pack          # produces @malloydata/motly-ts-parser tarball
```
Zero native dependencies. Uses Node.js built-in test runner (`node:test`).

## Shared Test Fixtures

The shared fixtures in `test-data/fixtures/` are the single source of truth for MOTLY language behavior. Both implementations run them:
- **Rust** — `src/tests.rs` embeds fixtures with `include_str!`, parses with `serde_json`, converts expected values with `from_json::from_wire()`
- **Pure TS** — `test/test.ts` loads fixtures from disk, hydrates `{$date}` to `Date` objects

Fixture files are JSON arrays. Dates use `{"$date": "..."}` convention.

Format summary:
- **parse.json**: `{name, input: string|string[], expected?, expectErrors?}`
- **parse-errors.json**: `{name, input, expectErrors: true}`
- **schema.json**: `{name, schema, input, expectedErrors: [{code, path?}]}`
- **refs.json**: `{name, input, expectedErrors: [{code, path?}]}`
- **session.json**: `{name, steps: [{action, input?, expected?, expectedErrors?}]}`

When `input` is a `string[]`, each element is a separate `parse()` call (tests accumulation).

### Test comparison strategy

- **Value comparison**: Custom `deepEqual` that handles `Date` objects and compares object keys order-independently (sorts keys before comparing).
- **Error list comparison**: Schema and refs fixture runners sort both actual and expected errors by `(code, path)` before comparing, so error emission order doesn't matter. This is consistent across both implementations.

## Common Pitfalls

1. **Array types must be quoted in schemas**: `items = "string[]"` not `items = string[]`
2. **`@` starts special values**: `@true`, `@false`, `@2024-...` — values containing `@` must be quoted
3. **Replace vs merge**: `:` replaces, space merges — this is the most common source of confusion in the language
4. **Bare strings**: Tokens like `v2` (digit after letter) are bare strings, not numbers. Only pure digit sequences (optionally with `.`, `e`, `-`) parse as numbers.
5. **`file:` deps and Babel**: `file:` dependencies in `package.json` create symlinks. Babel resolves the real path and processes files it shouldn't, causing `@babel/runtime` errors. Fix: use `npm pack` tarball for local testing in downstream projects.
