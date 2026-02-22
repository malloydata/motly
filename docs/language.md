# MOTLY Language Reference

This is the complete, implementation-agnostic specification for the MOTLY configuration language.

## Values

### Strings

**Bare strings** don't need quotes. They can contain letters (`A-Z`, `a-z`), digits (`0-9`), underscores (`_`), and extended Latin characters (accented characters like `é`, `ñ`, `ü`):

```motly
name = hello
color = blue
log_level = info
café = open
```

Strings containing any other characters must be quoted.

**Double-quoted strings** support escape sequences:

```motly
message = "Hello, World!"
path = "/usr/local/bin"
hostname = "db.example.com"
tab_separated = "col1\tcol2"
```

Escape sequences: `\n` (newline), `\r` (carriage return), `\t` (tab), `\b` (backspace), `\f` (form feed), `\\` (backslash), `\"` (double quote), `\uXXXX` (Unicode code point). Any other `\x` produces the literal character `x`.

**Single-quoted strings** are raw -- backslashes are literal, not escape characters. The only special case is `\'`, which includes a literal backslash and prevents the quote from ending the string:

```motly
regex = 'foo\d+bar'
windows_path = 'C:\Users\name'
escaped_quote = 'it\'s raw'  # value: it\'s raw
```

**Triple-double-quoted strings** (`"""..."""`) span multiple lines and support escape sequences:

```motly
description = """
This is a long description
that spans multiple lines.
It preserves newlines.
"""
```

**Triple-single-quoted strings** (`'''...'''`) span multiple lines with raw semantics (backslashes are literal):

```motly
regex_block = '''
^(?:https?://)
[a-z0-9\-]+
\.example\.com$
'''
```

**Heredoc strings** (`<<<...>>>`) span multiple lines with raw semantics. The indentation of the first non-empty line sets the baseline — that amount of leading whitespace is stripped from all subsequent lines. This makes heredocs clean to use inside nested configuration without artificial left-alignment:

```motly
server: {
  database: {
    setupSQL = <<<
      SET search_path TO analytics;
      CREATE TEMP TABLE foo
        AS SELECT 1;
    >>>
  }
}
# setupSQL value is:
#   SET search_path TO analytics;
#   CREATE TEMP TABLE foo
#     AS SELECT 1;
```

Heredocs are useful for embedding large blocks of content like SQL, templates, or other languages where you don't want to worry about quote characters or indentation fighting your config structure:

```motly
template = <<<
  Dear {{name}},
  Your order #{{id}} has been shipped.
  Path: C:\Users\docs\receipt.txt
>>>
```

**Backtick-quoted strings** (`` `...` ``) are used for property names that contain characters not allowed in bare strings. They support escape sequences:

```motly
`content-type` = "application/json"
`my.dotted.key` = value
```

### Numbers

Numbers are parsed as native numeric values:

```motly
port = 8080
rate = 0.05
temperature = -40
fractional = .5
scientific = 1.5e10
negative_exp = 3.14E-2
```

If a token starts with digits but continues with letters, it is treated as a bare string (e.g., `v2` is the string `"v2"`, not a number). To force a numeric-looking value to be a string, use quotes: `zip = "01234"`.

### Booleans

Booleans use the `@` prefix to avoid ambiguity with bare strings:

```motly
enabled = @true
debug = @false
```

### None

The `@none` literal represents the absence of a value. It is used to clear the value slot of a node:

```motly
name = hello
name = @none   # name now has no value (but its properties, if any, are untouched)
```

### Dates

Dates use ISO 8601 format with the `@` prefix:

```motly
# Date only
created = @2024-01-15

# Date and time (UTC)
updated = @2024-01-15T10:30:00Z

# Date and time with timezone offset
scheduled = @2024-01-15T10:30:00+05:00

# With fractional seconds
precise = @2024-01-15T10:30:00.123Z
```

### Arrays

Arrays are enclosed in square brackets. Elements are separated by commas. Trailing commas are allowed:

```motly
# Strings (bare words in arrays are always strings, never property names)
colors = [red, green, blue]

# Numbers
ports = [80, 443, 8080]

# Mixed types
config = [@true, 42, hello, @2024-01-15]

# Trailing comma
items = [one, two, three,]

# Empty array
nothing = []
```

Array elements can be objects:

```motly
users = [
  { name = alice  role = admin },
  { name = bob    role = user }
]
```

Arrays can be nested:

```motly
matrix = [[1, 2], [3, 4]]
```

Array elements can have both a value and properties:

```motly
items = [
  widget { color = red  size = 10 },
  gadget { color = blue size = 20 }
]
```

## Nodes, Values, and Properties

Every named entry in MOTLY is a **node**. A node has two independent aspects:

- A **value** — a scalar (string, number, boolean, date, `@none`, or reference)
- **Properties** — a map of child nodes

This dual nature reflects how humans naturally think about things. A font isn't a bag of attributes with a `family` field — it *is* Helvetica, and it *has* properties:

```motly
font = Helvetica { size = 14  weight = bold }
```

