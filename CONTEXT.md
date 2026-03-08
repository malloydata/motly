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
  ast.rs           — AST types: ScalarValue, Statement, TagValue, ArrayElement, RefPathSegment, Span
  parser.rs        — Recursive descent parser, produces Vec<Statement> with source spans
  interpreter.rs   — Executes statements against a MOTLYNode tree (mutates in place), sets source locations
  tree.rs          — Output types: MOTLYNode (enum: Data|Ref), MOTLYDataNode, Scalar, EqValue, MOTLYLocation
  validate.rs      — Reference validation + schema validation
  error.rs         — MOTLYError with Position spans (line, column, offset)
  json.rs          — JSON serialization (compact, pretty, wire format with $date)
  from_json.rs     — JSON deserialization, wire format parsing
  lib.rs           — Public API: parse_motly(), WASM FFI session functions
  main.rs          — CLI: reads stdin, outputs JSON to stdout, errors to stderr
  tests.rs         — Shared fixture runners + implementation-specific tests

bindings/typescript/
  interface/           — "motly-ts-interface" package (shared types, private)
    src/
      types.ts         — TypeScript types (MOTLYNode, MOTLYDataNode, MOTLYRef, MOTLYLocation, MOTLYParseResult, etc.)

  parser/              — "@malloydata/motly-ts-parser" npm package (pure TypeScript)
    src/
      index.ts         — re-exports MOTLYSession + types
      session.ts       — MOTLYSession wrapping TS parser + interpreter + validator
      ast.ts           — TypeScript port of src/ast.rs
      parser.ts        — TypeScript port of src/parser.rs (~990 lines)
      interpreter.ts   — TypeScript port of src/interpreter.rs (~310 lines)
      validate.ts      — TypeScript port of src/validate.rs (~810 lines)
      mot.ts           — Mot API: resolved read-only view of MOTLY tree
      clone.ts         — Deep clone helpers for MOTLYNode trees
    test/test.ts       — fixture-driven tests + hand-written tests
    test/mot.test.ts   — Tests for Mot API

docs/
  language.md                — Complete MOTLY language reference with EBNF grammar
  schema_spec.md               — ALL-CAPS schema language specification (iteration 2)
  motly_schema.motly         — Self-validating meta-schema in the new format

test-data/
  fixtures/        — Shared JSON test fixtures (both implementations run these)
    parse.json         — 134 entries: parse input → expected value
    parse-errors.json  — 14 entries: parse input → expected errors
    schema.json        — 118 entries: schema + input → expected validation errors
    refs.json          — 15 entries: input → expected reference validation errors
    session.json       — 10 entries: multi-step session operations
  k8s-deployment-schema.motly  — Example: Kubernetes deployment schema
  k8s-deployment-sample.motly  — Example: Kubernetes deployment config
```

## The MOTLY Language

Full reference: `docs/language.md`. EBNF grammar is at the end of that file.

Every node has two independent slots: a **value** (scalar, array, env reference, or `@none`) and **properties** (a map of child nodes or link references). The three core operators each control a different combination:

- **`=`** — sets the value, never touches properties
- **`:`** — replaces properties, never touches the value
- **`:=`** — sets value AND replaces properties simultaneously

Merge (preserving existing properties) uses space-before-brace: `name { }`. See `docs/language.md` for the full assignment matrix, string types (bare, quoted, triple-quoted, heredoc `<<<...>>>`), `@none`, references, cloning with `:=`, env refs, and all other details.

## Parser Pipeline

Both implementations (Rust and pure TS) follow the same pipeline:

```
source text → Parser → Vec<Statement> → Interpreter → MOTLYNode tree
                                                            ↓
                                              Validator (schema + references)
