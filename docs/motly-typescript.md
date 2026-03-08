# MOTLY TypeScript API

The `@malloydata/motly-ts-parser` package is a pure TypeScript MOTLY parser with zero native dependencies. This document covers the full consumer API.

For the MOTLY language itself, see [language.md](language.md). For schema validation, see [schema_spec.md](schema_spec.md).

## Installation

```sh
npm install @malloydata/motly-ts-parser
```

## Quick Start

```ts
import { MOTLYSession } from "@malloydata/motly-ts-parser";

const session = new MOTLYSession();
session.parse(`
  server {
    host = localhost
    port = 8080
  }
  tags = [web, api, production]
`);

const config = session.getMot();
const port = config.get("server", "port").numeric();  // 8080
const tags = config.get("tags").texts();              // ["web", "api", "production"]

// Pathed shorthand — equivalent to the above
const port2 = config.numeric("server", "port");        // 8080
const tags2 = config.texts("tags");                   // ["web", "api", "production"]
```

## MOTLYSession

A stateful parsing session. Source text is parsed and accumulated into an internal value tree. An optional schema can be loaded for validation. The `Mot` read API provides typed, navigable access to the resolved tree.

### `parse(source: string): MOTLYParseResult`

Parse MOTLY source and apply it to the session's value. Multiple calls accumulate — later statements merge with or override earlier ones.

Returns a `MOTLYParseResult` containing the assigned `parseId` and any errors.

```ts
const session = new MOTLYSession();
let { errors } = session.parse("server { host = localhost }");
({ errors } = session.parse("server { port = 8080 }"));  // merges with existing
```

### `parseSchema(source: string): MOTLYParseResult`

Parse MOTLY source as a schema. Replaces any previously loaded schema. The schema is parsed fresh (not merged with previous schemas).

```ts
session.parseSchema(`
  Required {
    server {
      Required {
        host = string
        port = number
      }
    }
  }
`);
```

### `validateReferences(): MOTLYValidationError[]`

Check that all `$`-references in the value tree resolve to existing nodes. Returns an array of validation errors.

```ts
const refErrors = session.validateReferences();
for (const err of refErrors) {
  console.log(err.code, err.path.join("."), err.message);
}
```

### `validateSchema(): MOTLYSchemaError[]`

Validate the value tree against the loaded schema. Returns an empty array if no schema has been set.

```ts
const schemaErrors = session.validateSchema();
for (const err of schemaErrors) {
  console.log(err.code, err.path.join("."), err.message);
}
```

Error codes: `missing-required`, `wrong-type`, `unknown-property`, `invalid-schema`, `invalid-enum-value`, `pattern-mismatch`.

### `getMot<M extends Mot = Mot>(options?: GetMotOptions<M>): M`

Return a resolved `Mot` view of the current value tree. All references followed, environment variables substituted, deletions consumed.

```ts
const config = session.getMot({
  env: { API_KEY: "secret", DB_HOST: "db.example.com" }
});
```

The `env` option provides values for `@env.NAME` references. Missing env vars produce nodes with no value.