In JSON you'd write `{"font": {"family": "Helvetica", "size": 14, "weight": "bold"}}` — burying the most important fact inside a property. MOTLY lets the primary identity of a thing be its value, with properties as secondary detail. Configuration is for humans, and humans say "the font is Helvetica" not "the font has a family field whose value is Helvetica."

These two aspects are controlled independently by different operators. Understanding this separation is the key to understanding MOTLY's assignment syntax.

## The Three Core Operators

MOTLY has three assignment operators. Each controls a different combination of a node's value and properties:

- **`=`** operates **only on the value slot**. It never touches properties.
- **`:`** operates **only on the properties slot**. It never touches the value.
- **`:=`** operates on **both** simultaneously.

## The Assignment Matrix

Every combination of value and property manipulation is covered by a simple, orthogonal set of gestures:

| | Assign value | Keep value | Remove value |
|---|---|---|---|
| **Keep properties** | `name = val` | `name` | `name = @none` |
| **Merge properties** | `name = val { }` | `name { }` | `name = @none { }` |
| **Replace properties** | `name := val { }` | `name: { }` | `name := @none { }` |

The operators compose predictably:

- Need to change just the value? Use `=`.
- Need to change just the properties? Use `:` or space-before-brace.
- Need to change both? Use `:=`.
- Need to clear the value? Assign `@none`.

### Example: Assigning Values with `=`

The `=` operator sets the value of a node without affecting its properties:

```motly
port = 8080
name = hello
enabled = @true
```

If the node already has properties, they are preserved:

```motly
server = webhost { port = 8080  ssl = @true }

# Change the value, properties are untouched
server = apphost

# Result: server is "apphost" with properties { port = 8080, ssl = @true }
```

### Example: Assigning Values and Merging Properties with `= val { }`

When `=` is followed by a value and then braces, the value is assigned and the properties in the braces are **merged** with any existing properties:

```motly
server = webhost { port = 8080 }

# Assign new value and merge additional properties
server = apphost { ssl = @true }

# Result: server is "apphost" with properties { port = 8080, ssl = @true }
```

### Example: Replacing Properties with `:`

The colon operator replaces all properties on a node without affecting its value:

```motly
server: { host = localhost  port = 8080 }

# This REPLACES everything in server
server: { url = "http://example.com" }

# Result: server only has url (host and port are gone)
```

### Example: Merging Properties with Space-Before-Brace

A name followed by braces (no operator) merges with existing properties:

```motly
server: { host = localhost }

# This ADDS to server
server { port = 8080 }

# Result: server has both host and port
```

### Example: Assigning Both with `:=`

The `:=` operator assigns the value **and** replaces properties in a single gesture:

```motly
name := car { color = red  year = 2024 }
```

This sets the value to `car` and replaces all properties with `{ color = red  year = 2024 }`. An optional trailing `{ }` block after `:=` **merges** overrides on top of the replaced properties (see "Cloning with `:=`" under References).

### Summary: Replace vs. Merge

Replace is the normal mode for defining configuration — you're stating the complete set of properties. Merge is useful when extending or overriding configuration from multiple sources, or when a session accumulates statements incrementally.

| Syntax | Properties behavior |
|--------|---|
| `name: { }` | Replace |
| `name { }` | Merge |
| `name := val { }` | Replace (then merge if trailing `{ }`) |
| `name = val { }` | Merge |

## Deep Path Notation

Use dot notation to set deeply nested values without nesting braces:

```motly
database.connection.pool.max = 100
database.connection.pool.min = 10
database.connection.timeout = 5000
```

This is equivalent to:

```motly
database: {
  connection: {
    pool: {
      max = 100
      min = 10
    }
    timeout = 5000
  }
}
```

## Deletion

Remove a property with `-`:

```motly
server: {
  host = localhost
  port = 8080
  debug = @true
}

# Delete the debug property
-server.debug
```

Remove all properties from the current scope with `-...`:

```motly
config: {
  a = 1
  b = 2
  c = 3
}

# Remove everything
config { -... }
```

## Flags (Define)

A bare name with no value, `=`, or braces creates a "flag" -- a node that exists but has no value or properties:

```motly
hidden
deprecated
experimental
```

This is useful for presence-based configuration where the existence of a name is meaningful.

## References

References allow one value to point to another value in the tree.

### Absolute References

Use `$` followed by a dotted path to reference from the root:

```motly
defaults: {
  timeout = 30
  retries = 3
}

api: {
  timeout = $defaults.timeout
  retries = $defaults.retries
}
```

### Relative References

Use `^` to go up levels from the reference's location:

```motly
server: {
  host = localhost

  endpoints: {
    api: {
      # $^ goes up one level (to endpoints)
      # $^^ goes up two levels (to server)
      url = $^^host
    }
  }
}
```

| Syntax | Meaning |
|--------|---------|
| `$path` | Absolute path from root |
| `$^path` | Up one level, then follow path |
| `$^^path` | Up two levels, then follow path |

### Array Indexing in References

Reference specific array elements with brackets:

```motly
users = [
  { name = alice  role = admin },
  { name = bob    role = user }
]

primary_user = $users[0].name
```

### References with `=` (Pointers)

