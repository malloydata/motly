# VISION.md — Future Plans for MOTLY

This document captures the long-term direction for MOTLY beyond the initial 0.0.1 parser release.

## The Big Idea: Schema-Driven DOM Bindings

The end goal is an ORM-like experience: you define a MOTLY schema, and get back a typed, language-native DOM with getters, setters, and validation — all backed by serializable MOTLY data.

Instead of `getValue()` returning a plain object that consumers wrap in their own DOM layer (which is what Malloy does today), MOTLYNode *becomes* the API. Consumers get typed accessors, transparent reference resolution, and mutation methods directly on the node tree.

This is a breaking change from the current parse-only API, which is why we're shipping 0.0.1 first to validate usefulness.

## Mot: The Read API

The first step toward a full DOM is the read-only `Mot` interface — a resolved,
typed, navigable view of parsed MOTLY data. Refs followed, env vars substituted,
deletions consumed. The consumer sees clean data; the internal model retains
provenance for future mutation support.

**Status**: TypeScript implementation complete (`bindings/typescript/parser/src/mot.ts`).
`session.getMot({ env })` returns a `Mot`. Replaces the old untyped `resolve()`
that returned plain JS objects.

Cross-language API design notes:
- **Rust**: [`docs/mot-api-rust.md`](docs/mot-api-rust.md) — `Option<&Mot>` instead of Null Object pattern; `get()` + `get_path()` instead of variadic; arena allocation for circular refs
- **Python**: [`docs/mot-api-python.md`](docs/mot-api-python.md) — Null Object pattern translates directly; dunder support (`__getitem__`, `__contains__`, `__bool__`) for Pythonic usage

### Future: Mutation and Serialization

The Mot read API is the foundation for a richer DOM:

- `setEq(path, value)`, `setProperty(path)`, `deleteProperty(path)` — permissive mutations (always apply, validate on demand)
- `snapshot()` / `restore(snapshot)` — rollback support
- `serialize()` / `serializeAt(path)` — MOTLY source output

### Open Questions

1. `getValue()` survival — keep for plain object snapshots?
2. Node identity — same object or fresh wrapper on repeated access?
3. Array mutation — `setEq` with array? `appendToArray`? or just innerMOTLY/parse?
4. Snapshot representation — opaque handle vs serialized string

## Schema Metadata

### Goal

Extend MOTLY schemas to carry UI metadata (labels, placeholders, secret flags, file pickers) for driving dynamic UI generation — for example, connection editor panels.

### Current State

The new ALL-CAPS schema language (see `docs/schema_spec.md`) already provides property-level metadata directives: `DESCRIPTION`, `DEFAULT`, and `DEPRECATED`. The meta-schema (`docs/motly_schema.motly`) self-validates, confirming the design is sound.

### Key Observations

- **DESCRIPTION already works**: `DESCRIPTION = "Hostname or IP address"` on property definitions provides documentation that tools can surface as tooltips.
- **DEFAULT already works**: `DEFAULT = 30` on optional properties provides fallback values for the DOM API.
- **DEPRECATED already works**: bare `DEPRECATED` or `DEPRECATED = "Use X instead"` for migration guidance.
- **UI-specific metadata (secret, filePicker, label, placeholder) is not yet in the spec.** The question is whether to add more directives, use a `META` section, or put UI hints in property metadata alongside DESCRIPTION.

### Design Direction

The new schema language's inline metadata on property definitions may be sufficient:

```motly
TYPES {
  SecretString { VALUE = string }
  FilePath { VALUE = string }
}
OPTIONAL {
  databasePath = FilePath { DESCRIPTION = "Database file path" DEFAULT = ":memory:" }
  motherDuckToken = SecretString { DESCRIPTION = "MotherDuck Token" DEPRECATED }
}
```

For UI-specific hints (secret flag, file picker), options include:
1. **New directives**: `SECRET`, `FILE_PICKER` etc. — simple but grows the directive set
2. **Generic metadata**: A `META` property on definitions that accepts arbitrary tags — flexible but unvalidated
3. **Application-level types**: Applications define types like `SecretString` and use them as signals — no schema language changes needed

### Open Questions

- Which approach for UI metadata? Dedicated directives vs generic META vs type-name conventions?
- Should the extraction API merge type-level and property-level metadata?
- How does metadata interact with the Mot read API? (e.g., `mot.description("field")`)

