# MOTLY Schema Language Specification — Iteration 2

This is the second iteration of the schema language. The first used lowercase keywords (`Required`, `Optional`, `Types`) and ad-hoc features (`eq`, `matches`, `oneOf`). This version is more robust, better tested, and has a self-validating meta-schema. It will probably change again.

The TypeScript validator implements this spec fully (118 test fixtures). The Rust validator is not yet updated. The meta-schema (`motly_schema.motly`) validates against itself.

## Overview

The MOTLY schema language is a DSL written in MOTLY for describing the expected structure of MOTLY documents. MOTLY provides the fixed lexer (strings, numbers, booleans, dates, flags, nodes with values, nodes with properties, arrays); the schema language provides the grammar for a specific document type.

A schema file is itself a valid MOTLY document. The meta-schema (the schema that validates schema files) is written in the schema language, serving as the self-description litmus test.

## Design Principles

1. **Everything is a type reference.** There is no "sugar" or shorthand expansion. Inside `REQUIRED` and `OPTIONAL`, `property_name = type_name` is a type reference. Pre-loaded types (`string`, `number`, `flag`, etc.) are identical in status to user-defined types.

2. **Directives are ALL-CAPS.** All schema DSL keywords (`VALUE`, `REQUIRED`, `OPTIONAL`, `TYPES`, `ADDITIONAL`, `ENUM`, `MATCHES`, `ONEOF`, etc.) are written in all capitals. User-defined property and type names are lowercase/mixed case. This eliminates namespace collisions and creates two visually distinct layers.

3. **Portable, data-driven validation.** A schema file fetched via URL must provide useful validation without application-specific code. Cross-node semantic constraints that cannot be cleanly expressed as data are out of scope.

4. **Compiler analogy.** The schema is to MOTLY what a grammar is to a lexer. Types are non-terminals. `REQUIRED`/`OPTIONAL` are production rules. `VALUE` constraints are terminal-level matching rules. Application-level semantic validation is a separate phase, like semantic analysis in a compiler.

## The Recursive Constraint Node

The fundamental unit of the schema language is the **constraint node** — a type definition that describes the expected shape of a node in a target MOTLY document. A constraint node can be named (in `TYPES`) or anonymous (inline at point of use). Its structure is the same everywhere:

| Directive | Purpose |
|-----------|---------|
| `VALUE` | Constrains the target node's value slot |
| `REQUIRED` | Child properties that must exist |
| `OPTIONAL` | Child properties that may exist |
| `ADDITIONAL` | Policy for unlisted child properties |
| `ONEOF` | Union — node can match one of several named types |
| `DESCRIPTION` | Human-readable documentation |
| `TYPES` | Named type definitions (**root level only**) |

Inside `REQUIRED` and `OPTIONAL`, every entry is:

```
property_name = type_name
```

or, for inline type definitions:

```
property_name {
  VALUE = ...
  REQUIRED { ... }
  OPTIONAL { ... }
  ...
}
```

These are the same construct — one references a named type, the other defines an anonymous type at point of use.

## Top-Level Schema Structure

A schema file's root is an anonymous constraint node describing the target document's root, with the addition of `TYPES` which is only allowed at root level:

```
TYPES {
  # Named type definitions
}
REQUIRED {
  # Properties that must exist at the document root
}
OPTIONAL {
  # Properties that may exist at the document root
}
ADDITIONAL = reject
```

All top-level directives are optional. A minimal schema could be just `ADDITIONAL = accept` (allow anything).

## Pre-loaded Types

The validator pre-loads these type definitions into the `TYPES` namespace before reading the schema file. They are ordinary type definitions, not language keywords:

```
# Value-slot types — these constrain the value slot
string  { VALUE = string }
number  { VALUE = number }
integer { VALUE = integer }
boolean { VALUE = boolean }
date    { VALUE = date }

# Whole-node types — these constrain the node's shape
flag    { ADDITIONAL = reject }
tag     { ADDITIONAL = accept }
any     { ADDITIONAL = accept }
```

