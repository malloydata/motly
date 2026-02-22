# MOTLY Schema Validation Reference

> **NOTE:** This describes some early thinking on the idea that a schema for a MOTLY config file could be written using MOTLY as a DSL, which is attractive but has not been fully thought out.

MOTLY schemas validate that configuration files conform to expected structures. Schemas are themselves MOTLY files.

## Overview

A schema defines the expected shape of a MOTLY document using three sections:

- **`Required`** -- properties that must be present
- **`Optional`** -- properties that may be present
- **`Types`** -- reusable type definitions

Any property in the document not listed in `Required` or `Optional` is rejected by default.

```motly
Required: {
  name = string
  port = number
}
Optional: {
  debug = boolean
  tags = "string[]"
}
```

## Built-in Types

| Type | Matches | Example |
|------|---------|---------|
| `string` | Any string value | `name = string` |
| `number` | Any numeric value | `port = number` |
| `boolean` | `@true` or `@false` | `enabled = boolean` |
| `date` | ISO 8601 date | `created = date` |
| `tag` | Any node (may have properties, no scalar value required) | `config = tag` |
| `flag` | Presence-only node (exists but no specific type) | `hidden = flag` |
| `any` | Any value or node | `data = any` |

## Array Types

Append `[]` to any type name. Array types must be quoted to prevent the brackets from being parsed as an array literal:

| Type | Matches | Example |
|------|---------|---------|
| `"string[]"` | Array of strings | `tags = "string[]"` |
| `"number[]"` | Array of numbers | `ports = "number[]"` |
| `"boolean[]"` | Array of booleans | `flags = "boolean[]"` |
| `"date[]"` | Array of dates | `dates = "date[]"` |
| `"tag[]"` | Array of objects | `items = "tag[]"` |
| `"any[]"` | Array of anything | `data = "any[]"` |

Each element in the array is validated against the inner type.

## Nested Schemas

Define the structure of nested objects by nesting `Required`/`Optional` blocks inside a property definition:

```motly
Required: {
  database: {
    Required: {
      host = string
      port = number
    }
    Optional: {
      ssl = boolean
      pool: {
        Required: {
          min = number
          max = number
        }
      }
    }
  }
}
```

### Array Element Validation

For arrays of objects (`"tag[]"`), define the element schema as a custom type:

```motly
Types: {
  Item: {
    Required: {
      size = number
      color = string
    }
  }
}
Required: {
  items = "Item[]"
}
```

Validation errors include the array index in the path (e.g., `items.[1].size`).

## Custom Types

Define reusable types in the `Types` section at the **root level** of the schema file. Types defined at the root are available throughout the entire schema, including inside nested property definitions. (Nested `Types` sections are not supported.)

```motly
Types: {
  PersonType: {
    Required: {
      name = string
      age = number
    }
  }
}
Required: {
  user = PersonType
  manager = PersonType
}
```

Custom type names cannot conflict with built-in type names (`string`, `number`, `boolean`, `date`, `tag`, `flag`, `any`).

### Custom Type Arrays

Use quoted `"TypeName[]"` for arrays of a custom type:

```motly
Types: {
  PersonType: {
    Required: { name = string  age = number }
  }
}
Required: {
  people = "PersonType[]"
}
```

### Recursive Types

Custom types can reference themselves, enabling validation of recursive structures:

```motly
Types: {
  TreeNode: {
    Required: { value = number }
    Optional: { children = "TreeNode[]" }
  }
}
Required: {
  root = TreeNode
}
```

This validates trees of arbitrary depth where each node has a `value` and optional `children`.

## Enum Types

Define an enum as an array of allowed values:

```motly
Types: {
  StatusType = [pending, active, completed]
  LevelType = [1, 2, 3]
}
Required: {
  status = StatusType
  level = LevelType
}
```

Values not in the allowed list are rejected with an `invalid-enum-value` error.

Enums can also be defined inline using the `eq` property:

```motly
Required: {
  protocol = string { eq = [TCP, UDP, SCTP] }
}
```

## Pattern Types

Define a type with a `matches` property containing a regular expression:

