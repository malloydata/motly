# @malloydata/motly-ts-parser

A pure TypeScript implementation of a parser for [MOTLY](https://github.com/malloydata/motly), a configuration language from [Malloy](https://github.com/malloydata/malloy). Zero native dependencies.

This package parses MOTLY source text into a node tree and validates it against schemas. Higher-level language bindings for working with MOTLY data are planned separately.

## Install

```sh
npm install @malloydata/motly-ts-parser
```

## Quick usage

```typescript
import { MOTLYSession } from "@malloydata/motly-ts-parser";

const session = new MOTLYSession();

const errors = session.parse(`
  server = webhost {
    host = localhost
    port = 8080
    ssl  = @true
  }
`);

if (errors.length > 0) {
  console.error(errors);
} else {
  const value = session.getValue();
  console.log(value);
  // {
  //   properties: {
  //     server: {
  //       eq: "webhost",
  //       properties: {
  //         host: { eq: "localhost" },
  //         port: { eq: 8080 },
  //         ssl:  { eq: true }
  //       }
  //     }
  //   }
  // }
}

session.dispose();
```

## MOTLY syntax at a glance

```motly
# Bare strings, numbers, booleans, dates
name    = hello
port    = 8080
enabled = @true
created = @2024-01-15

# Nested properties
database: {
  host = localhost
  pool: { max = 20  min = 5 }
}

# Deep path shorthand
database.pool.timeout = 5000

# Arrays (including objects)
colors = [red, green, blue]
users  = [
  { name = alice  role = admin },
  { name = bob    role = user }
]

# References and cloning
defaults: { timeout = 30  retries = 3 }
api := $defaults { timeout = 10 }

# Environment references
secrets: { password = @env.DB_PASSWORD }
```

See the [full language reference](https://github.com/malloydata/motly/blob/main/docs/language.md) for all string types, operators, deletion, schemas, and more.

## API reference

### `MOTLYSession`

```typescript
class MOTLYSession {
  parse(source: string): MOTLYError[];
  parseSchema(source: string): MOTLYError[];
  getValue(): MOTLYNode;
  reset(): void;
  validateSchema(): MOTLYSchemaError[];
  validateReferences(): MOTLYValidationError[];
  dispose(): void;
}
```

| Method | Description |
|--------|-------------|
| `parse(source)` | Parse MOTLY source and apply it to the session value. Returns parse errors. Successive calls accumulate into the same value tree. |
| `parseSchema(source)` | Parse MOTLY source as a schema (replaces any previous schema). |
| `getValue()` | Return a deep clone of the current value tree. |
| `reset()` | Clear the value tree (schema is kept). |
| `validateSchema()` | Validate the current value against the stored schema. Returns `[]` if no schema is set. |
| `validateReferences()` | Check that all `$`-references in the value tree resolve. |
| `dispose()` | Mark the session as dead. All subsequent method calls will throw. |

### Exported types

```typescript
// The value tree
interface MOTLYNode {
  eq?: MOTLYValue;
  properties?: Record<string, MOTLYPropertyValue>;
  deleted?: boolean;
}

type MOTLYScalar        = string | number | boolean | Date;
type MOTLYValue         = MOTLYScalar | MOTLYEnvRef | MOTLYPropertyValue[];
type MOTLYPropertyValue = MOTLYNode | MOTLYRef;

interface MOTLYRef    { linkTo: string }   // $-reference (structural link)
interface MOTLYEnvRef { env: string }       // @env.NAME (value from environment)

// Errors
interface MOTLYError           { code: string; message: string; begin: Position; end: Position }
interface MOTLYSchemaError     { code: string; message: string; path: string[] }
interface MOTLYValidationError { code: string; message: string; path: string[] }
```

### Type guards

```typescript
import { isRef, isEnvRef } from "@malloydata/motly-ts-parser";

isRef(propertyValue);   // true if MOTLYRef  ({ linkTo })
isEnvRef(node.eq);      // true if MOTLYEnvRef ({ env })
```

## Full language documentation

[docs/language.md](https://github.com/malloydata/motly/blob/main/docs/language.md) — complete reference with EBNF grammar.

## License

MIT
