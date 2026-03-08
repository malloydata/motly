# Mot API — Python Design Notes

## Draft API

```python
from __future__ import annotations
from datetime import datetime
from typing import Optional, Iterator

class Mot:
    """Resolved, consumer-facing read interface to parsed MOTLY data.

    Wraps the raw parse tree into a navigable object where references
    have been followed, env vars substituted, and deletions consumed.
    """

    @property
    def exists(self) -> bool:
        """True for any real node. False only for the Undefined Mot."""
        ...

    def get(self, *props: str) -> Mot:
        """Walk into properties by name. Returns the Undefined Mot if any
        step does not exist. Never returns None."""
        ...

    def has(self, *props: str) -> bool:
        """True if the full property path exists."""
        ...

    # --- Value type ---

    @property
    def value_type(self) -> Optional[str]:
        """Returns 'string', 'number', 'boolean', 'date', 'array',
        or None if the node has no value."""
        ...

    # --- Typed accessors ---

    @property
    def text(self) -> Optional[str]:
        ...

    @property
    def numeric(self) -> Optional[float]:
        ...

    @property
    def boolean(self) -> Optional[bool]:
        ...

    @property
    def date(self) -> Optional[datetime]:
        ...

    # --- Array access ---

    @property
    def values(self) -> Optional[list[Mot]]:
        ...

    @property
    def texts(self) -> Optional[list[str]]:
        ...

    @property
    def numerics(self) -> Optional[list[float]]:
        ...

    @property
    def booleans(self) -> Optional[list[bool]]:
        ...

    @property
    def dates(self) -> Optional[list[datetime]]:
        ...

    # --- Property enumeration ---

    @property
    def keys(self) -> Iterator[str]:
        ...

    @property
    def entries(self) -> Iterator[tuple[str, Mot]]:
        ...

    # --- Dunder support ---

    def __contains__(self, key: str) -> bool:
        """Supports `'port' in config.get('server')`."""
        ...

    def __iter__(self) -> Iterator[str]:
        """Iterates property names. Same as .keys."""
        ...

    def __len__(self) -> int:
        """Number of properties."""
        ...

    def __bool__(self) -> bool:
        """True if exists. Enables `if config.get('server'): ...`."""
        ...

    def __getitem__(self, key: str) -> Mot:
        """Supports `config['server']['port']`. Returns Undefined Mot
        for missing keys (does not raise KeyError)."""
        ...
```

## Usage

```python
session.parse(source)
mot = session.get_mot(env={"API_KEY": "secret"})

# Variadic get + accessor
port = mot.get("server", "port").numeric         # float | None (property style)
host = mot.get("server", "host").text           # str | None

# Pathed shorthand (if method-style accessors adopted)
port = mot.numeric("server", "port")             # float | None
host = mot.text("server", "host")               # str | None
tags = mot.texts("config", "tags")              # list[str] | None

# Pythonic alternatives via dunders
port = mot["server"]["port"].numeric
if "ssl" in mot.get("server"):
    ...

# Enumeration
for key in mot:
    child = mot.get(key)
    print(f"{key}: {child.value_type}")

# Truthiness
if mot.get("optional_section"):
    process(mot.get("optional_section"))
```

## Design Critique

### The Null Object pattern works beautifully in Python

Unlike Rust, Python has no ownership or lifetime constraints. The Undefined Mot
singleton translates directly — `get()` always returns a `Mot`, chaining works
without any `?.` or `Optional` unwrapping. Python's duck typing means the
Undefined Mot just needs to quack right.

This is arguably the best language fit of the three (TS, Rust, Python) for this
API pattern.

### Dunders make it more Pythonic

Python developers expect `__getitem__`, `__contains__`, `__iter__`, `__bool__`.
Adding these makes the API feel native:

```python
if "port" in config["server"]:
    port = config["server"]["port"].numeric
```

The question is whether to support BOTH `get()` and `[]` or pick one. Having
both is fine — `get()` for multi-step paths, `[]` for single steps.

### __getitem__ should NOT raise KeyError

Standard Python dicts raise `KeyError` for missing keys. This Mot API returns
the Undefined Mot instead. This breaks the principle of least surprise for
Python developers who expect `[]` to throw. Document it clearly, or:
- Use `[]` for throwing access, `get()` for safe access (matching dict API)
- Only provide `get()`, skip `__getitem__` entirely

Recommendation: provide `__getitem__` with Undefined Mot semantics (matching
the TypeScript behavior), but document the divergence from dict. Config
navigation with KeyError would be painful — the whole point of this API is
safe traversal.

### __bool__ is a double-edged sword

`if mot.get("x"):` is ergonomic, but it means a flag node (exists, no value)
is truthy while a missing node is falsy. This matches `exists`. But Python
developers might expect `bool(mot)` to reflect the *value* — e.g., a node
with `value = @false` would still be truthy because `exists` is True. This
could surprise people. Document it clearly.

### Optional[float] loses int vs float distinction

MOTLY numbers are all f64. Python has separate `int` and `float`. Returning
`Optional[float]` means `port = 8080` comes back as `8080.0`. Options:
- Return `int | float` based on whether the number is whole
- Always return `float` (matches the data model)
- Return a custom Number type

Recommendation: return `int` when the value is a whole number, `float`
otherwise. This matches how JSON libraries behave in Python and avoids the
`8080.0` surprise. The `value_type` is still `"number"` either way.

### property vs method for accessors

The TypeScript API now uses **methods** for all value accessors (`text()`,
`number()`, etc.) rather than getter properties. This was done so that:
1. Implementations can add side effects (read tracking) transparently
2. Accessors can accept optional path arguments: `mot.text("server", "host")`
3. Tag (Malloy's read-tracked wrapper) can implement the Mot interface

For Python, the choice is open. `@property` is more Pythonic for simple reads,
but methods would match TS and enable pathed accessors:

```python
# Property style (Pythonic, but no path shorthand)
port = config.get("server", "port").numeric

# Method style (matches TS, enables shorthand)
port = config.numeric("server", "port")
```

`.keys` and `.entries` as properties returning iterators feels slightly off —
Python's `dict.keys()` and `dict.items()` are method calls. Consider using
`items` instead of `entries` to match Python's dict vocabulary.

### Type annotations and mypy

The API is fully annotatable, which is good. Using `from __future__ import
annotations` enables forward references for `Mot` in return types. A `py.typed`
marker file should be included for PEP 561 compliance so mypy/pyright users
get type checking out of the box.

### Implementation path

If the Rust implementation comes first, the Python binding would likely be
built with PyO3/maturin, wrapping the Rust `Mot`. The Null Object pattern
maps cleanly through PyO3 — the Undefined Mot would be a Python-side singleton
that the Rust binding returns when navigation fails.

Alternative: pure Python implementation (like the pure TS parser), which
would be simpler to distribute but slower.