When used with `=`, a reference creates a **pointer** — a live link to another node:

```motly
name = $ref              # value becomes a pointer to ref. Properties untouched.
name = $ref { color = red }  # value becomes a pointer to ref. Merge these properties.
```

### Cloning with `:=` (Copy)

When used with `:=`, a reference is **dereferenced and cloned** — the value and entire property subtree are copied into the target:

```motly
name := $ref             # clone ref's value AND properties into name
name := $ref { color = red } # clone everything from ref, then merge overrides
```

The difference between `= $ref` and `:= $ref` is the difference between "point at it" and "copy it."

This is especially useful for configuration modes, where a mode starts as a copy of a base config and then overrides specific values:

```motly
connections: {
  cache: { host = redis.internal  port = 6379 }
  db: { host = localhost  port = 5432  username = dev }
}

modes: {
  staging := $connections {
    db { host = staging-db.internal  username = staging_svc }
  }
  production := $connections {
    db { host = prod-db.internal  username = prod_svc }
  }
}
```

Loading `modes.staging` yields a complete connection map with both `cache` (cloned from the base) and `db` (cloned then overridden).

### Clone Boundary Rule

When `:=` clones a subtree, all references within the cloned subtree must resolve within the subtree itself. If a relative reference would resolve to a node outside the cloned subtree, that is a compile error.

A clone is always a self-contained snapshot. If you need to refer to something outside the subtree, use a concrete value rather than a relative reference that escapes the clone boundary.

```motly
# OK — internal reference resolves within the cloned subtree
base: {
  shared_host = "db.internal"
  primary: { host = $^shared_host }
}
copy := $base   # works: $^shared_host resolves within base

# ERROR — relative reference escapes the clone boundary
root_setting = important
other: {
  val = $^^root_setting   # points outside other
}
copy := $other   # error: $^^root_setting resolves outside the cloned subtree
```

## Comments

Line comments start with `#`. Everything from `#` to the end of the line is ignored:

```motly
# This is a comment
server: {
  host = localhost  # inline comment
  port = 8080
}
```

There are no block comments.

## Schema Directive

> **Note:** Schema validation is planned for a future version of MOTLY. The syntax below describes the intended convention for declaring schemas. It is documented here to establish the design direction, but tooling support is not yet implemented.

A MOTLY file can declare its schema on the first line using the `#!` convention:

```motly
#! schema=app-config url="./schemas/app.motly"
name = "My Application"
port = 8080
```

The `#!` line is a comment as far as the parser is concerned (the `#` makes it a comment). The convention is that tools strip the `#!` prefix and parse the remainder as MOTLY to extract:

- `schema` -- a short identifier code for the schema
- `url` -- location of the schema file (URL or relative path)

A file may specify just `schema`, just `url`, or both:

```motly
#! schema=well-known-config
#! url="./local-schema.motly"
#! schema=x-acme url="https://example.com/schema.motly"
```

Use the `x-` prefix for organization-specific schema codes (e.g., `x-acme-deploy`).

## Syntax Quick Reference

| Syntax | Description | Example |
|--------|-------------|---------|
| `key = value` | Assign value (keep props) | `port = 8080` |
| `key = val { }` | Assign value, merge props | `server = web { port = 80 }` |
| `key: { ... }` | Replace properties (keep value) | `server: { host = localhost }` |
| `key { ... }` | Merge properties (keep value) | `server { port = 8080 }` |
| `key := val { }` | Assign value, replace props | `server := web { port = 80 }` |
| `key = @none` | Remove value (keep props) | `name = @none` |
| `key = [a, b]` | Array | `ports = [80, 443]` |
| `key.sub = val` | Deep path | `db.pool.max = 10` |
| `"quoted"` | Double-quoted string | `host = "my-app.com"` |
| `'raw'` | Single-quoted raw string | `regex = 'foo\d+'` |
| `"""multi"""` | Triple-quoted string | `desc = """..."""` |
| `'''raw multi'''` | Triple-single-quoted raw string | `block = '''...\n...'''` |
| `<<<...>>>` | Heredoc raw string (auto-dedent) | `sql = <<<SELECT 1;>>>` |
| `` `backtick` `` | Quoted property name | `` `content-type` = json `` |
| `@true` / `@false` | Boolean | `enabled = @true` |
| `@none` | No value | `name = @none` |
| `@2024-01-15` | Date | `created = @2024-01-15` |
| `name` | Flag (define) | `hidden` |
| `-key` | Delete property | `-deprecated_field` |
| `-...` | Delete all properties | `-...` |
| `$path` | Reference (absolute) | `timeout = $defaults.timeout` |
| `$^path` | Reference (relative) | `host = $^^server.host` |
| `$arr[0]` | Reference with index | `first = $items[0]` |
| `= $ref` | Reference (pointer) | `link = $other.node` |
| `:= $ref` | Clone (copy) | `copy := $base` |
| `# comment` | Line comment | `# This is a comment` |
| `#! ...` | Schema directive | `#! schema=app url="..."` |

## Grammar

The formal EBNF grammar is maintained in a separate file: [motly-grammar.md](motly-grammar.md).
