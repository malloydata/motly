# VISION.md — Future Plans for MOTLY

This document captures the long-term direction for MOTLY beyond the initial 0.0.1 parser release.

## The Big Idea: Schema-Driven DOM Bindings

The end goal is an ORM-like experience: you define a MOTLY schema, and get back a typed, language-native DOM with getters, setters, and validation — all backed by serializable MOTLY data.

Instead of `getValue()` returning a plain object that consumers wrap in their own DOM layer (which is what Malloy does today), MOTLYNode *becomes* the API. Consumers get typed accessors, transparent reference resolution, and mutation methods directly on the node tree.

This is a breaking change from the current parse-only API, which is why we're shipping 0.0.1 first to validate usefulness.

## DOM API

### MOTLYNode Interface

Typed accessors (all take `...path: PathSegment[]`):
- `.text()`, `.number()`, `.boolean()`, `.date()`

Array accessors:
- `.array()`, `.textArray()`, `.numericArray()`

Structure:
- `.node()`, `.has()`, `.bare()`, `.keys()`, `.entries()`

Ref inspection:
- `.isRef`, `.refTarget`

Serialization:
- `.prefix`

Content replacement:
- `.innerMOTLY` (write-only setter)

### MOTLYSession Additions

- `root: MOTLYNode` — DOM access
- `setEq(path, value)`, `setProperty(path)`, `deleteProperty(path)` — permissive mutations (always apply, validate on demand)
- `validateReferences()` — separate from schema validation
- `snapshot()` / `restore(snapshot)` — rollback support
- `serialize()` / `serializeAt(path)` — MOTLY source output

### Types

- `MOTLYScalar = string | number | boolean | Date`
- `PathSegment = string | number`
- `Path = PathSegment[]`

### Key Decisions

- Mutations are permissive (always apply), validate on demand
- Refs resolve transparently but are inspectable
- Path varargs for reads, array for mutations

### Open Questions

1. `getValue()` survival — keep for plain object snapshots?
2. Node identity — same object or fresh wrapper on repeated access?
3. Array mutation — `setEq` with array? `appendToArray`? or just innerMOTLY/parse?
4. Integer vs float in schemas
5. `innerMOTLY` vs `parseAt` — is replace sufficient or also need merge?
6. Snapshot representation — opaque handle vs serialized string

### Implementation Roadmap

1. Lock down interface in `motly-ts-interface`
2. Implement TS DOM in `motly-ts-interface` (shared by both TS packages)
3. Add shared DOM test fixtures in `test-data/fixtures/`
4. Rewrite pure TS interpreter to produce DOM nodes directly
5. Update WASM package to construct DOM from Rust output
6. Implement Rust DOM (for Python/Ruby/Go bindings)
7. Add serializer (TS first, then Rust)
8. Add snapshot/restore
9. MOTLYValue removed from public API

## Schema Metadata

### Goal

Extend MOTLY schemas to carry UI metadata (labels, placeholders, secret flags, file pickers) for driving dynamic UI generation — for example, connection editor panels.

### Key Observations

- **The meta-schema is too loose**: `PropDefFull` and `StructuralType` have bare `Additional`, which accepts any garbage in schema files. Tested: tightening to `Additional = tag` as Optional property self-validates and correctly rejects garbage.
- **Built-in types ignore properties**: `string` type validator only checks eq value, never looks at properties. So `PropDef.oneOf = [string, PropDefFull]` already allows inline metadata on field definitions for free — no validator changes needed. Adding `string` to `TypeDef.oneOf` would extend this to type aliases.
- **Can't validate metadata with current schema language**: structural properties on a type spec describe the *data*, not the type. Can't combine "eq must be string" with "these optional metadata properties are allowed". A separate mechanism is needed.

### Design Direction: `Meta:` Section

A `Meta:` section in schemas, parallel to `Types:`, keyed by type names:

```motly
Types: {
  secretString = string
  filePath = string
}
Meta: {
  secretString: { secret }
  filePath: { filePicker }
}
Optional: {
  databasePath = filePath { label="Database File" placeholder=":memory:" }
  motherDuckToken = secretString { label="MotherDuck Token" }
}
```

Type-level metadata (secret, filePicker) lives in the Meta section. Field-level metadata (label, placeholder) goes inline on field definitions. An extraction API merges both.

### Open Questions

- Should Meta contain only type names, or also field names?
- How should the extraction API merge type meta + field meta?
- Should the meta-schema be tightened now or later?
- What about the `Type` property in PropDefFull (defined in meta-schema but unused by validator)?

### Implementation (once design settles)

- Changes in both Rust (`src/validate.rs`) and pure TS (`bindings/typescript/parser/src/validate.ts`)
- Meta-schema update (`test-data/motly-schema.motly`)
- New metadata extraction API
- Test fixtures for metadata scenarios

## WASM Backend

The WASM package (`bindings/typescript/wasm/`) has been removed for the 0.0.1 release. It will return as an alternative backend behind the same `motly-ts-interface` types, giving consumers a drop-in performance upgrade with no API changes.

The Rust side already has the FFI layer (`src/lib.rs` exposes `wasm_session_*` functions). Reintroduction is a packaging and CI task, not a design task.

## Multi-Language Bindings

Once the Rust DOM is implemented (step 6 in the DOM roadmap), it can be exposed to Python, Ruby, Go, and other languages through their respective FFI mechanisms. The Rust core becomes the shared engine; each language gets native-feeling bindings.

## Dual Implementation Strategy

The parser and interpreter exist in both Rust and pure TypeScript. Both implementations must stay in sync:

- `src/parser.rs` ↔ `bindings/typescript/parser/src/parser.ts`
- `src/interpreter.rs` ↔ `bindings/typescript/parser/src/interpreter.ts`
- `src/validate.rs` ↔ `bindings/typescript/parser/src/validate.ts`

Shared test fixtures in `test-data/fixtures/` are the single source of truth for correctness. Any feature or fix applied to one implementation must be applied to the other.