### Implementation (once design settles)

- Meta-schema update (`docs/motly_schema.motly`)
- Metadata extraction API on MOTLYSession or Mot
- Test fixtures for metadata scenarios

## WASM Backend

The WASM package (`bindings/typescript/wasm/`) has been removed for the 0.0.1 release. It will return as an alternative backend behind the same `motly-ts-interface` types, giving consumers a drop-in performance upgrade with no API changes.

The Rust side already has the FFI layer (`src/lib.rs` exposes `wasm_session_*` functions). Reintroduction is a packaging and CI task, not a design task.

## Multi-Language Strategy

There are two paths for supporting new languages: **FFI bindings** (wrap the
Rust implementation via PyO3, CGo, etc.) or **native implementations** (rewrite
the parser in each language, validated against the shared test fixtures).

### FFI Bindings (Rust as shared engine)

- One implementation to maintain
- But: build complexity (manylinux wheels, cross-compilation), debugging across
  FFI boundaries, platform matrix headaches, native dep install friction

### Native Implementations (shared test suite as spec)

This is already the model for TypeScript — the pure TS parser is an independent
implementation validated by the same fixtures in `test-data/fixtures/`. It has
zero native dependencies and installs everywhere.

Advantages of extending this to Python, Go, etc.:
- **Zero native deps** — `pip install` / `go get` just works, no compilation
- **Debuggable** — users step through code in their own language
- **Contributable** — Python devs fix Python bugs without knowing Rust
- **AI-assisted porting** — given fixtures + a reference implementation,
  generating a new port is a well-defined task with a clear done-state

The cost model has shifted: maintaining N implementations used to be prohibitive,
but with a solid test suite and AI assistance, the marginal cost of a new port
is lower than the ongoing tax of FFI bindings.

### The tradeoff: language changes become expensive

Every language feature addition or behavioral change must be applied to every
implementation. The shared fixtures catch regressions, but the implementation
work scales with the number of ports. This is manageable with two implementations
(Rust + TS) but gets heavy at four or five.

### Recommendation

- **TypeScript**: native (already done, zero-dep npm is a hard requirement)
- **Python**: native (pip install simplicity matters; parser is ~1300 lines)
- **Go**: native (Go developers strongly prefer pure Go)
- **Rust**: the reference implementation
- **WASM**: compile from Rust (for environments that need it)

Use differential fuzzing (see below) to keep implementations in sync.
Fixtures are the spec; implementations are commodities.

## Implementation Sync

All implementations must produce identical results for the shared test fixtures
in `test-data/fixtures/`. The fixtures are the specification; the implementations
are interchangeable.

Current implementations:
- `src/parser.rs` ↔ `bindings/typescript/parser/src/parser.ts`
- `src/interpreter.rs` ↔ `bindings/typescript/parser/src/interpreter.ts`
- `src/validate.rs` ↔ `bindings/typescript/parser/src/validate.ts`

Any language feature or behavioral change must update the fixtures first, then
each implementation. As the number of ports grows, the fixtures become
increasingly important as the single source of truth.

### Future: Differential Fuzzing

The hand-written test fixtures verify known behaviors but can't guarantee completeness. As the two implementations evolve independently, the risk of subtle drift grows — edge cases where one parser accepts input the other rejects, or where they produce structurally different ASTs.

**Differential fuzzing** addresses this directly: generate random MOTLY inputs, feed them to both parsers, and compare outputs. No expected values needed — the two implementations just have to agree.

The plan:

1. **Grammar-aware fuzzer** — walks the EBNF and makes random choices at each production (with depth/size limits to keep inputs reasonable). This hits the weird corners humans forget: nested triple-quoted strings inside arrays, heredocs with unusual indentation, numbers that almost look like bare strings, references with deep `^` chains, etc.

2. **Differential test harness** — runs both parsers on each generated input and does a structural comparison. For each input, either both parsers succeed and produce equivalent ASTs, or both parsers fail. Any disagreement is a bug.

3. **CI integration** — run as a periodic sweep (e.g., 10,000 random inputs per run). The longer it runs, the more confidence accumulates. Failures get distilled into minimal reproducing cases and added to the shared fixture files as regression tests.
