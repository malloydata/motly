# Interpreter Architecture

The MOTLY interpreter transforms parsed AST statements into mutations on a
`MOTLYDataNode` tree. Both the Rust (`src/interpreter.rs`) and TypeScript
(`bindings/typescript/parser/src/interpreter.ts`) implementations use the
same four-phase architecture.

## Pipeline

```
source text → Parser → Statement[] → Interpreter → MOTLYDataNode tree
                                      │
                                      ├─ Phase 1: flatten()
                                      ├─ Phase 2: chunk()
                                      ├─ Phase 3: topoSort()
                                      └─ Phase 4: executeChunked()
```

## Phase 1 — Flatten

Walk the AST depth-first, accumulating the current path. Each leaf
statement produces a **transformer** — a `(path, operation)` pair with
the fully resolved absolute path.

```
source:  a { b = 1  c { d = 2 } }

transformers:
  0: (["a"],           define)
  1: (["a","b"],       setValue 1)
  2: (["a","c"],       define)
  3: (["a","c","d"],   setValue 2)
```

Pure function — no tree mutation.

### Transformer

```
Transformer {
  path: string[]          // absolute path in the tree
  op: TransformerOp       // what to do
  span: Span              // source position (begin/end)
  parseId: number         // which parse() call produced this
}
```

### TransformerOp variants

| Op | Description |
|----|-------------|
| `setValue` | Set value, preserve existing properties. First-appearance location. |
| `assignValue` | Create fresh node with new value and location. No carry-over. |
| `clearProperties` | Delete properties, preserve value and location. |
| `clearAll` | Clear both value and properties (`***` statement). |
| `define` | Get-or-create a node (no-op if it exists). |
| `delete` | Create a deleted-marker node with new location. |
| `link` | Insert a read-only reference (`= $ref`). |
| `clone` | Deep-copy a reference target (`:= $ref`). |

### AST → Transformer mapping

