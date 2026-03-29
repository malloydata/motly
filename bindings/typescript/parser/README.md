# @malloydata/motly-ts-parser

A pure TypeScript implementation of a parser for [MOTLY](https://github.com/malloydata/motly), a configuration language from [Malloy](https://github.com/malloydata/malloy). Zero native dependencies.

This package parses MOTLY source text into a node tree, optionally validates it against schemas, and provides a high-level read API (`Mot`) for navigating the result.

## Install

```sh
npm install @malloydata/motly-ts-parser
```

## Quick usage

```typescript
import { MOTLYSession } from "@malloydata/motly-ts-parser";

const session = new MOTLYSession();

const { errors } = session.parse(`
  server = webhost {
    host = localhost
    port = 8080
    ssl  = @true
  }
`);

if (errors.length > 0) {
  console.error("Parse errors:", errors);
}

const result = session.finish();

if (result.errors.length > 0) {
  console.error("Interpretation errors:", result.errors);
}

// Low-level tree access
const tree = result.getValue();
console.log(tree);
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

// High-level Mot access
const mot = result.getMot();
mot.get("server", "host").text();  // "localhost"
mot.get("server", "port").numeric();  // 8080
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

A write-only session that accumulates input. Call `parse()` one or more times, then `finish()` to interpret everything. The session is spent after `finish()`.

```typescript
class MOTLYSession {
  constructor(options?: MOTLYSessionOptions);
  parse(source: string): MOTLYParseResult;
  finish(): MOTLYResult;
  dispose(): void;
}

interface MOTLYSessionOptions {
  disableReferences?: boolean;  // reject $-references (clones still allowed)
}
```

| Method | Description |
|--------|-------------|
| `parse(source)` | Parse MOTLY source and accumulate statements. Returns `{ parseId, errors }`. Only syntax errors are returned here; semantic errors are deferred to `finish()`. |
| `finish()` | Interpret all accumulated input, validate references, and return an immutable `MOTLYResult`. The session is spent after this call. |
| `dispose()` | Mark the session as dead. All subsequent method calls will throw. |

### `MOTLYResult`

Immutable result from `finish()`. All interpretation and reference resolution has already happened.

```typescript
class MOTLYResult {
  readonly errors: MOTLYError[];
  getValue(): MOTLYDataNode;
  getMot<M extends Mot = Mot>(options?: GetMotOptions<M>): M;
}
```

| Method | Description |
|--------|-------------|
| `errors` | Interpretation + reference validation errors. |
| `getValue()` | Return a deep clone of the interpreted tree. |
| `getMot(options?)` | Return a resolved, read-only `Mot` view. Follows references lazily. Options: `{ factory }` for custom Mot subclasses, `{ env }` for `@env` resolution. |

### `MOTLYSchema`

Independent of sessions. Parse a schema once, validate any number of trees.

```typescript
class MOTLYSchema {
  static parse(source: string): { schema: MOTLYSchema; errors: MOTLYError[] };
  validate(tree: MOTLYDataNode): MOTLYSchemaError[];
}
```

### Exported types

```typescript
// The value tree
type MOTLYNode = MOTLYDataNode | MOTLYRef;

interface MOTLYDataNode {
  eq?: MOTLYValue;
  properties?: Record<string, MOTLYNode>;
  deleted?: boolean;
  location?: MOTLYLocation;
}

type MOTLYScalar = string | number | boolean | Date;
type MOTLYValue  = MOTLYScalar | MOTLYEnvRef | MOTLYNode[];

interface MOTLYRef    { linkTo: MOTLYRefSegment[]; linkUps: number }
interface MOTLYEnvRef { env: string }

// Locations
interface MOTLYLocation {
  parseId: number;
  begin: { line: number; column: number; offset: number };
  end:   { line: number; column: number; offset: number };
}

interface MOTLYParseResult { parseId: number; errors: MOTLYError[] }

// Errors
interface MOTLYError       { code: string; message: string; begin: Position; end: Position }
interface MOTLYSchemaError { code: string; message: string; path: string[]; location?: MOTLYLocation }
```

### Type guards

```typescript
import { isRef, isDataNode, isEnvRef } from "@malloydata/motly-ts-parser";

isRef(node);       // true if MOTLYRef  ({ linkTo, linkUps })
isDataNode(node);  // true if MOTLYDataNode (not a ref)
isEnvRef(node.eq); // true if MOTLYEnvRef ({ env })
```

## Full language documentation

[docs/language.md](https://github.com/malloydata/motly/blob/main/docs/language.md) — complete reference with EBNF grammar.

## License

MIT