The five value-slot primitives (`string`, `number`, `integer`, `boolean`, `date`) are the only things the validator recognizes natively at the bottom of the recursion. Everything else — including `flag`, `tag`, and `any` — is built on top of them as ordinary type definitions.

A schema file can reference pre-loaded types exactly like user-defined types:

```
REQUIRED {
  name = string       # pre-loaded type
  port = number       # pre-loaded type
  server = ServerDef  # user-defined type
  hidden = flag       # pre-loaded type (no value, no properties)
}
```

## VALUE

`VALUE` constrains the value slot of the target node. Its own value is a type name reference (one of the five value-slot primitives or a user-defined value type). Refinement properties are type-appropriate.

### String VALUE

```
VALUE = string
VALUE = string { ENUM = [red, green, blue] }
VALUE = string { MATCHES = "^[a-z][a-z0-9-]*$" }
VALUE = string { MIN_LENGTH = 1  MAX_LENGTH = 255 }
```

Available refinements: `ENUM` (`"string[]"`), `MATCHES` (string), `MIN_LENGTH` (integer), `MAX_LENGTH` (integer).

### Number/Integer VALUE

```
VALUE = number
VALUE = integer
VALUE = number { MIN = 0  MAX = 65535 }
VALUE = number { ENUM = [1, 2, 3] }
VALUE = integer { MIN = 0 }
```

Available refinements: `ENUM` (`"number[]"`), `MIN` (number), `MAX` (number).

### Boolean VALUE

```
VALUE = boolean
VALUE = boolean { ENUM = [@true] }
```

Available refinements: `ENUM` (`"boolean[]"`). Rarely needed since boolean has only two values.

### Date VALUE

```
VALUE = date
VALUE = date { ENUM = [@2024-01-01, @2024-07-01] }
```

Available refinements: `ENUM` (`"date[]"`).

### Refinement Summary

| Refinement | Applies to | Type of refinement value | Purpose |
|------------|-----------|--------------------------|---------|
| `ENUM` | string, number, integer, boolean, date | Array of matching type | Allowed literal values |
| `MATCHES` | string only | string | Regex pattern |
| `MIN` | number, integer only | number | Minimum value (inclusive) |
| `MAX` | number, integer only | number | Maximum value (inclusive) |
| `MIN_LENGTH` | string only | integer | Minimum string length |
| `MAX_LENGTH` | string only | integer | Maximum string length |

Applying a refinement to the wrong type is a schema validation error (e.g., `VALUE = string { MIN = 0 }` is invalid).

## TYPES

Named type definitions for reuse. The namespace is flat — all types are defined at root level. Nested `TYPES` blocks are not supported.

```
TYPES {
  # Value refinement type
  Email {
    VALUE = string { MATCHES = "^[^@]+@[^@]+$" }
  }

  # Enum type
  Color {
    VALUE = string { ENUM = [red, green, blue] }
  }

  # Structured type
  ServerConfig {
    REQUIRED {
      host = string
      port = number
    }
    OPTIONAL {
      ssl = boolean
    }
  }

  # Value + structure (MOTLY's dual nature)
  Font {
    VALUE = string
    REQUIRED {
      size = number
      weight = string
    }
  }

  # Recursive type
  TreeNode {
    REQUIRED {
      value = number
    }
    OPTIONAL {
      children = "TreeNode[]"
    }
  }
}
```

User-defined type names cannot shadow pre-loaded type names (`string`, `number`, `integer`, `boolean`, `date`, `flag`, `tag`, `any`).

### Union Types (ONEOF)

A union says the node can match one of several named types. `ONEOF` is a constraint-node-level directive (sibling of `VALUE`, `REQUIRED`, `OPTIONAL`).

```
TYPES {
  # Shorthand — brackets at the TYPES level always mean union
  Auth = [TokenAuth, UsernamePasswordAuth]

  # Explicit form (equivalent)
  Auth {
    ONEOF = [TokenAuth, UsernamePasswordAuth]
  }
}
```