Pass a `MotFactory` via `options.factory` to control what objects are created (e.g., Tags with read tracking). Without a factory, returns plain Mot instances. See [MotFactory](#motfactory) for details.

`getMot()` is forgiving — it always succeeds. Unresolved references produce the Undefined Mot at that position. To detect problems, call `validateReferences()` and `validateSchema()` before `getMot()`.

### `getValue(): MOTLYDataNode`

Return a deep clone of the raw, unresolved parse tree. This is the low-level representation with refs, env refs, and deleted nodes still present. Most consumers should use `getMot()` instead.

### `reset(): void`

Clear the value tree to empty, keeping the schema.

### `dispose(): void`

Mark the session as disposed. After calling `dispose()`, all other methods throw. `dispose()` itself is idempotent. For the pure TS implementation this is a no-op (no native resources to free), but calling it enables consistent lifecycle management across backends.

## Mot

The `Mot` interface is the consumer-facing read API. Every `Mot` has two independent aspects:

- A **value** — a scalar (string, number, boolean, date), an array of Mots, or nothing
- **Properties** — a map of named child Mots

All value accessors are **methods**, not properties. This allows implementations to add side effects (e.g., read tracking) and to accept optional path arguments for shorthand navigation.

### Navigation

#### `get(...path: MotPath): Mot`

Navigate by property names and/or array indices. Returns the `Mot` at the end of the path. String segments navigate properties; number segments index into array values. If any step does not exist, returns the **Undefined Mot** (see below). Never returns `undefined`.

```ts
config.get("server", "port")         // equivalent to
config.get("server").get("port")

// Numeric segments index into arrays
config.get("items", 0, "name")       // first item's name
config.get("items", 2)               // third array element
```

`MotPath` is `(string | number)[]`. Non-integer numeric indices (e.g., `1.5`, `NaN`) return the Undefined Mot.

#### `has(...path: MotPath): boolean`

Returns `true` if the full path exists. Equivalent to `.get(...path).exists`.

```ts
if (config.has("server", "ssl")) {
  // ...
}
```

#### `exists: boolean`

`true` for any real node (including flags with no value). `false` only for the Undefined Mot.

### Value Type

#### `valueType(...path: MotPath): "string" | "number" | "boolean" | "date" | "array" | undefined`

The type of the value slot, or `undefined` if the node has no value. If path segments are provided, navigates first via `get()`. This distinguishes three states:

| State | `exists` | `valueType()` |
|---|---|---|
| Flag (node exists, no value) | `true` | `undefined` |
| Valued node | `true` | `"string"`, `"number"`, etc. |
| Undefined Mot | `false` | `undefined` |

### Typed Value Accessors

Each returns the value if it matches the requested type, `undefined` otherwise. Accessors never coerce. All accept optional path segments for shorthand navigation.

| Accessor | Returns |
|---|---|
| `text(...path)` | `string \| undefined` |
| `numeric(...path)` | `number \| undefined` |
| `boolean(...path)` | `boolean \| undefined` |
| `date(...path)` | `Date \| undefined` |

```ts
// Explicit navigation + accessor
const port = config.get("server", "port").numeric();

// Pathed shorthand — equivalent
const port = config.numeric("server", "port");
```

### Array Access

#### `values(...path: MotPath): Mot[] | undefined`

The array elements as Mots, or `undefined` if the value is not an array. Each element is a full `Mot` with its own value and properties.

```motly
items = [
  widget { color = red  size = 10 },
  gadget { color = blue size = 20 }
]
```

```ts
const items = config.get("items").values();
const name  = items?.[0]?.text();                  // "widget"
const color = items?.[0]?.get("color")?.text();    // "red"

// Or use numeric path segments
const name  = config.get("items", 0).text();       // "widget"
const color = config.text("items", 0, "color");    // "red"
```

#### Typed Array Convenience Accessors

Return a typed array if **all** elements match the requested type. If any element doesn't match, the accessor returns `undefined`. All accept optional path segments.

| Accessor | Returns |
|---|---|
| `texts(...path)` | `string[] \| undefined` |
| `numerics(...path)` | `number[] \| undefined` |
| `booleans(...path)` | `boolean[] \| undefined` |
| `dates(...path)` | `Date[] \| undefined` |

```ts
const tags = config.texts("tags");  // ["web", "api", "production"]
```

### Property Enumeration

#### `keys: Iterable<string>`

Property names. Empty for nodes with no properties and for the Undefined Mot.

#### `entries: Iterable<[string, Mot]>`

`[name, Mot]` pairs for all properties.

```ts
for (const [key, child] of config.entries) {
  console.log(key, child.valueType());
}
```

### The Undefined Mot

A special singleton returned by `get()` when any step in the path does not exist. Enables safe deep navigation without null checks.

| Method / Property | Value |
|---|---|
| `exists` | `false` |
| `valueType()` | `undefined` |
| `text()`, `numeric()`, `boolean()`, `date()` | `undefined` |
| `values()`, `texts()`, `numerics()`, `booleans()`, `dates()` | `undefined` |
| `keys`, `entries` | empty |
| `get(...)` | returns itself (propagates) |
| `has(...)` | `false` |

```ts
// If "server" doesn't exist, get("port") returns the Undefined Mot,
// and .numeric() returns undefined. No ?. needed.
const port = config.get("server", "port").numeric();
```

## MotFactory

The `MotFactory` interface lets you control what objects `getMot()` creates. This enables custom Mot implementations with additional capabilities (e.g., read tracking, mutation).

```ts
interface MotFactory<M extends Mot = Mot> {
  createMot(value: MotResolvedValue, properties: Map<string, M>): M;
  undefinedMot: M;
}
```

The factory's `createMot` receives a resolved value and a mutable properties `Map`. The Map is empty at creation time and populated afterward — implementations must read from it lazily (not copy at construction time).

When `M extends Mot`, array elements in `MotResolvedValue` are typed as `Mot[]` but are `M` instances at runtime. Factory implementations should cast if needed.

```ts
const mot = session.getMot({ factory: myFactory });
```

## References and Environment Variables

References and env refs are resolved before the `Mot` is returned. The consumer never sees them.

- **Unresolved references** (target doesn't exist, or `^` escapes root) produce the Undefined Mot.
- **Circular references** with a concrete backing node work — `Mot` instances are shared. Pure cycles with no backing node produce Undefined Mots.
- **Environment references** are substituted from the `env` map passed to `getMot()`. Missing env vars produce a node with no value (flag).

## Error Types

### `MOTLYParseResult`

Returned by `parse()` and `parseSchema()`.

```ts
interface MOTLYParseResult {
  parseId: number;       // auto-incrementing per session
  errors: MOTLYError[];
}
```

### `MOTLYError`

A parse error with source location.

```ts
interface MOTLYError {
  code: string;                                    // e.g. "tag-parse-syntax-error"
  message: string;
  begin: { line: number; column: number; offset: number };
  end: { line: number; column: number; offset: number };
}
```

### `MOTLYSchemaError`

A schema validation error with path.

```ts
interface MOTLYSchemaError {
  code: string;      // e.g. "missing-required", "wrong-type"
  message: string;
  path: string[];    // e.g. ["server", "port"]
}
```

### `MOTLYValidationError`

A reference validation error with path.

```ts
interface MOTLYValidationError {
  code: string;      // e.g. "unresolved-reference"
  message: string;
  path: string[];
}
```

## Complete Example

```ts
import { MOTLYSession } from "@malloydata/motly-ts-parser";

const session = new MOTLYSession();

// Load schema
session.parseSchema(`
  Required {
    database {
      Required {
        host = string
        port = number
      }
      Optional {
        name = string
      }
    }
  }
  Optional {
    features {
      Additional = flag
    }
  }
`);

// Load config
session.parse(`
  database {
    host = @env.DB_HOST
    port = 5432
    name = myapp
  }
  features {
    caching
    logging
  }
`);

// Validate
const errors = [
  ...session.validateReferences(),
  ...session.validateSchema(),
];
if (errors.length > 0) {
  for (const e of errors) console.error(e.message);
  process.exit(1);
}

// Read
const config = session.getMot({ env: process.env });

const dbHost = config.text("database", "host");     // from DB_HOST env var
const dbPort = config.numeric("database", "port");   // 5432
const dbName = config.text("database", "name");     // "myapp"

if (config.has("features", "caching")) {
  enableCaching();
}

for (const [feature] of config.get("features").entries) {
  console.log(`Feature enabled: ${feature}`);
}
```