```

1. **Parser** (`parser.rs` / `parser.ts`): recursive descent, produces a list of `Statement` AST nodes
2. **Interpreter** (`interpreter.rs` / `interpreter.ts`): executes statements against a `MOTLYNode`, handling merge/replace/delete semantics
3. **Validator** (`validate.rs` / `validate.ts`): optional schema validation and reference resolution

### Key types

**AST** (intermediate, not exposed):
- `Statement` — enum: `SetEq`, `AssignBoth`, `ReplaceProperties`, `UpdateProperties`, `Define`, `ClearAll`. Each variant carries a `Span` (begin/end source positions)
- `ScalarValue` — enum: `String`, `Number`, `Boolean`, `Date`, `Reference`, `None`, `Env`
- `TagValue` — scalar or array
- `ArrayElement` — value + optional properties + `Span`

**Output tree** (public API):
- `MOTLYNode` — the union type: either a `MOTLYDataNode` (concrete node) or a `Ref` (link reference). In Rust this is `enum MOTLYNode { Data(MOTLYDataNode), Ref { link_to, link_ups } }`. In TS it's `type MOTLYNode = MOTLYDataNode | MOTLYRef`.
- `MOTLYDataNode` — a concrete node with optional `eq` (scalar/array/env-ref), optional `properties` (map of `MOTLYNode`), optional `deleted` flag, optional `location` (source location from first appearance)
- Link references (`$ref`) are a `MOTLYNode` variant — they replace the entire node (no own eq or properties). `link_to` holds parsed path segments (names and indices), `link_ups` holds the number of `^` levels for relative refs (0 for absolute)
- Environment refs (`@env.NAME`) live in `eq` as `EqValue::EnvRef` (Rust) or `{ env }` (TS) — they are values, so a node can have an env ref AND properties

The interpreter mutates the `MOTLYNode` tree in place (does not return a new value).

### Source location tracking

Every `MOTLYNode` carries an optional `location: MOTLYLocation` recording where it was first defined. A location contains:
- `parseId` — which `parse()`/`parseSchema()` call produced this node (0-based, auto-incrementing per session)
- `begin` / `end` — `{ line, column, offset }` positions within that parse call's source text

**Semantics:**
- **First-appearance rule**: location is set when a node is first created and is NOT updated by subsequent modifications (value changes, property merges, etc.)
- **Exception — `:=` (assignBoth)**: since `:=` fully replaces a node, it always sets a new location
- **Exception — deletion (`-name`)**: always creates a fresh deleted node with a new location
- The `parseId` lets callers (e.g., Malloy) map MOTLY-relative positions back to their own source files

Schema validation errors and reference validation errors include the offending node's `location` when available.

### Property key ordering

Rust uses `BTreeMap` for properties (sorted keys). The pure TS implementation does **not** sort — properties are in insertion order. Test comparison is order-independent: the `deepEqual` helpers sort keys before comparing, and error list comparisons sort by (code, path).

## Schema Validation

Full spec: `docs/schema_spec.md`. Self-validating meta-schema: `docs/motly_schema.motly`.

The schema language uses ALL-CAPS directives to avoid namespace collisions with user-defined names:

- `REQUIRED { name = type }` — properties that must exist
- `OPTIONAL { name = type }` — properties that may exist
- `TYPES { TypeName { ... } }` — custom reusable types (root level only)
- `ADDITIONAL` / `ADDITIONAL = accept` / `ADDITIONAL = reject` / `ADDITIONAL = TypeName` — controls unknown properties
- `VALUE = primitive { refinements }` — constrains the value slot (string, number, integer, boolean, date)
- `ONEOF = [TypeA, TypeB]` — union types

Pre-loaded types: `string`, `number`, `integer`, `boolean`, `date`, `tag`, `flag`, `any`. User types cannot shadow these.

VALUE refinements: `ENUM` (all types), `MATCHES` (string), `MIN`/`MAX` (number, integer), `MIN_LENGTH`/`MAX_LENGTH` (string).

Property metadata: `EXCLUSIVE` (mutual exclusion groups), `REQUIRES` (sibling dependencies), `DEFAULT`, `DEPRECATED`, `DESCRIPTION`.

**IMPORTANT GOTCHA**: Array types MUST be quoted: `items = "string[]"`, `ports = "number[]"`. The brackets `[]` are not valid bare-string characters, so unquoted `string[]` causes a parse error.

**Implementation status**: TypeScript validator is complete (118 test fixtures passing). Rust schema validator is stubbed out (nop) — reference validation still works. See `docs/schema_spec.md` for the full spec.

Error codes: `missing-required`, `wrong-type`, `unknown-property`, `invalid-schema`, `invalid-enum-value`, `pattern-mismatch`, `out-of-range`, `length-violation`, `exclusive-violation`, `requires-violation`

## MOTLYSession API

```typescript
class MOTLYSession {
  parse(source: string): MOTLYParseResult;        // parse + apply to value, returns { parseId, errors }
  parseSchema(source: string): MOTLYParseResult;  // parse as schema, returns { parseId, errors }
  reset(): void;                                   // clear value, keep schema (parseId counter continues)
  getValue(): MOTLYDataNode;                       // deep clone of current value (includes locations)
  getMot<M extends Mot = Mot>(options?: GetMotOptions<M>): M;  // resolved read-only view; factory for custom Mot impls
  validateSchema(): MOTLYSchemaError[];            // validate value against schema
  validateReferences(): MOTLYValidationError[];    // check all $-references resolve
  dispose(): void;                                 // free resources / mark dead
}
```

**Breaking change**: `parse()` and `parseSchema()` now return `MOTLYParseResult` (`{ parseId: number, errors: MOTLYError[] }`) instead of `MOTLYError[]`. Callers must destructure: `const { parseId, errors } = session.parse(source)`.

After `dispose()`, all methods throw. `dispose()` itself is idempotent.

## Build & Test

### Rust
```sh
cargo test              # fixture runners + implementation-specific tests
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
npm test              # fixture-driven + hand-written tests via node --test
npm run pack          # produces @malloydata/motly-ts-parser tarball
```
Zero native dependencies. Uses Node.js built-in test runner (`node:test`).

Tests live in `test/*.ts` and are compiled by a separate `test/tsconfig.json` (CommonJS, `rootDir: "."`, `outDir: "../build-test"`). The test script builds both the library and tests, then runs `node --test build-test/*.js`. Test files import from the *built* library at `../build/parser/src/index` (relative to the test source). `MOTLYRef` has structured `linkTo` (array of path segments) and `linkUps` (number of `^` levels) — e.g. `$^parent.name` becomes `{ linkTo: ["parent", "name"], linkUps: 1 }`.

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

- **Value comparison**: Custom `deepEqual` that handles `Date` objects and compares object keys order-independently (sorts keys before comparing). The `location` field is excluded from fixture comparisons (TS `deepEqual` filters it out; Rust `strip_locations()` removes it before comparing).
- **Error list comparison**: Schema and refs fixture runners sort both actual and expected errors by `(code, path)` before comparing, so error emission order doesn't matter. This is consistent across both implementations.
- **Location tests**: Dedicated `describe("Location tracking")` suite (TS) and `test_loc_*` tests (Rust) verify location semantics separately from fixtures: first-appearance rule, `:=` override, deletion, intermediate path nodes, multi-parse IDs, clone independence, etc.

## Release Process

1. Make sure all work is committed and pushed on `main`
2. Run `./scripts/release.sh [patch|minor|major]` (default: `patch`)
   - Preflight: checks clean tree, on `main`, in sync with remote, no tag collision
   - Bumps version in `bindings/typescript/parser/package.json` and `Cargo.toml`
   - Runs all tests (Rust + TS) — auto-reverts version files if tests fail
   - Commits `vX.Y.Z`, tags, pushes — auto-reverts commit+tag if push fails
3. Go to GitHub Actions, trigger the **"Publish to npm"** workflow (`workflow_dispatch`)
   - Tests again on CI, then publishes `@malloydata/motly-ts-parser` to npm
   - npm secrets live on GitHub, not locally

The Rust crate is not published to crates.io (yet). Tags are the version history; there are no GitHub Releases.

## Common Pitfalls

1. **Array types must be quoted in schemas**: `items = "string[]"` not `items = string[]`
2. **`@` starts special values**: `@true`, `@false`, `@none`, `@2024-...` — values containing `@` must be quoted
3. **Three operators**: `=` (value only), `:` (properties only), `:=` (both). Space-before-brace merges, `:` replaces.
4. **Bare strings**: Tokens like `v2` (digit after letter) are bare strings, not numbers. Only pure digit sequences (optionally with `.`, `e`, `-`) parse as numbers.
5. **`file:` deps and Babel**: `file:` dependencies in `package.json` create symlinks. Babel resolves the real path and processes files it shouldn't, causing `@babel/runtime` errors. Fix: use `npm pack` tarball for local testing in downstream projects.
