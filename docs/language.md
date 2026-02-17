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

## Objects

### Basic Nesting

Objects are created with colon and braces:

```motly
server: {
  host = localhost
  port = 8080
}
```

### Deep Path Notation

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

### Replace vs Merge

This is a key MOTLY concept. There are two ways to set properties on a name, and they differ in how they interact with existing data:

**Colon syntax (`: { }`) replaces all properties:**

```motly
server: { host = localhost  port = 8080 }

# This REPLACES everything in server
server: { url = "http://example.com" }

# Result: server only has url (host and port are gone)
```

**Space syntax (`{ }`) merges with existing properties:**

```motly
server: { host = localhost }

# This ADDS to server (merge)
server { port = 8080 }

# Result: server has both host and port
```

Replace is the normal mode for defining configuration. Merge is useful when extending or overriding configuration from multiple sources, or when a session accumulates statements incrementally.

### Equals-with-braces Syntax

When `=` is followed by braces, it also replaces properties (like `:`):

```motly
# These are equivalent:
name: { a = 1 }
name = { a = 1 }
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

## Preserve Semantics

MOTLY provides syntax for selectively preserving a node's value or properties during an update.

**Preserve value** (`= ... { }`): replaces the properties but keeps the existing scalar value:

```motly
name = hello { color = red }

# Replace properties but keep the value "hello"
name = ... { color = blue  size = 10 }

# Result: name is "hello" with properties { color = blue, size = 10 }
```

**Preserve properties** (`= val { ... }`): changes the scalar value but keeps existing properties:

```motly
name = hello { color = red  size = 10 }

# Change value to "world" but keep properties
name = world { ... }

# Result: name is "world" with properties { color = red, size = 10 }
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
| `key = value` | Assign a value | `port = 8080` |
| `key: { ... }` | Object (replace) | `server: { host = localhost }` |
| `key { ... }` | Object (merge) | `server { port = 8080 }` |
| `key = [a, b]` | Array | `ports = [80, 443]` |
| `key.sub = val` | Deep path | `db.pool.max = 10` |
| `"quoted"` | Double-quoted string | `host = "my-app.com"` |
| `'raw'` | Single-quoted raw string | `regex = 'foo\d+'` |
| `"""multi"""` | Triple-quoted string | `desc = """..."""` |
| `'''raw multi'''` | Triple-single-quoted raw string | `block = '''...\n...'''` |
| `` `backtick` `` | Quoted property name | `` `content-type` = json `` |
| `@true` / `@false` | Boolean | `enabled = @true` |
| `@2024-01-15` | Date | `created = @2024-01-15` |
| `name` | Flag (define) | `hidden` |
| `-key` | Delete property | `-deprecated_field` |
| `-...` | Delete all properties | `-...` |
| `= ... { }` | Preserve value | `n = ... { color = blue }` |
| `= val { ... }` | Preserve properties | `n = world { ... }` |
| `$path` | Reference (absolute) | `timeout = $defaults.timeout` |
| `$^path` | Reference (relative) | `host = $^^server.host` |
| `$arr[0]` | Reference with index | `first = $items[0]` |
| `# comment` | Line comment | `# This is a comment` |
| `#! ...` | Schema directive | `#! schema=app url="..."` |

## Grammar (EBNF)

Commas are optional separators between statements (at the top level and inside `{ }` blocks). They are treated as whitespace in those contexts. In arrays, commas remain required element separators.

```ebnf
(* Entry point — commas are optional separators between statements *)
document        ::= statementList
statementList   ::= { "," } { statement { "," } }

(* Statements *)
statement       ::= assignment
                  | replaceProps
                  | updateProps
                  | clearAll
                  | definition

assignment      ::= propName "=" value [ properties ]
replaceProps    ::= propName ":" properties
                  | propName "=" [ "..." ] properties
updateProps     ::= propName properties
definition      ::= [ "-" ] propName
clearAll        ::= "-..."

(* Property paths *)
propName        ::= identifier { "." identifier }

(* Values *)
value           ::= array | boolean | date | number | string | reference

boolean         ::= "@true" | "@false"
date            ::= "@" isoDate
number          ::= [ "-" ] digits [ "." digits ] [ exponent ]
                  | [ "-" ] "." digits [ exponent ]
string          ::= tripleString | tripleSingleString | sqString | dqString | bareString
reference       ::= "$" { "^" } refPath
refPath         ::= refElement { "." refElement }
refElement      ::= identifier [ "[" digits "]" ]

exponent        ::= ( "e" | "E" ) [ "+" | "-" ] digits
digits          ::= digit { digit }
digit           ::= "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"

(* ISO 8601 date/datetime *)
isoDate         ::= year "-" month "-" day [ "T" time [ timezone ] ]
time            ::= hour ":" minute [ ":" second [ "." fraction ] ]
timezone        ::= "Z" | ( "+" | "-" ) hour [ ":" ] minute
year            ::= digit digit digit digit
month           ::= digit digit
day             ::= digit digit
hour            ::= digit digit
minute          ::= digit digit
second          ::= digit digit
fraction        ::= digits

(* Arrays *)
array           ::= "[" [ arrayElements ] "]"
arrayElements   ::= arrayElement { "," arrayElement } [ "," ]
arrayElement    ::= scalarValue [ properties ]
                  | properties
                  | array

scalarValue     ::= boolean | date | number | string

(* Properties block *)
properties      ::= "{" statementList "}"
                  | "{" "..." "}"

(* Identifiers — for property names *)
identifier      ::= bqString | bareString

(* String literals *)
bareString      ::= bareChar { bareChar }
bareChar        ::= letter | digit | "_"
letter          ::= "A"-"Z" | "a"-"z" | extendedLatin
extendedLatin   ::= (* Unicode: U+00C0–U+024F, U+1E00–U+1EFF *)

tripleString    ::= '"""' { tripleChar } '"""'
tripleChar      ::= (* any character except unescaped """, or escape sequence *)

tripleSingleString ::= "'''" { tripleSingleChar } "'''"
tripleSingleChar   ::= (* any character; backslash pairs with next char; only ''' closes *)

dqString        ::= '"' { dqChar } '"'
dqChar          ::= (* any character except ", \, newline, or escape sequence *)

sqString        ::= "'" { sqChar } "'"
sqChar          ::= (* any character except ', newline; backslash pairs with next char literally *)

bqString        ::= "`" { bqChar } "`"
bqChar          ::= (* any character except `, \, newline, or escape sequence *)

(* Escape sequences (dqString, tripleString, bqString): \b \f \n \r \t \uXXXX \char *)
(* Raw strings (sqString, tripleSingleString): backslash is literal but pairs with next char *)

(* Whitespace and comments — allowed between tokens *)
whitespace      ::= " " | "\t" | "\r" | "\n"
comment         ::= "#" { (* any char except newline *) } newline
```