Union members are type names (strings). The validator determines which branch matches. Dispatch is an implementation concern — the language does not prescribe a matching strategy, but deterministic dispatch based on observable node features (presence/type of value, presence of properties) is recommended for better error messages.

### Enum vs Union

These are different concepts at different levels:

- **ENUM** is a value refinement inside `VALUE`. It constrains which literal values are allowed in the value slot. It answers: "which specific values are legal here?"
- **ONEOF** is a structural directive at the constraint-node level. It says the node can take one of several shapes. It answers: "which types can this node be?"

Brackets inside `VALUE { ENUM = [...] }` are always literal values. Brackets at the `TYPES` definition level (or in `ONEOF = [...]`) are always type names. No ambiguity.

## ADDITIONAL

Controls handling of properties not listed in `REQUIRED` or `OPTIONAL`. Takes a type reference or a keyword:

```
ADDITIONAL = reject          # reject unknown properties (default when absent)
ADDITIONAL = accept          # allow unknown properties without validation
ADDITIONAL = string          # unknown properties must have string values
ADDITIONAL = ServerConfig    # unknown properties must conform to ServerConfig
ADDITIONAL = PropertyDef     # in the meta-schema: unlisted props are property defs
```

When `ADDITIONAL` is absent, unknown properties are rejected.

`ADDITIONAL` can also be an inline type definition:

```
ADDITIONAL {
  VALUE = string
  OPTIONAL {
    label = string
  }
}
```

## Array Types

Append `[]` to any type name, quoted to prevent bracket parsing:

```
tags = "string[]"
ports = "number[]"
items = "ServerConfig[]"
children = "TreeNode[]"
```

Each array element is validated against the inner type.

## Property Metadata

Properties inside `REQUIRED` and `OPTIONAL` can carry metadata directives. These describe relationships, defaults, and documentation — not the type itself. They are written as properties on the property definition node.

### EXCLUSIVE

Mutual exclusion — at most one property from a named group can be present:

```
OPTIONAL {
  number = NumberFormat { EXCLUSIVE = format_group }
  currency = CurrencyFormat { EXCLUSIVE = format_group }
  percent = flag { EXCLUSIVE = format_group }
  duration = DurationFormat { EXCLUSIVE = format_group }
}
```

A property can belong to multiple exclusion groups using an array:

```
OPTIONAL {
  viz = VizType { EXCLUSIVE = renderer }
  bar_chart = flag { EXCLUSIVE = [renderer, legacy] }
  line_chart = flag { EXCLUSIVE = [renderer, legacy] }
}
```

The exclusion is symmetric by definition — all members of a group are mutually exclusive with each other.

### REQUIRES

Dependency — if this property is present, the listed siblings must also be present:

```
OPTIONAL {
  comparison_field = string
  comparison_label = string { REQUIRES = [comparison_field] }
}
```

Named properties in the `REQUIRES` list must be defined in the same `REQUIRED` or `OPTIONAL` block (they are sibling references).

### DEFAULT

