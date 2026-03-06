# MOTLY TypeScript API

The `@malloydata/motly-ts-parser` package is a pure TypeScript MOTLY parser with zero native dependencies. This document covers the full consumer API.

For the MOTLY language itself, see [language.md](language.md). For schema validation, see [schema.md](schema.md).

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
const port = config.get("server", "port").number;  // 8080
const tags = config.get("tags").texts;              // ["web", "api", "production"]
```

## MOTLYSession

A stateful parsing session. Source text is parsed and accumulated into an internal value tree. An optional schema can be loaded for validation. The `Mot` read API provides typed, navigable access to the resolved tree.

### `parse(source: string): MOTLYError[]`

Parse MOTLY source and apply it to the session's value. Multiple calls accumulate — later statements merge with or override earlier ones.

Returns an array of parse errors (empty on success).

```ts
const session = new MOTLYSession();
let errors = session.parse("server { host = localhost }");
errors = session.parse("server { port = 8080 }");  // merges with existing
```

### `parseSchema(source: string): MOTLYError[]`

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

### `getMot(options?: GetMotOptions): Mot`

Return a resolved `Mot` view of the current value tree. All references followed, environment variables substituted, deletions consumed.

```ts
const config = session.getMot({
  env: { API_KEY: "secret", DB_HOST: "db.example.com" }
});
```

The `env` option provides values for `@env.NAME` references. Missing env vars produce nodes with no value.

`getMot()` is forgiving — it always succeeds. Unresolved references produce the Undefined Mot at that position. To detect problems, call `validateReferences()` and `validateSchema()` before `getMot()`.

### `getValue(): MOTLYNode`

Return a deep clone of the raw, unresolved parse tree. This is the low-level representation with refs, env refs, and deleted nodes still present. Most consumers should use `getMot()` instead.

### `reset(): void`

Clear the value tree to empty, keeping the schema.

### `dispose(): void`

Mark the session as disposed. After calling `dispose()`, all other methods throw. `dispose()` itself is idempotent. For the pure TS implementation this is a no-op (no native resources to free), but calling it enables consistent lifecycle management across backends.

## Mot

The `Mot` interface is the consumer-facing read API. Every `Mot` has two independent aspects:

- A **value** — a scalar (string, number, boolean, date), an array of Mots, or nothing
- **Properties** — a map of named child Mots

### Navigation

#### `get(...props: string[]): Mot`

Walk into properties by name. Returns the `Mot` at the end of the path. If any step does not exist, returns the **Undefined Mot** (see below). Never returns `undefined`.

```ts
config.get("server", "port")         // equivalent to
config.get("server").get("port")
```

Property navigation only — no numeric indexing. Array elements are accessed through `.values`.

#### `has(...props: string[]): boolean`

Returns `true` if the full property path exists. Equivalent to `.get(...props).exists`.

```ts
if (config.has("server", "ssl")) {
  // ...
}
```

#### `exists: boolean`

`true` for any real node (including flags with no value). `false` only for the Undefined Mot.

### Value Type

#### `valueType: "string" | "number" | "boolean" | "date" | "array" | undefined`

The type of the value slot, or `undefined` if the node has no value. This distinguishes three states:

| State | `exists` | `valueType` |
|---|---|---|
| Flag (node exists, no value) | `true` | `undefined` |
| Valued node | `true` | `"string"`, `"number"`, etc. |
| Undefined Mot | `false` | `undefined` |

### Typed Value Accessors

Each returns the value if it matches the requested type, `undefined` otherwise. Accessors never coerce.

| Accessor | Returns |
|---|---|
| `text` | `string \| undefined` |
| `number` | `number \| undefined` |
| `boolean` | `boolean \| undefined` |
| `date` | `Date \| undefined` |

```ts
const port = config.get("server", "port").number;
if (port !== undefined) {
  listen(port);
}
```

### Array Access

#### `values: Mot[] | undefined`

The array elements as Mots, or `undefined` if the value is not an array. Each element is a full `Mot` with its own value and properties.

```motly
items = [
  widget { color = red  size = 10 },
  gadget { color = blue size = 20 }
]
```

```ts
const items = config.get("items").values;
const name  = items?.[0]?.text;                 // "widget"
const color = items?.[0]?.get("color")?.text;   // "red"
```

#### Typed Array Convenience Accessors

Return a typed array if **all** elements match the requested type. If any element doesn't match, the accessor returns `undefined`.

| Accessor | Returns |
|---|---|
| `texts` | `string[] \| undefined` |
| `numbers` | `number[] \| undefined` |
| `booleans` | `boolean[] \| undefined` |
| `dates` | `Date[] \| undefined` |

```ts
const tags = config.get("tags").texts;  // ["web", "api", "production"]
```

### Property Enumeration

#### `keys: Iterable<string>`

Property names. Empty for nodes with no properties and for the Undefined Mot.

#### `entries: Iterable<[string, Mot]>`

`[name, Mot]` pairs for all properties.

```ts
for (const [key, child] of config.entries) {
  console.log(key, child.valueType);
}
```

### The Undefined Mot

A special singleton returned by `get()` when any step in the path does not exist. Enables safe deep navigation without null checks.

| Property | Value |
|---|---|
| `exists` | `false` |
| `valueType` | `undefined` |
| `text`, `number`, `boolean`, `date` | `undefined` |
| `values`, `texts`, `numbers`, `booleans`, `dates` | `undefined` |
| `keys`, `entries` | empty |
| `get(...)` | returns itself (propagates) |
| `has(...)` | `false` |

```ts
// If "server" doesn't exist, get("port") returns the Undefined Mot,
// and .number returns undefined. No ?. needed.
const port = config.get("server", "port").number;
```

## References and Environment Variables

References and env refs are resolved before the `Mot` is returned. The consumer never sees them.

- **Unresolved references** (target doesn't exist, or `^` escapes root) produce the Undefined Mot.
- **Circular references** with a concrete backing node work — `Mot` instances are shared. Pure cycles with no backing node produce Undefined Mots.
- **Environment references** are substituted from the `env` map passed to `getMot()`. Missing env vars produce a node with no value (flag).

## Error Types

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

const dbHost = config.get("database", "host").text;     // from DB_HOST env var
const dbPort = config.get("database", "port").number;   // 5432
const dbName = config.get("database", "name").text;     // "myapp"

if (config.has("features", "caching")) {
  enableCaching();
}

for (const [feature] of config.get("features").entries) {
  console.log(`Feature enabled: ${feature}`);
}
```
