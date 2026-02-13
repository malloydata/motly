# MOTLY

MOTLY is a lightweight, human-friendly configuration language. It originated as the annotation/tag language in [Malloy](https://github.com/malloydata/malloy) and is designed for readability: no quotes on simple strings, no commas, explicit braces instead of significant whitespace, and native types for booleans and dates.

While MOTLY is indeed the syntax for the Malloy "Tag" language, it is growing into an experiment in configuration languages. The plan is for MOTLY + MOTLY Schema to produce native DOM-like access to configuration data with re-serialization.

MOTLY isn't really an acronym, but if it were, it could stand for:

- **M**alloy **O**bject **T**agging **L**anguage, **Y**ahoo!
- **M**ore **O**bjects **T**han **L**ines, **Y**ippee!
- **M**arkup **O**bjects **T**ersely, **L**ike **Y**AML
- **M**akes **O**ther **T**hings **L**ook **Y**ucky
- **M**ight **O**vertake **T**OML **L**ater, **Y**'know

It is spelled MOTLY (all upper case), like YAML, TOML and JSON though.

## Quick Example

```motly
# Application configuration
app: {
  name = "My Application"
  version = 1.2
  debug = @false

  server: {
    host = localhost
    port = 8080
    ssl = @true
  }

  features = [logging, metrics, caching]

  scheduled_maintenance = @2024-06-15T02:00:00Z
}
```

## Why MOTLY?

| | JSON | YAML | TOML | MOTLY |
|---|---|---|---|---|
| Quotes on simple keys/values | Required | No | Keys no, values yes | No |
| Separators | Commas required | None | None | None |
| Nesting | Braces | Indentation | Section headers | Braces |
| Comments | No | Yes | Yes | Yes |
| Native booleans | `true`/`false` | Many (`yes`, `on`, ...) | `true`/`false` | `@true`/`@false` |
| Native dates | No | Yes (ambiguous) | Yes | `@2024-01-15` |
| Copy-paste safe | Yes | No (whitespace) | Partially | Yes |

MOTLY uses explicit braces like JSON (no whitespace surprises like YAML) but drops the syntactic noise (no quotes on simple strings, no commas, no colons between keys and values). Booleans and dates are unambiguous with the `@` prefix.

## Status

MOTLY is under active development. The language itself is solid -- [Malloy](https://malloydata.dev) uses the MOTLY parser for its [tagged annotations](https://docs.malloydata.dev/documentation/language/tags).

**What works today (0.0.1):**
- Full parser and interpreter (Rust and pure TypeScript, kept in sync)
- Schema validation with custom types, enums, patterns, unions, recursive types
- Reference system (`$root.path`, `$^sibling`, `$^^grandparent`, indexed access)
- [`@malloydata/motly-ts-parser`](bindings/typescript/parser/) -- the TypeScript parser, published on npm with zero native dependencies

**Where it's going:**
- **DOM API** -- schema-driven, language-native DOM bindings. You define a MOTLY schema, you get back a typed DOM with getters, setters, and validation -- like an ORM for configuration. This replaces the current `getValue()` plain-object approach and eliminates the DOM layer that Malloy builds on top today.
- **Schema metadata** -- extending schemas to carry UI metadata (labels, placeholders, secret flags) for driving dynamic UI generation.
- **WASM backend** -- the Rust parser compiled to WASM, behind the same TypeScript API. Drop-in performance upgrade.
- **Multi-language bindings** -- once the Rust DOM exists, Python/Ruby/Go bindings come naturally through FFI.

See [VISION.md](VISION.md) for the full roadmap.

## Documentation

- **[Language Reference](docs/language.md)** -- complete syntax and semantics
- **[Schema Validation Reference](docs/schema.md)** -- validating MOTLY files against schemas
- **[Future Plans](VISION.md)** -- DOM API, schema metadata, and long-term direction

## Packages

- **Rust**: `cargo doc --open` -- Rust library and CLI ( not published yet )
- **TypeScript**: [`@malloydata/motly-ts-parser`](bindings/typescript/parser/) -- pure TypeScript parser (zero native deps)

The file extension is `.motly`.