Default value for a property. Only valid inside `OPTIONAL` (required properties don't need defaults). Important for the ORM/DOM API use case:

```
OPTIONAL {
  timeout = number { DEFAULT = 30 }
  ssl = boolean { DEFAULT = @false }
  log_level = LogLevel { DEFAULT = info }
}
```

The default value must be valid according to the property's type.

### DESCRIPTION

Human-readable documentation. Drives IDE tooltips, hover docs, and generated documentation:

```
REQUIRED {
  host = string { DESCRIPTION = "Hostname or IP address of the server" }
  port = number { DESCRIPTION = "TCP port number (1-65535)" }
}
```

Also valid on type definitions:

```
TYPES {
  ServerConfig {
    DESCRIPTION = "Database server connection configuration"
    REQUIRED { ... }
  }
}
```

### DEPRECATED

Marks a property as still accepted but discouraged. Can be bare (flag-like presence) or carry a message:

```
OPTIONAL {
  bar_chart = flag { DEPRECATED = "Use viz = bar instead" }
  line_chart = flag { DEPRECATED = "Use viz = line instead" }
  old_setting = string { DEPRECATED }
}
```

The validator should produce a warning (not an error) for deprecated properties.

## Schema Directive

A MOTLY file can declare its schema on the first line:

```
#! schema=app-config url="./schemas/app.motly"
```

The `#!` line is a comment. By convention, tools strip the `#!` prefix and parse the remainder as MOTLY to extract:

- `schema` — a short identifier for the schema
- `url` — location of the schema file (URL or relative path)

A file may specify just `schema`, just `url`, or both.

## Error Model

Validation errors should include:

| Field | Description |
|-------|-------------|
| Path | Dot-separated path to the offending node (e.g., `server.port`) |
| Code | Machine-readable error code |
| Message | Human-readable description |

### Error Codes

| Code | Description |
|------|-------------|
| `missing-required` | A required property is not present |
| `wrong-type` | Property value has incorrect type |
| `unknown-property` | Property not defined in schema and ADDITIONAL rejects it |
| `invalid-enum-value` | Value not in the allowed ENUM list |
| `pattern-mismatch` | String doesn't match MATCHES regex |
| `out-of-range` | Number outside MIN/MAX bounds |
| `length-violation` | String outside MIN_LENGTH/MAX_LENGTH bounds |
| `exclusive-violation` | Multiple properties from same EXCLUSIVE group present |
| `requires-violation` | Property present but required sibling(s) missing |
| `invalid-schema` | The schema itself is invalid |

Deprecated properties produce warnings, not errors.

## Complete Example

### Schema (`app-schema.motly`)

```
#! schema=motly-schema

TYPES {
  LogLevel {
    VALUE = string { ENUM = [debug, info, warn, error] }
  }
  Semver {
    VALUE = string { MATCHES = "^\\d+\\.\\d+\\.\\d+$" }
  }
}

REQUIRED {
  app {
    REQUIRED {
      name = string
      version = Semver
    }
    OPTIONAL {
      debug = boolean { DEFAULT = @false }
    }
  }
  server {
    REQUIRED {
      host = string
      port = integer { MIN = 1  MAX = 65535 }
    }
    OPTIONAL {
      ssl = boolean
      timeout = number
    }
  }
}

OPTIONAL {
  features = "string[]"
  logLevel = LogLevel
  metadata = tag
}
```

### Valid Configuration (`app.motly`)

```
#! schema=app-config url="./app-schema.motly"

app {
  name = "My Application"
  version = "1.2.0"
  debug = @false
}

server {
  host = localhost
  port = 8080
  ssl = @true
}

features = [logging, metrics]
logLevel = info
```

### Invalid Configuration and Errors

```
app {
  name = "My Application"
  # missing required: version
}

server {
  host = localhost
  port = "not a number"
}

unknown_prop = hello
logLevel = trace
```

| Error | Path | Code |
|-------|------|------|
| Missing required property "version" | `app.version` | `missing-required` |
| Expected integer, got string | `server.port` | `wrong-type` |
| Unknown property | `unknown_prop` | `unknown-property` |
| Value "trace" not in enum [debug, info, warn, error] | `logLevel` | `invalid-enum-value` |

## Open Design Questions

### 1. Imports

Cross-file type reuse. The flat type namespace means imports are "more names become available." Not yet designed. The core language does not depend on this.

### 2. Merge vs Replace Semantics in Schema Files

For well-formed schemas where each directive appears once per scope, the merge/replace distinction is moot. Whether the validator should reject duplicate directives at the same level is undecided.

### 3. Value-Dependent Requirements

`REQUIRES` and `EXCLUSIVE` handle presence-based dependencies. "If X has *this value* then Y is required" is value-dependent and probably belongs in application-level validation, not the schema. Confirm against real-world schemas.
