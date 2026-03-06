# Mot API — Rust Design Notes

## Draft API

```rust
use std::collections::BTreeMap;
use chrono::{DateTime, FixedOffset};

/// The resolved, consumer-facing read interface to parsed MOTLY data.
/// Wraps the raw MOTLYNode tree into a navigable object where references
/// have been followed, env vars substituted, and deletions consumed.
pub struct Mot {
    inner: Option<MotData>,
}

struct MotData {
    value: Option<MotValue>,
    properties: BTreeMap<String, Mot>,
}

enum MotValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Date(DateTime<FixedOffset>),
    Array(Vec<Mot>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    String,
    Number,
    Boolean,
    Date,
    Array,
}

impl Mot {
    // --- Existence ---

    pub fn exists(&self) -> bool;
    pub fn has(&self, path: &[&str]) -> bool;

    // --- Navigation ---

    /// Single-step navigation. Returns None if the property does not exist.
    pub fn get(&self, key: &str) -> Option<&Mot>;

    /// Multi-step convenience. Equivalent to chained get()?.get()?.
    pub fn get_path(&self, path: &[&str]) -> Option<&Mot>;

    // --- Value type ---

    pub fn value_type(&self) -> Option<ValueType>;

    // --- Typed accessors ---

    pub fn text(&self) -> Option<&str>;
    pub fn number(&self) -> Option<f64>;
    pub fn boolean(&self) -> Option<bool>;
    pub fn date(&self) -> Option<DateTime<FixedOffset>>;

    // --- Array access ---

    pub fn values(&self) -> Option<&[Mot]>;
    pub fn texts(&self) -> Option<Vec<&str>>;
    pub fn numbers(&self) -> Option<Vec<f64>>;
    pub fn booleans(&self) -> Option<Vec<bool>>;
    pub fn dates(&self) -> Option<Vec<DateTime<FixedOffset>>>;

    // --- Property enumeration ---

    pub fn keys(&self) -> impl Iterator<Item = &str>;
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Mot)>;
}

/// Construct a Mot from a parsed MOTLYNode tree.
pub fn build_mot(
    root: &MOTLYNode,
    env: Option<&HashMap<String, String>>,
) -> Mot;
```

## Usage

```rust
let errors = session.parse(source);
let mot = session.get_mot(Some(&env));

// Single-step chaining with ?
let port = mot.get("server")?.get("port")?.number();   // Option<f64>
let host = mot.get("server")?.get("host")?.text();     // Option<&str>

// Multi-step convenience
let port = mot.get_path(&["server", "port"])
    .and_then(|m| m.number());                          // Option<f64>

// Array convenience
let tags = mot.get("config")?.get("tags")?.texts();    // Option<Vec<&str>>

// Existence check
if mot.has(&["server", "ssl"]) { /* ... */ }

// Enumeration
for (key, child) in mot.entries() {
    println!("{}: {:?}", key, child.value_type());
}
```

## Design Critique

### get() returns Option, not a Null Object

The TypeScript API uses an Undefined Mot singleton so `.get()` never returns
undefined — it always returns a Mot, enabling `config.get("a", "b", "c").number`
with no null checks. This doesn't translate well to Rust:

- A `&'static Mot` singleton has lifetime mismatch with `&self` borrows
- Storing an Undefined Mot in every node or threading one from the root adds overhead
- Returning `Option<&Mot>` is idiomatic Rust and plays well with `?` and `and_then`

The tradeoff: two extra characters per step (`?.`), but every Rust developer
already knows the pattern.

### &[&str] paths vs single-key get()

`get(&["server", "port"])` has more ceremony than TS's variadic args. Rust
doesn't have variadic functions. Providing both `get(key)` and `get_path(path)`
covers both the common case and the deep-navigation case. A macro could help
but macros in public APIs have discoverability problems.

### Convenience array accessors allocate

`texts() -> Option<Vec<&str>>` allocates a Vec on every call. Options:
- Return an iterator: `Option<impl Iterator<Item = &str>>` — but
  Option-of-iterator ergonomics are rough
- Accept the allocation — config trees are small, this is rarely hot
- Cache on first access with interior mutability (`OnceCell`) — adds complexity

Recommendation: accept the allocation for now, same as TypeScript.

### Date type

MOTLY dates can include timezone offsets (`@2024-01-15T10:00:00+05:00`), so
`DateTime<FixedOffset>` from chrono is more correct than `NaiveDate`. This does
pull in chrono as a dependency. Alternative: store dates as a custom type that
wraps the parsed components, avoiding the chrono dependency for consumers who
don't need it.

### Circular refs and ownership

In TypeScript, circular refs work because Mots are heap objects with shared
references. In Rust, `BTreeMap<String, Mot>` owns its children — circular
ownership is impossible without indirection.

Options:
- `Rc<Mot>` — shared ownership, but changes the entire API surface
- Arena allocation — Mots live in a `Vec<MotData>`, properties store indices.
  Clean and cache-friendly, but Mot becomes a handle not a value
- Detect cycles during `build_mot`, produce `inner: None` for the back-edge —
  this is what the TS code effectively does for pure cycles

Recommendation: arena allocation if circular refs matter, or just produce
undefined-mots for back-edges (matching TS behavior) if they don't.

### exists() is redundant with Option

Since `get()` returns `Option<&Mot>`, `exists()` is only useful on a Mot you
already have — i.e., the root. For navigation, `.is_some()` on the Option
replaces `exists`. Could remove `exists()` from the Rust API entirely, or keep
it for parity with other language bindings.
