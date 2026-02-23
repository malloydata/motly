# TypeScript Bindings — Architecture

## Package Layout

```
bindings/typescript/
  interface/         — shared MOTLY tree types (single source of truth)
  parser/            — pure TS parser, published as @malloydata/motly-ts-parser
  wasm/              — (future) Rust/WASM parser, same API surface
```

## How Shared Types Work

The `interface/` directory contains the canonical TypeScript type definitions
for the MOTLY tree format: `MOTLYValue`, `MOTLYNode`, `MOTLYError`, etc.
These types define the wire format that all implementations must produce.

**`interface/` is never published to npm.** Instead, each implementation
package compiles the interface source directly into its own build output
using plain `tsc`. This is done by:

1. Widening `rootDir` in the implementation's `tsconfig.json` to `".."`,
   which encompasses both `interface/` and the implementation directory.
2. Adding `"../interface/src/**/*"` to the `include` array.
3. Using relative imports: `from "../../interface/src/types"`.

The result is that `tsc` emits the interface code into each package's
`build/` directory:

```
parser/build/
  interface/src/types.js       ← compiled from ../interface/src/types.ts
  interface/src/types.d.ts
  parser/src/index.js          ← require("../../interface/src/types") resolves within build/
  parser/src/session.js
  ...
```

The published tarball is completely self-contained — no external dependency
on a shared types package.

## Why Not a Separate Published Interface Package?

We want multiple implementation packages (`motly-ts-parser`, future
`motly-ts-wasm`) to export the same types. The obvious approach — publish
`@malloydata/motly-ts-interface` as its own npm package — creates an
operational burden: you must coordinate publishing the interface before
any implementation that depends on it, the interface rarely changes but
you still need a publish pipeline for it, and `file:` path references
in `devDependencies` break at publish time.

The current approach avoids all of this. Each implementation compiles the
types into its own output. TypeScript's structural type system means that
`MOTLYValue` from the parser package and `MOTLYValue` from the WASM package
are fully interchangeable — a consumer can swap implementations by changing
one import path.

## Adding a New Implementation

To add a new implementation (e.g., `wasm/`):

1. Create `wasm/tsconfig.json` with `rootDir: ".."` and include
   `"../interface/src/**/*"`.
2. Import types via relative paths: `from "../../interface/src/types"`.
3. Set `package.json` `main`/`types` to point into
   `build/<dirname>/src/index.js`.
4. The build output will include `build/interface/src/types.js` alongside
   the implementation code.

No changes to `interface/` or `parser/` are needed.