```motly
Types: {
  EmailType.matches = "^[^@]+@[^@]+$"
  SemverType.matches = "^\\d+\\.\\d+\\.\\d+$"
}
Required: {
  email = EmailType
  version = SemverType
}
```

Non-matching strings are rejected with a `pattern-mismatch` error. Non-string values are rejected with a `wrong-type` error.

Patterns can also be defined inline:

```motly
Required: {
  name = string { matches = "^[a-z][a-z0-9-]*$" }
}
```

## Union Types (oneOf)

Define a type that accepts any of several types using `oneOf`:

```motly
Types: {
  StringOrNumber.oneOf = [string, number]
}
Required: {
  value = StringOrNumber
}
```

The value is validated against each type in the list; if any type matches (produces no errors), the value is valid.

Union types can reference built-in types, custom types, enums, and pattern types:

```motly
Types: {
  StatusEnum = [pending, active, completed]
  FlexibleValue.oneOf = [string, number, StatusEnum]
}
Required: {
  data = FlexibleValue
}
```

## Additional Properties

By default, properties not listed in `Required` or `Optional` cause `unknown-property` errors. The `Additional` keyword controls this:

**Reject unknown properties (default):**

```motly
Required: {
  name = string
}
# Any other property is an error
```

**Allow any additional properties:**

```motly
Additional
Required: {
  name = string
}
# Any other property is accepted without type checking
```

**Validate additional properties against a type:**

```motly
Types: {
  MetadataEntry: {
    Required: { key = string  value = string }
  }
}
Required: {
  name = string
}
Additional = MetadataEntry
```

With typed `Additional`, unknown properties must conform to the specified type. This is useful for dictionaries or extensible configurations:

```motly
# A "labels" type: arbitrary string keys mapping to string values
Types: {
  Labels: { Additional = string }
}
Required: {
  metadata: {
    Required: { name = string }
    Optional: { labels = Labels }
  }
}
```

**Summary:**

| Form | Behavior |
|------|----------|
| *(no `Additional`)* | Reject unknown properties |
| `Additional` | Allow any additional properties |
| `Additional = allow` | Allow any additional properties (explicit) |
| `Additional = reject` | Reject unknown properties (explicit) |
| `Additional = TypeName` | Validate additional properties as `TypeName` |

## Error Codes

| Code | Description |
|------|-------------|
| `missing-required` | A required property is not present |
| `wrong-type` | Property value has incorrect type |
| `unknown-property` | Property not defined in schema (and `Additional` not set) |
| `invalid-schema` | Schema itself is invalid (unknown type name, bad regex) |
| `invalid-enum-value` | Value not in the allowed enum values |
| `pattern-mismatch` | String doesn't match the required regex pattern |

## Complete Example

**Schema (`app-schema.motly`):**

```motly
Types: {
  LogLevel = [debug, info, warn, error]
  Semver.matches = "^\\d+\\.\\d+\\.\\d+$"
}
Required: {
  app: {
    Required: {
      name = string
      version = Semver
    }
    Optional: {
      debug = boolean
    }
  }
  server: {
    Required: {
      host = string
      port = number
    }
    Optional: {
      ssl = boolean
      timeout = number
    }
  }
}
Optional: {
  features = "string[]"
  logLevel = LogLevel
  metadata = tag
}
```

**Configuration (`app.motly`):**

```motly
app: {
  name = "My Application"
  version = "1.2.0"
  debug = @false
}

server: {
  host = localhost
  port = 8080
  ssl = @true
}

features = [logging, metrics]
logLevel = info
```

This configuration is valid against the schema: all required properties are present with correct types, optional properties that appear have correct types, and there are no unknown properties.

**Invalid configuration and resulting errors:**

```motly
app: {
  name = "My Application"
  # missing required: version
}

server: {
  host = localhost
  port = "not a number"  # wrong type
}

unknown_prop = hello  # unknown property
logLevel = trace      # invalid enum value
```

| Error | Path | Code |
|-------|------|------|
| Missing required property "version" | `app.version` | `missing-required` |
| Expected type "number" | `server.port` | `wrong-type` |
| Unknown property "unknown_prop" | `unknown_prop` | `unknown-property` |
| Value does not match any allowed enum value | `logLevel` | `invalid-enum-value` |