| AST Statement | Emitted Transformers |
|---|---|
| `setEq` (literal) | `setValue` at path, then flatten children |
| `setEq` (reference) | `link` at path (children dropped — refs can't have properties) |
| `assignBoth` (literal) | `assignValue` at path, then flatten children |
| `assignBoth` (ref, no props) | `clone` at path |
| `assignBoth` (ref, with props) | `clone` at path, `clearProperties` at path, then flatten children |
| `replaceProperties` (`:`) | `clearProperties` at path, then flatten children |
| `updateProperties` (merge) | `define` at path, then flatten children |
| `define` | `define` at path |
| `define` (deleted) | `delete` at path |
| `clearAll` (`***`) | `clearAll` at enclosing path |

## Phase 2 — Chunk

Scan transformers linearly, maintaining a set of paths written so far.
When a link or clone references a path not yet written (a forward
reference), make three cuts:

1. Before the reference — ends the current chunk
2. After the reference — isolates it as a singleton chunk
3. At the last write to the target path — separates target writes

Forward references are detected by checking whether the target path or
any descendant has been written (ancestor writes do not count — they
don't guarantee the specific target exists).

A dependency graph is built alongside the splits:

- **Reference edges**: ref chunk depends on chunk containing the last
  write to the target — scanning exact matches, descendants, AND
  ancestors (an ancestor delete/replace affects the target)
- **Write-after-reference edges**: chunks writing to a ref's output path
  (after it in source order) depend on the ref chunk

Returns chunk boundary indices (`splits`) and a dependency adjacency
list (`deps`).

### Path serialization

Paths are serialized with `\0` as the segment separator (not `.`)
because property names may contain dots via backtick-quoted identifiers
(e.g., `` `a.b` ``). All prefix checks use `\0` consistently.

### Worked example

```motly
a = 1
x = $y         # forward ref — y not yet defined
x.z = 1
b = 2
y { z = 0 }
c = 3
```

```
Transformers:
0: (["a"],      setValue 1)
1: (["x"],      link $y)      ← forward ref
2: (["x","z"],  setValue 1)
3: (["b"],      setValue 2)
4: (["y"],      define)        ← from updateProperties
5: (["y","z"],  setValue 0)    ← last write to y subtree
6: (["c"],      setValue 3)

Cuts at: 1, after 1, and 5.

Chunks:
  A: [0]           a = 1
  B: [1]           x = $y        (singleton)
  C: [2, 3, 4]     x.z = 1, b = 2, y (define)
  D: [5, 6]        y.z = 0, c = 3

Dependency graph:
  A (no deps)    D (no deps)
                   ↓
                 B (depends on D)
                   ↓
                 C (depends on B)

Topo-sort: [A, D, B, C]  (A and D are independent, A first by source order)

Execution:
  1. Chunk A:  a = 1
  2. Chunk D:  y.z = 0, c = 3
  3. Chunk B:  x = $y  →  x = { z: 0 }
  4. Chunk C:  x.z = 1  →  x = { z: 1 }, b = 2
```

Result: `x.z = 1`. Source-order write semantics preserved.

### Why singleton chunks

The reference must be isolated because grouping it with neighboring
statements creates false dependencies. Consider:

```motly
x = $y
b = 2
y { z = 0 }
c = $b
```

If we naively split into two chunks `[x=$y, b=2]` and `[y.z=0, c=$b]`:
- First chunk depends on second (forward ref to `$y`)
- Second chunk depends on first (`$b` defined there)
- **False cycle** — but there is no real circular dependency

With singleton isolation: `[x=$y]`, `[b=2]`, `[y.z=0]`, `[c=$b]`. The
dependency graph is acyclic and sorts cleanly.

### Common case performance

Most files have no forward references — produces a single chunk that
executes linearly with zero graph overhead. Files with one or two forward
references produce a handful of chunks. The graph is only as large as
the number of forward references requires.

## Phase 3 — Sort

Topological sort (Kahn's algorithm) on the dependency graph. **Must use a
FIFO queue** so independent chunks preserve source order.

Returns execution order and any cycle members. In practice, topo-sort
cycles are rare — the chunker adds all transformer paths (including
refs) to the written set, so most mutual references have one direction
seen as backward. Clone cycles are instead detected post-execution
(see Phase 4). Link cycles are caught by reference validation.

## Phase 4 — Execute

Apply chunks in sorted order. Within each chunk, apply transformers
linearly. Each operation receives the tree built by all previous
operations.

### Operation semantics

**setValue(path, value)**
Navigate to path (creating intermediate nodes as needed). If the target
node doesn't exist, create it. Set the value slot. Preserve existing
properties. Set location only on first appearance.

**assignValue(path, value)**
Navigate to path. Create a fresh node (discarding any existing node at
that path). Set value and location. Used by `:=` with literal values.

**clearProperties(path)**
Navigate to path. Delete the node's properties map, preserving its value
and location. If the node is a ref or doesn't exist, replace with a fresh
empty node.

**clearAll(path)**
Navigate to path. Clear both value and properties. Sets
`properties: {}` (not deleted) to match `***` statement semantics.

**define(path)**
Navigate to path. If no node exists, create an empty one with location.
No-op if the node already exists. Used by `updateProperties` to ensure
the node is created even for empty blocks (`flag { }`).

**delete(path)**
Navigate to path. Replace with a deleted-marker node (`deleted: true`)
with a new location. Always replaces regardless of existing state.

**link(path, ups, refPath)**
Navigate to path. Place a reference node (`MOTLYRef` / `MOTLYNode::Ref`).
If `disableReferences` is set, emit a `ref-not-allowed` error but still
create the ref (diagnostic only, not enforcement).

**clone(path, ups, refPath)**
1. Build intermediate nodes along path (so relative refs can navigate)
2. Resolve the reference target and deep-copy it
3. Sanitize relative refs in the clone (error if they escape the subtree)
4. Set location on the clone
5. Place the clone at path

If resolution fails, record it as a failed clone. After all chunks execute,
a post-pass checks for circular clone dependencies (A clones B, B clones A)
and replaces individual errors with a single `circular-reference` error.

### Array element properties

Array elements with properties (e.g. `items = [{name=x port=80}]`) use a
legacy recursive executor. Array elements are self-contained — they
cannot contain forward references — so the four-phase engine is not needed.

### Path navigation

`buildAccessPath` (TS) / `build_access_path` (Rust) navigates to the
parent of the write target, creating intermediate `MOTLYDataNode`
entries along the way. If any segment along the path is a link
reference, it emits a `write-through-link` error and aborts. This is
what enforces the read-only semantics of links.

### Why links are read-only

The dependency graph is built from static path strings gathered during
flattening. If links were writable, a write to `alias.prop` would
secretly mutate `target.prop` — but the static analyzer only sees the
string `"alias.prop"`, not the resolved pointer. The dependency graph
would miss the mutation entirely, causing the topological sort to
produce an incorrect execution order.

By enforcing read-only links, every mutation path in the flattened
transformer list is exactly the path that gets mutated in the tree. The
static analyzer can trust the strings completely, and the four-phase
engine works without runtime alias resolution during graph construction.

### Clone boundary rule

When `:= $ref` copies a subtree, relative references within it must
resolve within the subtree. A reference that escapes the clone boundary
produces a `clone-reference-out-of-scope` error.

```motly
# OK — resolves within the subtree
base: {
  shared = "db.internal"
  primary: { host = $^.shared }
}
copy := $base

# Error — escapes the subtree
root_val = important
other: { val = $^^.root_val }
copy := $other   # error: $^^.root_val resolves outside
```

## Error codes

| Code | When |
|------|------|
| `ref-not-allowed` | `disableReferences` is set and a link ref is used |
| `ref-with-properties` | `= $ref { ... }` — refs can't have properties (use `:=`) |
| `write-through-link` | Writing into a path that traverses a link reference |
| `unresolved-clone-reference` | Clone target path doesn't exist |
| `circular-reference` | Clone chain forms a cycle |
| `clone-reference-out-of-scope` | Relative ref in cloned subtree escapes boundary |

## Session lifecycle

The interpreter is invoked through `MOTLYSession`:

1. **parse()** — parse source, accumulate statements. Returns syntax errors only.
2. **finish()** — flatten all accumulated statements, chunk, sort, execute.
   Validate references. Return `MOTLYResult` with the tree and all errors.

Multiple `parse()` calls accumulate. Each gets a sequential `parseId` so
locations can be traced back to their source. After `finish()`, the
session is spent.

The `ExecContext` bundles `parseId` and `SessionOptions` (currently just
`disableReferences`). It's threaded through all interpreter operations.
